# Giao thức đồng bộ change journal

Trạng thái: **Blueprint quy chuẩn PLANNED**

Tài liệu này đặc tả mô hình đồng bộ đa thiết bị của Synveil. Tài liệu tuân theo
ADR-006 và từ vựng domain trong [DOMAIN_MODEL.md](DOMAIN_MODEL.md). Quy tắc
object chuẩn và Trash nằm trong [STORAGE.md](STORAGE.md); truyền content nằm
trong [UPLOADS.md](UPLOADS.md). Backup được cố ý tách riêng và đặc tả trong
[BACKUP.md](BACKUP.md).

Giao thức được thiết kế cho web, desktop dùng chung core Rust và client Apple,
với Android, iPhone và iPad là mobile profile tương lai. Tài liệu không tuyên bố
các client đó đã được implement. Windows, macOS, Linux Desktop và Linux Server
chỉ là first-class host target khi client/service evidence gate của chúng đạt.

## Sync so với backup

Synchronization hội tụ một `Library` về một logical namespace hiện tại. Create,
edit, rename, move, Trash và restore lan truyền tới các thiết bị. Backup chụp các
historical manifest bất biến theo retention; source vắng mặt trong backup mới
không phá hủy snapshot trước.

Vì vậy backup chỉ-upload không phải sync filter và sync tombstone không phải
backup-retention command. API, table, event, status và test giữ hai domain này
tách biệt.

## Bất biến giao thức

1. Mỗi `Library` có một journal epoch và một change head `BIGINT` tăng trong
   transaction. Sequence chỉ có nghĩa cùng library+epoch đó.
2. Cấp sequence được tuần tự hóa bằng row `sync_head` đã lock trong cùng
   transaction với domain mutation và event insert. Nó không phải bare
   PostgreSQL sequence và không thể nhìn thấy trước khi event commit.
3. Nếu client nhận cursor tại sequence `n`, mọi committed event có sequence
   `<= n` đều nhìn thấy trong journal epoch đó. Transaction sau không thể commit
   sequence được cấp trước đó phía sau cursor.
4. Cursor là server token mờ đục, có version, đã xác thực. Nó không phải
   authorization capability và client không bao giờ parse, increment hay tạo nó.
5. Change page có thể được giao lại. Client áp dụng trọn page và lưu returned
   cursor trong một local transaction.
6. Mọi logical mutation mang `client_mutation_id` ổn định và base
   version/revision phù hợp operation. Mutation lặp lại trả persisted outcome và
   không append event khác.
7. Content conflict bảo toàn cả hai verified byte stream. Metadata conflict trả
   current authoritative state để rebase tường minh; không có silent
   last-writer-wins.
8. `ChangeEvent` là resulting fact/invalidation bền vững, không phải object-store
   command, authorization grant, arbitrary plugin event hay audit log hoàn chỉnh.
9. Trash phát recursive tombstone cho subtree bị ảnh hưởng. Physical purge và
   object GC không bao giờ giả dạng một client content deletion mới.
10. Cursor cũ hơn retained history fail cùng hướng dẫn rebaseline. Server không
    bao giờ reset nó về zero hay trả partial history như thể hoàn chỉnh.
11. Mọi journal read thiết lập/tăng cursor dùng PostgreSQL write authority. Read
    replica bị lag không thể phục vụ chúng.
12. Thứ tự cross-library không được định nghĩa. Không có global timestamp hay
    Lamport clock mà client có thể dùng để merge library.

## Mô hình journal phía server

### Sync head

Mỗi library có chính xác một record có thể lock tương đương:

```text
library_id
journal_epoch
last_committed_sequence BIGINT
minimum_retained_sequence BIGINT
revision
```

`last_committed_sequence` bắt đầu từ zero. `journal_epoch` chỉ đổi qua
recovery/migration đã review làm invalid cursor trước. Nó không đổi khi process
restart, retention pruning, device revocation hay schema deployment thông
thường.

`BIGINT` exhaustion được guard từ rất lâu trước giới hạn. Cấm reset hoặc wrap
sequence; new epoch có chủ đích cùng client rebaseline là transition duy nhất
được phép.

### Algorithm cấp sequence không tạo gap

Với mutation tạo `k >= 1` journal event:

1. Bắt đầu PostgreSQL transaction trên primary.
2. Reserve và lock unique idempotency receipt (hoặc đọc lại bên thắng sau khi
   unique insert của nó commit). Nếu mutation giống hệt đã commit, trả nó mà
   không chạm domain row hay sync head; nếu fingerprint khác, fail trước
   mutation. Điều này ngăn hai lần giao đầu của cùng client mutation đều đi tới
   domain write.
3. Authorize và lock domain row bị ảnh hưởng theo canonical order đã ghi tài
   liệu. Validate base version/revision, name uniqueness, state, hierarchy,
   quota và object durability. Stage thay đổi domain row trong transaction này.
4. Xác định event group có giới hạn và payload từ resulting state.
5. Cuối transaction, lock row `sync_head` của library này `FOR UPDATE`. Mọi code
   path cấp event tuân theo thứ tự này; không path nào giữ head rồi chờ unordered
   domain lock.
6. Đặt `first = head + 1`, `last = head + k` bằng checked arithmetic; update head
   trong transaction và insert chính xác contiguous event `first..last` với một
   transaction/group ID.
7. Insert audit fact, outbox/job bắt buộc và mutation receipt cùng resulting ID,
   revision, version và sequence interval.
8. Commit trong khi vẫn giữ head lock. Allocator kế tiếp không thể observe hay
   allocate sau interval này cho tới khi commit/rollback release row.

Nếu transaction rollback, head update và event row rollback, nên transaction kế
tiếp có thể reuse number; không tạo cursor-visible gap. Nếu response mất sau
commit, lookup mutation receipt trả cùng interval và result.

Per-library head lock cố ý là điểm correctness serialization ngắn. Hashing,
object write, network request và long validation scan diễn ra trước transaction
hoặc ngoài nó. Clock sharded/multi-writer tương lai cần ADR thay thế và protocol
proof; không thể âm thầm thay rule này.

### Thứ tự lock và cycle directory

Ordinary node lock dùng canonical UUID order sau các required parent/name guard.
Directory move cần predicate protection để chống hai move concurrent tạo cycle,
và subtree Trash không được commit concurrent với unchecked descendant edit.

Correctness profile ban đầu lấy một per-library namespace-mutation advisory lock
có scope transaction (hoặc dedicated guard row tương đương) trước mọi
transaction ngắn mutate `Node`, rồi lock source/target/domain row theo canonical
order. Object upload và hashing diễn ra trước transaction này, nên guard không
bao phủ byte transfer. Điều này cố ý tuần tự hóa logical node commit trong một
library, từ chối move vào self/descendant/cross-library/dưới file và cho
edit-versus-subtree-delete thứ tự xác định. Nó chỉ lấy `sync_head` sau khi domain
mutation sẵn sàng.

Scale revision dựa trên số đo có thể thay coarse guard bằng shared ancestor và
exclusive subtree lock cùng `ltree`, closure table hoặc materialized-path
projection. Nó phải bảo toàn cycle safety, subtree fencing, move atomicity,
stable node ID và established lock order qua conformance test; không âm thầm làm
yếu chúng vì throughput.

## Hợp đồng change event

Mỗi event chứa:

- event ID, library ID, epoch, sequence, event group/transaction ID và event
  schema version;
- kind, subject node ID, resulting node revision/state và current version ID khi
  áp dụng;
- minimal resulting projection như kind, parent ID, display name/name-key
  version, previous parent/name khi cần cho cache invalidation, recursive flag
  hoặc conflict origin;
- safe identity của actor user/device, server commit time và causal
  `client_mutation_id`/operation correlation;
- projection-completeness flag tường minh hoặc resource URL khi client phải
  refetch current state.

Event mang resulting server fact thay vì unsafe path delta. Client có thể bỏ qua
event có resulting node revision cũ hơn revision đã giữ, đồng thời vẫn ghi event
đã áp dụng. Unknown required schema version dừng cursor advancement với
`unsupported_event_version`; unknown optional field bị bỏ qua.

Các kind được đăng ký ban đầu nên gồm:

| Kind | Ý nghĩa bắt buộc |
|---|---|
| `NODE_CREATED` | Identity và projection của file/directory active mới tồn tại. |
| `CONTENT_UPDATED` | File hiện có có current immutable version mới. |
| `NODE_RENAMED` | Cùng node ID và content, display/name comparison projection mới. |
| `NODE_MOVED` | Cùng node ID và content, parent mới; cũng có thể gồm rename. |
| `NODE_TRASHED` | Root bị soft-delete; `recursive=true` invalidate known subtree của nó. |
| `NODE_RESTORED` | Trashed root active tại resulting parent/name. |
| `NODE_PURGED` | Logical Trash record không còn restore được; live tombstone trước vẫn hợp lệ về ngữ nghĩa. |
| `VERSION_RESTORED` | File hiện có có newly created head version lấy từ lịch sử. |
| `CONFLICT_CREATED` | Conflict/recovered node/version nhìn thấy được bảo toàn incoming edit. |

Một logical transaction có thể phát nhiều event. Feed page không tách event
group: `limit` là target và server có thể trả phần còn lại của một bounded group
tới documented hard maximum. Bulk/subtree operation dùng compact root event
hoặc bounded transactional batch thay vì unbounded event group.

Change event được giữ immutable tới khi journal-retention work tăng minimum
sequence. Correction là event/state mới, không phải edit history.

## Format và validation cursor

Public cursor là encoded token mờ đục có authenticated server claim gồm:

```text
token_format_version
signing_key_id
library_id
journal_epoch
position_sequence
optional feed/profile version
```

Encoding được bảo vệ integrity (ví dụ MAC hoặc authenticated envelope) cùng key
rotation có version. Nó không được expose raw database secret hay được chấp nhận
làm authorization. Old signing key vẫn verify được ít nhất trong supported
journal/retry horizon hoặc tạo directed rebaseline; key rotation không thể gây
silent history loss.

Validation phân biệt:

- `invalid_cursor`: malformed, integrity fail, sai library, token version không
  hỗ trợ hoặc cursor vượt committed head;
- `cursor_epoch_mismatch`: token hợp lệ theo cách khác cho old/new epoch;
- `cursor_expired`: sequence thấp hơn retained history;
- `permission_denied`: authenticated principal/device không còn access mà không
  làm lộ detail library.

Error bao gồm stable recovery action/route khi an toàn, không gồm decoded cursor
claim hay signing detail.

## Pull API

```http
GET /api/v1/libraries/{library_id}/changes?cursor=<opaque>&limit=<bounded>
```

Server:

1. authenticate/authorize current library access và device state;
2. decode/validate cursor, epoch, minimum retained sequence và feed profile;
3. trong short `REPEATABLE READ`, read-only transaction trên primary, kiểm tra
   retained boundary, capture committed head `H` và query event
   `sequence > cursor.sequence AND sequence <= H` theo thứ tự mà không split
   group; fixed MVCC snapshot ngăn concurrent retention xóa row giữa boundary
   validation và event query;
4. trả event và `next_cursor` do server sinh tại last returned sequence, hoặc tại
   `H` khi bounded query chứng minh không event nào bị bỏ sót;
5. có thể update device acknowledgement/checkpoint riêng làm operational hint.
   Checkpoint đó không bao giờ hồi tố làm event chưa trả trở nên an toàn.

Hình dạng response ví dụ:

```json
{
  "events": [],
  "next_cursor": "opaque",
  "has_more": false,
  "journal_epoch": "opaque-display-value-if-contract-requires",
  "server_time": "RFC3339"
}
```

JSON chính xác thuộc OpenAPI đã review. Response có thể lặp sau khi mất network
acknowledgement. Long polling/WebSocket notification về sau có thể nói “có thể
có change”, nhưng journal pull vẫn là sự thật; dropped notification không thể
làm mất dữ liệu.

### Quy tắc apply phía client

Với mỗi page, client dùng một local database transaction:

1. verify library/profile/event version và order;
2. deduplicate fact `(epoch, sequence)` đã áp dụng;
3. áp dụng resulting projection/tombstone theo sequence tăng dần, dùng node
   revision để tránh làm lùi bootstrap projection mới hơn;
4. ghi bounded content-download work riêng;
5. chỉ persist `next_cursor` sau khi mọi event trong page đã được apply bền vững;
6. commit, rồi request page kế tiếp.

Nếu local commit hoặc process shutdown fail, old cursor gây replay an toàn. Byte
file download vào temp, verify canonical hash/length và atomically replace local
materialization. Metadata convergence không chờ mọi cloud-only content hydrate.

## Bootstrap ban đầu và rebaseline cursor

Plain paginated live-tree listing có thể bỏ sót/trộn concurrent change trừ khi
được ghép với journal boundary. Giao thức ban đầu dùng bounded server-side
bootstrap session:

1. `POST /api/v1/libraries/{library_id}/sync-bootstrap` authenticate device, capture
   current epoch và head `H` trên primary, lưu feed/policy profile, tạo expiry và
   pin retention tại/trước `H` trong bounded bootstrap lifetime đó. Capture và
   pin commit atomically dưới cùng retention/head guard mà pruning kiểm tra, để
   pruning không thể vượt `H` giữa hai action.
2. Client page current authoritative node projection bằng opaque bootstrap page
   cursor chỉ được order theo immutable node ID, không bao giờ theo mutable path,
   name hay modification time. Server có thể gồm current revision mới hơn `H`;
   điều này an toàn vì event sau mang resulting revision.
3. Client ghi page vào staging local namespace. Parent có thể tới sau child;
   client validate/relink trước khi expose completed view.
4. Sau khi listing hoàn tất, server trả cursor chính xác cho `H`. Client
   atomically install staged view rồi drain mọi event sau `H`, bỏ qua resulting
   revision thấp hơn/bằng khi phù hợp.
5. Bootstrap retention pin còn tới acknowledged completion hoặc TTL. Nếu pin
   expire, client bỏ staging và restart; server không bao giờ cấp cursor có
   intervening event có thể đã prune.

Vì sao quá trình này hội tụ khi có write:

- node đổi sau `H` có thể xuất hiện ở state mới hơn trong listing, và event của
  nó lặp resulting revision đó;
- node bị xóa sau `H` có thể vắng hoặc có tùy page timing, nhưng retained
  tombstone sau `H` thiết lập final state;
- node được tạo và xóa trong lúc scan có cả hai retained fact dù không bao giờ
  xuất hiện trong listing;
- stable ID ordering ngăn rename/move làm skip pagination position.

Client không expose mixed bootstrap làm final synchronized state cho tới khi đã
drain change đến một observed head được chọn.

`cursor_expired` và epoch mismatch trả workflow này. Chúng không yêu cầu client
upload mọi local file như file mới. Trước rebaseline, client bảo toàn unsent
local mutation trong durable outbound queue; sau rebaseline chúng rebase/submit
bằng original base fact và conflict rule.

## Push mutation

File/folder API thông thường và/hoặc sync mutation endpoint đã review nhận:

- `client_mutation_id`, duy nhất trong device+library;
- operation kind và normalized request fingerprint;
- expected node metadata revision/ETag cho metadata operation;
- `base_version_id` cho content operation;
- target parent/name và expected parent revision khi cần;
- subtree precondition do server cấp cho recursive directory Trash/purge
  confirmation;
- client capability/protocol version;
- content upload session result cho operation có content.

Persistent mutation receipt có unique key
`(device_id, library_id, client_mutation_id)` và lưu request fingerprint, state,
result/error, node/version ID, event interval và commit time. Retry giống hệt trả
receipt đó. Tái sử dụng ID cho request khác trả `idempotency_conflict`.

Receipt cho committed content/namespace mutation nên được giữ dạng gọn ít nhất
suốt lifetime của library/device bị ảnh hưởng, không prune chỉ vì journal event
đã cũ. Nếu không, retry lost-response rất muộn có thể tạo version thứ hai sau
khi deduplication record biến mất.

### Precondition riêng theo operation

- **Content replace:** base current `FileVersion` là causal precondition.
  Rename/move có thể giao hoán nếu content head và state vẫn tương thích. Caller
  interactive có thể thêm strict node ETag để reject mọi concurrent metadata
  change.
- **Rename/move:** expected node metadata revision cùng constraint target
  parent/name. Content đổi đồng thời chỉ gây conflict nếu ETag contract bao phủ
  nó; API trả current state để rebase.
- **Trash/delete:** expected node revision/state. Recursive directory Trash còn
  mang opaque subtree precondition đổi khi retained descendant được tạo, edit,
  move hoặc delete. Stale delete không bao giờ âm thầm xóa edit mới hơn. Xác
  nhận tường minh “delete current subtree anyway” là mutation mới với
  precondition vừa cấp; content vẫn nằm trong Trash retention.
- **Restore:** expected Trash/node revision và explicit collision target/policy.
- **Create:** expected parent revision khi cần cộng active name uniqueness.

Điều này tránh coi physical object ETag, client timestamp hay wall clock là
causal version.

## Hành vi conflict xác định

Deterministic nghĩa là accepted transaction order và server rule có version tạo
một observable result; không có nghĩa device clock chọn winner. Client timestamp
và lexicographic device ID không bao giờ âm thầm phân xử dữ liệu.

### Content edit concurrent

Với server head `v4` và hai offline edit dựa trên `v4`:

```text
Device A: v4 -> bytes A
Device B: v4 -> bytes B
```

Commit thành công đầu tiên dưới node lock tạo `v5A` và giữ original node làm
head. Khi verified upload thứ hai revalidate base:

1. nó không replace `v5A`;
2. trong cùng transaction, nó tạo một sibling conflict `Node` với UUIDv7 mới và
   một current `FileVersion` tham chiếu byte B;
3. version đó ghi `conflict_base_version_id=v4`, conflict group/reason, source
   device, original node ID và safe server commit time;
4. nó gán server-generated portable conflict display name bằng naming algorithm
   có version và opaque suffix chống collision;
5. nó append `CONFLICT_CREATED` (và node projection cần thiết) vào cùng event
   group, lưu mutation receipt, audit và outbox atomically;
6. retry mutation của Device B trả cùng conflict node/version.

Visible sibling được ưu tiên hơn hidden alternate version vì client cũ có thể
bảo toàn và hiển thị nó dù không có conflict UI chuyên biệt. Original object và
conflict object vẫn immutable/addressable. Resolution là operation tường minh về
sau: giữ một, merge thủ công thành version mới hoặc Trash resolved copy. Synveil
không bao giờ tuyên bố có automatic binary merge.

Nếu verified byte giống hệt đến từ cả hai edit, policy chỉ có thể tránh conflict
người dùng nhìn thấy nếu resulting content hash/length bằng nhau chính xác và
metadata change giao hoán; mutation receipt vẫn ghi stale submission được resolve
thành `IDENTICAL_CONTENT`.

### Delete đối đầu offline edit

- Nếu Trash commit trước và offline content edit từ former version đến sau,
  original vẫn trashed. Synveil tạo một visible recovered-conflict node dùng
  verified incoming byte. Nó chọn former parent nếu active/authorized; nếu
  không thì nearest retained active ancestor, cuối cùng library root, và dùng
  recovered name chống collision. Event reason là `DELETE_VS_EDIT`.
- Nếu content edit commit trước và stale file delete đến sau, delete trả
  `version_conflict` cùng current revision/version. Với directory, changed
  subtree precondition tạo cùng conflict kể cả khi display metadata của
  directory không đổi. Client/user phải xác nhận tường minh deletion current
  state bằng mutation mới.
- Directory Trash đối đầu edit descendant tuân cùng rule. Nó không bao giờ ngầm
  mutate node trong trashed subtree về active.

Điều này bảo toàn byte mà không vô hiệu hóa deliberate deletion hay giấu dữ liệu
trong subtree không truy cập được.

### Metadata conflict

- Rename/move concurrent: một expected revision commit; stale request nhận
  `version_conflict` cộng current projection và có thể rebase bằng mutation ID
  mới. Không tạo duplicate empty file.
- Create concurrent với cùng comparison key: một bên thắng; bên kia nhận
  `name_conflict` và phải chọn/chấp nhận explicit non-destructive rename policy.
- Rename/move và content update có thể giao hoán theo stable node ID khi content
  base còn current và content caller không yêu cầu strict metadata ETag match.
- Move tới parent deleted/trashed/unauthorized fail cùng current safe state; nó
  không bao giờ âm thầm fallback về root.
- Cycle khi directory move bị ngăn bằng structural lock/ancestry rule.
- Restore vào former name bị chiếm trả conflict trừ khi caller chọn rõ target/name
  mới; không bao giờ overwrite occupant.

### Vòng đời conflict

Conflict node là ordinary visible node sau creation: chúng sync, version, share,
Trash, back up và tính vào quota. Conflict metadata của chúng được giữ để giải
thích/audit nhưng không cấp access tới origin node. Rename conflict không xóa
history của nó. Resolution UI tương lai có thể group chúng theo conflict group
nhưng không thể xóa byte nếu không có ordinary authorized mutation.

## Lan truyền rename, move, copy, Trash và restore

- Event rename/move bảo toàn node ID và content version. Client update local path
  bằng platform-safe atomic operation hoặc đánh dấu local name conflict; không
  bao giờ reupload byte chỉ vì path đổi.
- Copy tạo identity node/version mới và journal fact. Object byte có thể reuse
  trong dedup domain.
- `NODE_TRASHED recursive=true` invalidate root và mọi locally known descendant
  khỏi active view. Byte/version vẫn ở server trong Trash retention.
- `NODE_RESTORED` restore retained subtree tại resulting parent/name. Client đã
  bỏ local byte sẽ redownload theo version; không suy diễn resurrection từ
  stale filesystem residue.
- `NODE_PURGED` xóa item khỏi Recently Deleted. Client đã apply `NODE_TRASHED`
  không thực hiện live deletion thứ hai; object GC không bao giờ expose thành
  sync event.

Khi local platform không biểu diễn được server name (case collision, reserved
name, code point không hỗ trợ), client ghi state cục bộ tường minh
`UNREPRESENTABLE`/`NAME_CONFLICT` và bảo toàn server node identity. Client không
được rename server copy nếu không có user/policy consent.

## Selective sync và file on demand

Sync cursor ban đầu có phạm vi toàn library. Mọi authorized node metadata và
tombstone cần cho convergence đều được giao; device policy quyết định content
version nào cần hydrate. Điều này tránh move qua filter boundary làm mất âm thầm
ngữ nghĩa delete/create.

Các local materialization state được đề xuất:

```text
LOCAL
CLOUD_ONLY
PINNED
DOWNLOADING
UPLOADING
CONFLICT
UNAVAILABLE
EXCLUDED_CONTENT
```

Đây là projection cục bộ theo device, không phải alternate server object state.
Server cung cấp immutable version ID, length, hash, media metadata và range
download authorization. Client ghi download vào temp, verify, atomically install
và ghi hydrated version. Local filesystem watcher suppress echo bằng durable
node/version mapping, không phải timing sleep.

Future server-side filtered feed cần versioned filter identity nhúng trong
cursor, synthetic enter/leave event cho move, authorization analysis và
rebaseline rule. Chúng không thể reuse unfiltered cursor.

## Nghĩa vụ client theo platform

Mọi client dùng chung journal, mutation, version, conflict, backup và pairing
protocol. Platform adapter chỉ sở hữu behavior của host:

- Windows xử lý name NTFS/ReFS, reparse/junction, Windows Service hoặc
  supervisor đã khai báo, Credential Manager/DPAPI, locked file, sleep/reboot
  và watcher/rescan selected-folder.
- macOS xử lý name/case APFS, launchd, Keychain, sandbox/privacy permission,
  file coordination, sleep/wake và quyền selected-folder/photo.
- Linux Desktop xử lý filesystem/service/key-store profile đã khai báo, watcher
  khác nhau, desktop notification, warning tên portable và policy user-service
  so với system-service.
- Linux Server xử lý host/service và remote-operator; không giả định có desktop
  shell, watcher hay credential UI theo user session.
- Android, iPhone và iPad tương lai negotiate background execution,
  filesystem/photo-library access, local cache, notification và credential
  capability. Chúng có thể schedule best-effort nhưng không tuyên bố continuous
  background sync hay full-device backup nếu gate riêng chưa chứng minh.

Adapter không được biến local cache eviction, placeholder unavailable, watcher
miss, metadata unsupported hay service restart thành server deletion. Khi thiếu
capability, client báo state unsupported/degraded ổn định, giữ server identity và
dùng authoritative rescan/rebaseline hoặc action rõ ràng của user.

## Device checkpoint và journal retention

Device/library checkpoint ghi last acknowledged cursor sequence, last contact,
client/protocol version và health. Nó hỗ trợ UI và retention warning nhưng không
phải trusted proof local byte tồn tại.

Retention dùng cả age và storage bound và publish minimum retained sequence.
Bootstrap session tạm pin start boundary. Mặc định offline device không thể pin
journal mãi mãi; khi tụt sau minimum, nó thành `REBASE_REQUIRED` và dùng bootstrap
trong khi bảo toàn outbound mutation queue.

Trước pruning, retention job lock/check epoch và head, tôn trọng active bootstrap
pin, chỉ xóa bounded prefix và tăng `minimum_retained_sequence` atomically cùng
progress. Cursor tại boundary có một inclusive/exclusive rule đã test. Event
pruning không bao giờ xóa mutation receipt, history `FileVersion`, Trash record,
audit fact hay backup manifest như side effect.

## Hợp đồng lỗi

Sync API dùng code ổn định:

- `invalid_cursor`, `cursor_expired`, `cursor_epoch_mismatch`;
- `unsupported_protocol_version` và `unsupported_event_version`;
- `version_conflict`, `name_conflict`, `idempotency_conflict`;
- `resource_in_trash`, `parent_not_found`, `invalid_move`, `cycle_detected`;
- `upload_incomplete`, `checksum_mismatch`, `quota_exceeded`;
- `device_paused`, `device_revoked`, `permission_denied`;
- `storage_unavailable`, `storage_capability_unavailable`, `pairing_required`,
  `rate_limited` và `internal_error`.

Conflict response gồm current node revision/version mà caller được authorize,
safe retry guidance và verified incoming content có được bảo toàn thành conflict
node hay không. Chúng không bao giờ làm lộ collision hay object existence của
user khác.

## Ranh giới event, outbox và notification

Mutation transaction insert client `ChangeEvent`, security `AuditEvent` và
internal outbox/job bắt buộc cùng nhau. Chúng không phải cùng table hay contract:

- journal delivery được order theo library và giữ cho cursor replay;
- internal work là at-least-once, unordered trừ khi handler định nghĩa stable
  aggregate revision check;
- audit là accountability evidence với access/retention riêng;
- WebSocket/push notification là optional hint không chứa canonical mutation.
  Khi reconnect/drop, client pull journal.

Failure optional index/thumbnail/notification không thể rollback hay block sync
mutation. Derivative handler bind work tới immutable `FileVersion` và no-op nếu
đã complete hoặc stale.

## Ma trận failure và recovery

| Failure | Hành vi bắt buộc |
|---|---|
| Process chết trước transaction commit | Mutation, event, receipt và head increment đều rollback; cùng client mutation retry. |
| Process chết sau commit trước response | Receipt trả result/event interval giống hệt; không version/conflict thứ hai. |
| Transaction trước chờ trong khi transaction sau bắt đầu | Lock `sync_head` làm allocation/commit visibility có thứ tự; transaction sau không thể expose cursor vượt uncommitted event trước. |
| Event insert fail sau staged domain update | Toàn bộ PostgreSQL transaction rollback. |
| Outbox consumer offline | Mutation/journal vẫn commit; durable job lag quan sát được. |
| Change response mất | Old cursor request page lần nữa; client sequence dedupe làm replay an toàn. |
| Local DB client crash giữa page | Cursor chưa advance; toàn page replay. |
| Cursor bị prune/old epoch | Directed bootstrap; không tuyên bố empty/partial feed là current. |
| Bootstrap chạy khi mutation dày | Start boundary được pin; stable-ID listing cộng mọi post-boundary event hội tụ. |
| Device mất local database nhưng giữ file | Rebaseline xác định identity content; unmatched local modification submit với explicit base/conflict, không bao giờ bulk overwrite. |
| Hai content write race | Head được chấp nhận đầu thắng journal order; verified byte thứ hai thành một conflict copy. |
| Trash race offline edit | Trash được giữ; edit thành visible recovered conflict, hoặc stale delete bị reject tùy commit order. |
| Hai directory move có thể tạo cycle | Structural serialization làm một bên observe/reject result bên kia. |
| Object backend unavailable | Metadata pull tiếp tục nơi an toàn; content hydration báo retryable unavailable; không fake zero-byte file. |
| Device credential revoked | Pull/push mới fail ngay theo auth policy; byte đã download không thể bị thu hồi. |

## Scenario sync bắt buộc

### Convergence cơ bản

- create file/directory trên A -> chính xác node/version xuất hiện trên B;
- content update trên A -> B chỉ atomically replace sau hash verification;
- rename trên A -> B đổi path không upload/download khi content local;
- move file và nonempty directory trên A -> B bảo toàn node ID và subtree;
- Trash trên A -> recursive tombstone trên B; restore -> retained subtree xuất
  hiện lại;
- purge xóa trạng thái Recently Deleted nhưng không tạo live delete thứ hai;
- copy tạo node/version khác biệt và có thể reuse server object byte;
- restore old version tạo head mới và sync như immutable version mới.

### Ma trận offline và conflict

- A và B edit `v4` offline với byte khác nhau; reconnect A-rồi-B và B-rồi-A,
  chứng minh một head cộng một visible conflict và không mất byte;
- lặp losing mutation sau response loss, chứng minh không duplicate conflict;
- identical-content concurrent edit resolve không mất byte hoặc duplicate object
  và có documented outcome;
- rename/rename, move/move, rename/move, collision create/create name;
- rename trên A trong khi B edit content; content có thể commute theo stable
  ID/base;
- edit rồi delete và delete rồi edit cho file và descendant của trashed
  directory, gồm subtree-precondition invalidation;
- restore trong khi client khác create former name;
- move directory xuống descendant của nó và hai inverse move concurrent;
- offline client trở lại sau Trash retention/purge và submit queued edit;
- conflict case/Unicode/reserved-name qua fixture Linux, Windows và macOS.

### Correctness journal/cursor

- ép transaction T1 stage event rồi pause; chạy T2 và chứng minh không cursor nào
  advance qua T1 bằng commit inversion;
- ép T1 rollback sau head lock và chứng minh sequence được reuse an toàn/không
  visible gap;
- cấp multi-event group và chứng minh page không split chúng;
- duplicate page, retry overlap, empty page advance tới captured head, page limit
  boundary, backlog rất lớn và retention concurrent;
- cursor forged, wrong-library, beyond-head, malformed, expired, old-epoch và
  old-signing-key;
- test primary so với replica cố ý lag chứng minh cursor read không bao giờ dùng
  replica;
- hành vi event schema unknown-required dừng local cursor; optional field thì
  không;
- boundary journal pruning chính xác và active bootstrap pin chặn unsafe
  deletion.

### Bootstrap/rebaseline

- mutate/create/move/Trash/purge node giữa mọi bootstrap page; sau khi drain
  post-`H` event, client khớp authoritative server state;
- parent tới sau child, node revision trong snapshot vượt event replay sớm, node
  được tạo+xóa hoàn toàn trong scan và path change không làm nhiễu stable-ID
  pagination;
- bootstrap expire giữa scan, retention pressure xảy ra và restart không reuse
  unsafe page/cursor;
- stale device giữ outbound edit queue qua rebaseline và resolve với current
  base mà không coi mọi local file là mới;
- synthetic library một triệu node dùng bounded server/client memory và không
  có long-lived PostgreSQL transaction.

### Idempotency, crash và model testing

- mỗi mutation crash trước/sau domain write, head allocation, event, receipt,
  DB commit và response; outcome là zero hoặc one semantic mutation;
- hai API process và nhiều worker bảo toàn bất biến head/event/receipt;
- command ngẫu nhiên trên node, version, Trash, device và cursor so sánh server
  cùng nhiều client model sau disconnect/reorder/replay tùy ý;
- property: advance valid cursor không bao giờ bỏ committed event tại hoặc dưới
  sequence của nó;
- property: mọi accepted content byte stream vẫn reachable ở current,
  historical, Trash hoặc explicit conflict tới documented retention action;
- fuzz cursor decoder, event payload versioning, name, path depth, mutation batch
  và checked sequence arithmetic.

## Observability và release gate

Metric bao phủ event committed/read, head/minimum theo library, event-group size,
head-lock wait, latency/byte của change page, duplicate page/mutation rate,
device lag, stale/rebase-required device, bootstrap age/page, retention backlog,
conflict kind/count, unsupported protocol/event và content hydration error. Trace
correlate request, device, mutation, upload completion, journal group và outbox
mà không log cursor secret, token, content hay full path không cần thiết.

Không thể đánh dấu sync phase hoàn tất cho tới khi reference client state
machine, fault-injected PostgreSQL/object integration suite, multi-client model
test, mọi scenario trên, journal retention/rebaseline runbook và protocol version
compatibility fixture đều pass.

## Quyết định mở

OPEN DECISION OD-SYNC-001: format portable conflict display-name
Owner: Sync / Storage / Clients / Product
Needed by: Đóng băng fixture giao thức Phase 4
Options: original stem cộng device/date/opaque suffix; conflict directory chuyên dụng cộng original name; label chỉ ở UI trên opaque safe stored name
Recommendation: tạo sibling/recovered visible node với portable name có version dẫn xuất từ preserved display stem, safe device label, server UTC date và opaque suffix chống collision; Unicode folding chính xác tuân accepted namespace policy
Decision evidence: fixture round-trip đa platform, accessibility/usability review, path-length limit và deterministic collision test

OPEN DECISION OD-SYNC-002: service level retention journal
Owner: Sync / Operations / Product
Needed by: Gate vận hành production Phase 4
Options: chỉ fixed age; chỉ size cap; age target với protected minimum size và forced rebaseline ngoài giới hạn; device-ack pinning
Recommendation: dùng configurable age target cộng capacity guard, bounded bootstrap pin và explicit stale-device/rebaseline status; không cho abandoned device pin history mãi mãi
Decision evidence: event-volume benchmark, kỳ vọng offline household/device, disk-capacity test và đo duration rebaseline

OPEN DECISION OD-SYNC-003: representation ancestry directory
Owner: Database / Sync / Storage
Needed by: Gate schema move Phase 1
Options: adjacency list với serialized directory move; PostgreSQL `ltree` materialized path; closure table
Recommendation: bắt đầu với adjacency list cộng một per-library namespace-mutation guard cho short Node commit và indexed recursive query; chỉ đưa finer-grained path/closure locking vào sau measured contention, không bao giờ làm user-visible identity
Decision evidence: concurrent cycle test, benchmark move/list một triệu node, migration complexity và extension portability review

OPEN DECISION OD-SYNC-004: representation precondition recursive subtree
Owner: Sync / Database / Clients
Needed by: Schema/API freeze Phase 1, trước Trash recursive Phase 2
Options: subtree revision theo directory được update dọc ancestor; opaque snapshot token cộng journal descendant-conflict query; library-head precondition làm coarse guard bảo thủ
Recommendation: expose opaque subtree ETag ban đầu được hỗ trợ bởi per-directory subtree revision update dưới namespace guard; giữ nó tách khỏi display-metadata revision để client biết đang trình precondition nào
Decision evidence: model test delete-versus-descendant-edit, benchmark deep-tree write, race move/Trash và fixture offline client

# Giao thức đồng bộ change journal

Trạng thái: **Foundation Prompt 31–36 và desktop inbound core đã VALIDATED.
Server profile, enrollment groundwork, device authentication, secure credential
persistence và production HTTP SyncRemote Prompt 37 đã IMPLEMENTED. Composition
chu kỳ hai chiều bounded Prompt 91 và runtime scheduling chỉ trong process
Prompt 92 cùng durable-change-first runtime signal integration Prompt 93 đã
IMPLEMENTED; automatic conflict resolution, OS service integration và lifecycle
product rộng hơn vẫn là blueprint quy chuẩn PLANNED.**

Tài liệu này đặc tả mô hình đồng bộ đa thiết bị của Synveil. Tài liệu tuân theo
ADR-006 và từ vựng domain trong [DOMAIN_MODEL.md](DOMAIN_MODEL.md). Quy tắc
object chuẩn và Trash nằm trong [STORAGE.md](STORAGE.md); truyền content nằm
trong [UPLOADS.md](UPLOADS.md). Backup được cố ý tách riêng và đặc tả trong
[BACKUP.md](BACKUP.md).

Giao thức được thiết kế cho web, desktop dùng chung core Rust và client Apple,
với Android, iPhone và iPad là mobile profile tương lai. Tài liệu không tuyên bố
các client đó đã được implement. Windows, macOS, Linux Desktop và Linux Server
chỉ là first-class host target khi client/service evidence gate của chúng đạt.

Prompt 34 implement server boundary authenticated
`POST /api/v1/devices/{device_id}/libraries/{library_id}/mutations`: mỗi request
đúng một logical `CREATE_DIRECTORY`, `RENAME_NODE`, `MOVE_NODE`, `TRASH_NODE`
hoặc `RESTORE_NODE`, UUID idempotency bền, fingerprint canonical,
optimistic precondition explicit, conflict deterministic đã persist và đúng
một journal event khi success. Route không nhận file byte hay physical storage
identity, không tự động tạo conflict copy, merge hay last-writer-wins. Phần
Prompt 35 thêm durable conflict evidence, inspection bounded đã authenticate và
manual decision explicit/idempotent. Prompt 91 implement client cycle bounded
trung lập transport để compose inbound và outbound engine hiện có; full
lifecycle product, UI và content protocol rộng hơn vẫn là behavior tương lai.
Prompt 36 implement inbound apply; Prompt 37 kết nối core đó tới server thật
bằng device credential.

## Trạng thái đồng bộ Prompt 37

| Capability | Status |
|---|---|
| durable change journal | `VALIDATED` |
| device checkpoints/feed | `VALIDATED` |
| snapshot/rebaseline | `VALIDATED` |
| client mutation submission | `VALIDATED` |
| optimistic conflict detection | `VALIDATED` |
| durable conflict records | `IMPLEMENTED` |
| manual conflict inspection | `IMPLEMENTED` |
| explicit manual resolution | `IMPLEMENTED` |
| desktop inbound sync core | `VALIDATED` |
| desktop server profiles | `IMPLEMENTED` |
| device enrollment groundwork | `IMPLEMENTED` |
| device bearer authentication | `IMPLEMENTED` |
| secure desktop credential persistence | `IMPLEMENTED` |
| production HTTP SyncRemote | `IMPLEMENTED` |
| filesystem observation | `IMPLEMENTED` |
| self-generated change suppression | `IMPLEMENTED` |
| durable outbound intent capture | `IMPLEMENTED` |
| rename/move attribution | `IMPLEMENTED with conservative fallback` |
| watcher overflow/reconciliation | `IMPLEMENTED` |
| bounded automatic outbound mutation submission | `IMPLEMENTED (one-shot Prompt 91)` |
| process-local long-running sync runtime | `IMPLEMENTED (Prompt 92)` |
| desktop sync host/process lifecycle composition | `IMPLEMENTED (Prompt 94)` |
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop GUI/pairing UX | `NOT IMPLEMENTED` |

## Observation filesystem local và durable outbound intent đã implement

Prompt 38 thêm lớp observation local trong `crates/client-sync`. Đây chỉ là
control-plane boundary: event từ watcher native `notify` và watcher test xác định
chỉ phát bounded hint, còn quyết định logical luôn dựa trên SQLite bền vững,
managed-root validation, operation evidence Prompt 36 và reinspection filesystem
qua `LocalReplica`. Observer ghi `outbound_intents` typed (`CREATE_DIRECTORY`,
`CREATE_FILE`, `RENAME_NODE`, `MOVE_NODE`, `DELETE_OR_TRASH_NODE`,
`MODIFY_FILE_CONTENT`) với UUIDv7 local intent, base epoch/applied sequence,
revision/version của Node, path/fingerprint observed và semantic dedupe hash.
Không lưu file byte.

`.synveil/`, staging, quarantine, operation receipt và SQLite/control file local
không thuộc namespace user. Inbound apply Prompt 36 ghi `observation_suppressions`
bền vững bind operation ID, NodeId, expected path/presence/type và fingerprint
content khi cần; suppression không dựa trên timer. Sau khi filesystem result do
Synveil tạo được chứng minh, edit user ngay sau đó với fingerprint khác vẫn được
capture thành outbound intent thật. Observation không bao giờ advance
`applied_sequence` hoặc `acknowledged_sequence`.

Watcher overflow, event drop, backend error, rename unpaired ambiguous, hash
unstable, file bị khóa/không đọc được, tên không represent được, collision theo
portable-name policy, symlink/reparse point và special file Linux đều trở thành
observation issue bền vững hoặc `RESCAN_REQUIRED`; engine không đoán. Startup
luôn yêu cầu reconciliation và scan bounded để đóng downtime gap. Rename/move
intent cần paired native/evidence-based attribution; remove+create gần nhau không
đủ evidence thì fallback `AMBIGUOUS_RENAME`/rescan. Observer không có
`SyncRemote`/HTTP transport và không gọi `POST /mutations`, không tạo upload
session, không complete upload và không resolve conflict.

## Protocol durable conflict và manual resolution đã implement

Managed resource conflict Prompt 34 là evidence bền vững của client intent bị
từ chối. Trong cùng PostgreSQL transaction terminalize mutation gốc thành
CONFLICT, server tạo đúng một `SyncConflictId` UUIDv7 và một record scope theo
owner/Device nguồn/Library. Operation gốc giữ terminal vĩnh viễn. Replay cùng
ID/fingerprint trả cùng conflict ID và không tạo record thứ hai.
Authentication, CSRF, input malformed, mutation-ID reuse, infrastructure
failure và yêu cầu rebaseline không phải resource conflict nên không tạo row.

Record chỉ chứa field intent Prompt 34 đóng và historical logical observation
bất biến. Nó không có raw JSON, content byte, physical storage identity hay Node
foreign key. Lifecycle chính xác là OPEN, RESOLVED, DISMISSED. Vì vậy conflict
của resource đã purge vẫn inspect/dismiss được nhưng apply không thể recreate
Node.

Các route đã implement:

- `GET /api/v1/devices/{device_id}/libraries/{library_id}/conflicts` cho page
  OPEN-only mặc định 50, tối đa 100, order bất biến
  `(created_at DESC, conflict_id DESC)`, không OFFSET, dùng keyset cursor opaque
  có HMAC và bind scope;
- `GET /api/v1/devices/{device_id}/libraries/{library_id}/conflicts/{conflict_id}`
  cho original intent typed, server observation được gọi rõ là historical,
  lifecycle và terminal-resolution linkage; và
- `POST /api/v1/devices/{device_id}/libraries/{library_id}/conflicts/{conflict_id}/resolve`
  cho đúng một decision strict `ACCEPT_SERVER` hoặc `APPLY_CLIENT_INTENT`.

Mọi route authorize lại owner đã authenticate, Device ACTIVE, Library thuộc
owner và exact conflict scope. Sở hữu conflict ID không cấp quyền. GET là read
không cần CSRF; POST bắt buộc session-bound CSRF proof. Mọi result/error là
private/no-store và body resolution tối đa 16 KiB.

Mỗi decision có `resolution_id` UUIDv7 và fingerprint SHA-256 typed canonical
trên conflict ID, action cùng presence/value của fresh precondition explicit.
Raw JSON serialization không liên quan. Cùng ID/fingerprint replay chính xác
terminal outcome, completion time và journal linkage sau mất response; semantic
khác trả `resolution_id_conflict`.

`ACCEPT_SERVER` chọn current canonical server state một cách explicit. Nó ghi
decision nguyên tử và chuyển OPEN thành DISMISSED nhưng không đổi Node, không
phát resource journal event và không advance checkpoint.

`APPLY_CLIENT_INTENT` không bao giờ reuse revision stale gốc. Resolver phải gửi
current resource revision cho mọi kind được hỗ trợ, cộng current
parent/destination revision cho move và restore. Service lấy library namespace
guard và scoped row lock hiện có, đọc lại current state, reconstruct intent
semantic đã giữ cùng fresh precondition rồi gọi chung executor canonical Prompt
34 trong transaction. Success commit nguyên tử đúng một Node mutation, một
resource event thông thường, resolution result/linkage và conflict RESOLVED.
Nó không reopen mutation gốc hay advance checkpoint. Device khác observe event
qua feed thông thường.

Nếu server rename, move, purge hay canonical mutation khác thắng trước, apply
ghi durable stale resolution result có thể replay, trả `resolution_conflict`,
giữ conflict OPEN và commit zero Node change/journal event. Nó không refresh,
retry, merge, rename, copy hay chọn action khác. Evidence OPEN sống qua
rebaseline; sau khi Device complete rebaseline, con người có thể inspect current
state rồi gửi fresh precondition mới. Race APPLY/APPLY hay ACCEPT/APPLY tạo tối
đa một terminal conflict decision và một resource event. Không có policy
automatic conflict resolution.

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
6. Mọi logical mutation được hỗ trợ mang `client_mutation_id` ổn định,
   fingerprint canonical, base epoch/sequence và precondition typed phù hợp
   operation. Mutation lặp lại trả persisted outcome và không append event khác.
7. Prompt 34 không nhận content byte. Metadata conflict trả current
   authoritative logical state để rebase tường minh; không có silent
   last-writer-wins hay automatic conflict copy. Content protocol tương lai
   phải bảo toàn cả hai verified byte stream.
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

### Nền tảng snapshot nội bộ Prompt 81

Prompt 81 thêm seam `LogicalSnapshotService` transport-neutral để build đầy đủ
logical state của một Library mà không thêm HTTP route hay đổi protocol bootstrap
theo device đã có. `RebaselineSnapshot` trả về:

- library ID đã được owner authorize và một `LogicalSnapshot` canonical;
- root hiện tại cùng mọi Node logical hiện tại ở state `ACTIVE` và `TRASHED`,
  gồm parent ID, kind, name, revision và metadata content an toàn của current file;
- boundary `JournalHighWatermark`/`JournalCursor` lấy từ cùng database view.

Builder bắt đầu transaction PostgreSQL `REPEATABLE READ`, lấy namespace guard
theo Library hiện có trước data read đầu tiên, đọc journal head và projection Node
theo set query, order entry bằng immutable Node ID rồi commit state cùng boundary.
Mutation cooperative đồng thời vì vậy hoặc nằm trong snapshot và ở trước/tại
boundary, hoặc vắng trong cả hai và còn trong feed strictly sau boundary. Wall
clock chỉ có tính mô tả, không phải cursor.

Snapshot là logical: Node `PURGING`/đã purge, physical object key,
replica/storage locator, filesystem path, staging handle và byte file không thuộc
contract. Build/read snapshot không tạo hay advance device checkpoint, không
complete rebaseline, không apply state ở client và không chọn conflict resolution.

### Artifact fixed-cut durable và paging Prompt 82

Prompt 82 persist cut Prompt 81 thành artifact `RebaselineSnapshotId` theo scope
Library và owner đã authorize. Creation vẫn ở một transaction `REPEATABLE READ`
dưới namespace guard cũ: đọc/validate aggregate Prompt 81 đầy đủ, ghi header
bất biến có boundary `JournalHighWatermark`, count và expiry inject, materialize
entry bằng một `INSERT ... SELECT`, rồi commit. Nếu bất kỳ bước nào fail,
rollback không để lại header, boundary hay entry row có thể đọc.

Sau commit, `get_rebaseline_snapshot` và
`read_rebaseline_snapshot_page` chỉ dùng header/entry table durable. Chúng không
giữ transaction create, không lấy namespace guard và không query `nodes` live;
mutation bình thường vì thế vẫn tiếp tục trong lúc transfer lớn được page qua
connection khác hoặc process restart. Entry order theo immutable `NodeId` bằng
keyset `(snapshot_id, node_id)`: `node_id > after_node_id`, ascending, limit
bounded. OFFSET không phải continuation mechanism của snapshot.

Mọi `RebaselineSnapshotPage` lặp descriptor (artifact ID, Library, journal
boundary, count, timestamp) và chỉ trả `RebaselineSnapshotPageCursor` scope theo
snapshot sau Node cuối của page. Cursor này cố ý không phải `JournalCursor`;
journal cursor tiếp tục incremental sync strictly sau boundary capture. Page size
default là 256, maximum là 1000. Memory page là O(page size); creation có thể
tạm allocate aggregate O(total-node-count) đã validate trước copy set-oriented.

Artifact hết hạn logic khi `observed_at >= expires_at` (default Gen-1 24 giờ)
và sau đó fail `Expired`. Row hết hạn có thể còn vật lý tới lifecycle/retention
phase sau; prompt này không thêm cleanup worker, delete API explicit, retry loop,
global lock, HTTP/OpenAPI/SSE/WebSocket route hay client snapshot apply. Owner
khác đoán Snapshot ID nhận `NotFound`, và creation/read vẫn checkpoint-neutral.

### Apply nguyên tử ở client Prompt 84

Client-sync nhận descriptor đã được tạo qua page source transport-neutral. Nó
lưu tối đa một candidate mỗi Library trong bảng SQLite tách biệt, kiểm tra
snapshot ID/Library/boundary/count bất biến, thứ tự NodeId strict, duplicate,
cursor opaque cycle, count terminal chính xác và invariant root/parent/
reachability trước activation. Transfer bounded dùng O(page size) memory; tái
tạo metadata/path cuối dùng O(nodes).

Một transaction SQLite ngắn chỉ thay remote mirror authoritative
(`local_nodes`) và ghi `AppliedPendingHandoff`. Nó không xóa/ghi lại outbound
intent, base revision, upload staging hay byte content local. Inbound incremental
cũ bị fence tới phase handoff sau; phase này không gọi ACK/checkpoint server,
không resolve conflict và không thêm UI, retry loop hay cleanup daemon.

### Ranh giới abuse khi tạo artifact Prompt 83C

Durable snapshot service không có authenticated rate limiter dùng lại được hoặc
durable quota hiện hữu phù hợp cho artifact này. Vì vậy Gen-1 áp dụng bound
durable hẹp `MAX_ACTIVE_REBASELINE_SNAPSHOTS_PER_OWNER_LIBRARY = 8`: tối đa tám
artifact còn active, chưa hết hạn cho mỗi cặp owner và Library. Query admission
chạy sau khi lấy per-Library namespace guard hiện có và trước bước materialize
O(tổng số Node), trong cùng transaction được guard bảo vệ ở `READ COMMITTED`.
Creation cố ý dùng `READ COMMITTED`: PostgreSQL có thể tạo snapshot
`REPEATABLE READ` ngay khi statement advisory-lock đang chờ, khiến creator xếp
hàng đếm bộ artifact đã commit bị stale. Sau khi lấy guard, mọi mutation
namespace cooperative và creator đều serialized, nên head, admission count và
projection materialize vẫn là một cut được guard bảo vệ. Đây là policy ở
metadata transport-neutral, không phải SQL đặt trong handler, semaphore
in-process hay global lock.

Artifact active chính xác khi `observed_at < expires_at`; bằng nhau nghĩa là
expired và có thể admit artifact mới. Row expired không bị physical delete ở
phase này và không tính vào bound. Khi chạm bound, typed admission failure được
HTTP map thành response canonical `429 Too Many Requests` / `rate_limited` với
`Retry-After: 1`. Create bị reject không commit header, entry, checkpoint,
acknowledgment hay journal change nào. Không có idempotency/retry system: nếu
response thành công bị mất, POST lặp có thể để lại artifact duplicate, nhưng
durable active bound vẫn được giữ riêng theo owner/Library.

Bằng chứng PostgreSQL/HTTP trực tiếp cover path concurrent dưới bound, race biên
`N-1`, isolation owner/Library, re-admission đúng expiry, atomicity của create
bị reject, owner hợp lệ hết hạn trả HTTP `410`, owner khác với artifact expired
trả `404`, và denial `device_revoked` hiện có trên descriptor, page và create.

### Handoff checkpoint durable của rebaseline Prompt 85

Prompt 85 hoàn tất trạng thái local `AppliedPendingHandoff` mà Prompt 84 để
lại. Server chỉ thêm một operation
`DeviceSyncService::complete_rebaseline_handoff`, với HTTP surface:

```text
POST /api/v1/rebaseline-snapshots/{snapshot_id}/handoff
```

Route này chỉ nhận authenticated device bearer. Body rỗng (client chuẩn có thể
gửi `{}`); owner, Library, epoch và sequence không bao giờ là field trong
request. Bearer cung cấp owner/device đã authenticate; server đọc handoff proof
bất biến theo owner để derive Library và
`C = (journal_epoch, snapshot_resume_sequence)`. Payload đã expired vẫn hợp lệ
cho handoff khi proof còn; proof thiếu hoặc thuộc owner khác trả
`404 not_found`. Transaction handoff không đọc entry, không đổi snapshot,
journal retention, và không dùng ordinary feed ACK.

Server lock device checkpoint canonical và chỉ áp dụng transition monotone:
checkpoint thiếu hoặc cũ hơn được advance tới `C`, checkpoint đúng `C` là
idempotent, checkpoint cùng epoch đã vượt `C` trả `409 checkpoint_conflict`,
epoch mới hơn hoặc incompatible cũng trả conflict. Library epoch và boundary
được validate với Library hiện tại; persisted proof sai bị fail closed. Prompt
85 tự nó không thêm migration; Prompt 86 sau đó chuyển authority từ payload
header lớn sang proof nhỏ ở migration 36. Idempotency vẫn derive từ proof và
equality của checkpoint.

Client transport chỉ expose
`complete_rebaseline_handoff(snapshot_id) -> confirmation`, không có arbitrary
checkpoint assignment hay boundary argument. HTTP adapter decode strict
snapshot ID, Library ID và response decimal `{epoch, sequence}`. Engine chỉ
accept response khi mọi giá trị khớp chính xác marker pending local. Mismatch,
authentication/revocation, conflict, not-found hoặc transport/protocol failure
đều giữ marker. Sau khi server confirm khớp, một SQLite transaction ngắn verify
marker, set các field cursor hiện có của `replicas` là `journal_epoch`,
`applied_sequence`, `acknowledged_sequence` về `C`, set lifecycle
incremental-ready/idle và xóa marker. Transaction không gọi network; outbound
intent, base observation, staged upload và local content không bị đụng tới.
Inbound chỉ được un-fence sau khi transaction này commit; server success một
mình không đủ để mở lại old cursor.

Crash proof cover bảy boundary: trước request, server commit sau đó response
mất, đã nhận response, server success trước local finalize, SQLite rollback
trước commit, local commit trước process exit và restart/retry. Retry luôn an
toàn: response mất chỉ lặp operation derive từ header và idempotent; local
finalize đã commit trả `AlreadyComplete`, không tạo lại candidate hay snapshot.
Network không giữ SQLite transaction hay local writer lock, nên outbound intent
insert trong lúc handoff vẫn sống byte-for-byte và chỉ submit sau boundary.
Cùng snapshot concurrent hội tụ; device khác giữ checkpoint riêng; Library và
owner khác bị cô lập. Prompt 85 không thêm retention/compaction, physical
cleanup, conflict policy, retry daemon, polling/background scheduler, UI hay
deployment.

### Retention journal và cleanup vật lý bounded Prompt 86

Prompt 86 implement foundation retention journal/payload vật lý đầu tiên bằng
các call one-shot transport-neutral của `SyncRetentionService`. Giá trị
`libraries.minimum_retained_sequence` hiện có mang nghĩa chính xác là sequence
cao nhất đã compact qua trong epoch hiện tại. `cursor < floor` trả
rebaseline-required/history-unavailable hiện có; `cursor == floor` tiếp tục an
toàn bằng các event strictly lớn hơn floor. Vì vậy no-cursor stale sau khi floor
khác zero, kể cả journal vật lý rỗng. Compaction không đổi epoch, head hay bất kỳ
checkpoint device nào.

Horizon lịch sử incremental tối thiểu Gen-1 là 30 ngày. Một call journal chỉ
load/xóa tối đa 10.000 row cũ nhất (hard maximum 100.000) trong một prefix đủ
tuổi liên tục. Sequence là authority: nếu row sớm chưa đủ tuổi, mọi row sau vẫn
được giữ dù timestamp sau đó cũ hơn. Xóa row và tăng floor commit cùng
transaction. Lock order là namespace advisory guard, row clock/floor Library,
quan sát proof rồi journal row. Feed giữ share lock Library trong transaction
metadata ngắn, nên race cleanup chỉ trả feed cũ đầy đủ hoặc rebaseline theo floor
mới; append mới không thể lọt vào target xóa đã tính.

Migration 36 thêm row `rebaseline_snapshot_handoff_proofs` bất biến và backfill
mọi snapshot durable hiện có. Tạo snapshot mới ghi header, entry và proof nguyên
tử. Proof giữ snapshot ID, owner, Library, epoch/boundary, timestamp snapshot và
deadline 30 ngày sau expiry payload; không có Object/replica/storage/path/
credential và không FK cascade tới payload header.

Payload chỉ đọc được khi `observed_at < expires_at`. Tại equality, cleanup có
thể xóa tối đa 32 artifact mỗi call nhưng chỉ khi proof tồn tại; thiếu proof làm
cleanup fail closed. Header/entry biến mất nguyên tử. Owner đọc descriptor/page
vẫn nhận `SnapshotExpired` khi proof còn, owner khác vẫn nhận `NotFound`, và
handoff vẫn install đúng boundary sau payload cleanup. Header/entry page read
dùng một repeatable-read snapshot ngắn nên race không thể trả page thành công bị
rách.

Mọi proof current-epoch còn giữ pin journal tại boundary của nó:
`compacted_through <= C`; boundary tương thích nhỏ nhất thắng. Checkpoint device
không pin và vì thế không giữ history vô hạn. Tại/sau `proof_expires_at`, mỗi
call có thể xóa tối đa 128 proof (hard maximum 4.096) khi payload đã vắng mặt.
Sau đó handoff là `NotFound`, journal step sau có thể tiến lên, và Prompt 87 xử
lý convergence client. Prompt 86 không thêm endpoint, daemon, timer, scheduler,
retry/backoff, UI, conflict policy hay recovery client tự động.

### Invalidation retained-cursor và automatic rebaseline convergence Prompt 87

`RebaselineConvergenceCoordinator::run_convergence_once` là state machine client
bounded, transport-neutral có thể gọi trực tiếp. Nó không có daemon, timer,
poller, gọi đệ quy, retry/backoff, cleanup action hay conflict policy. Mỗi
invocation inspect durable SQLite state theo precedence cố định:

1. candidate snapshot thật đã persist tiếp tục đúng chuỗi page hữu hạn của nó;
2. `AppliedPendingHandoff` thử handoff Prompt 85 hiện có, derive từ header; rồi
3. incremental sync bình thường chạy nhiều nhất một operation bounded.

Chỉ kết quả server-authoritative `RebaselineRequired` (retained history hoặc
no-cursor sau floor khác zero, hay epoch incompatible) mới bắt đầu snapshot
recovery. Với pending handoff hiện có, chỉ `NotFound` (proof bị thiếu/conceal
cho snapshot local đã authenticate) và checkpoint conflict typed mới cho phép
snapshot thay thế. Authentication/revocation, permission denial, 429,
server/internal, TLS/DNS/timeout/offline, protocol malformed, SQLite local
failure và candidate corrupt không phải trigger rebaseline; chúng giữ lỗi typed
và durable state hiện có.

Trước POST create không idempotent duy nhất được phép, coordinator ghi một claim
inert, scope theo Library vào candidate row v5. Descriptor server thành công
promote claim đó nguyên tử thành candidate fetching bình thường. Response create
mất hoặc malformed sẽ release claim inert trước khi trả lỗi typed, nên cùng
invocation không POST lại còn invocation sau có thể thử đúng một lần. Caller
cùng Library chỉ serialize bằng guard local scope Library cộng durable claim;
Library khác độc lập. Không SQLite write transaction nào được giữ qua HTTP.

Khi recovery vì proof thiếu hoặc checkpoint conflict, H1 vẫn tồn tại nên inbound
vẫn fence trong lúc page S2 stage. Activation candidate là một transaction
SQLite: swap remote base authoritative và upsert H1 thành H2, rồi xóa candidate.
Reader chỉ thấy `base(S1)+H1` hoặc `base(S2)+H2`, không bao giờ thấy pair lẫn
hoặc không marker. Coordinator sau đó delegate handoff/finalization Prompt 85
bình thường cho S2. Nó không bao giờ gửi old local boundary lên server. Nếu S2
lại gặp missing-proof/checkpoint conflict, kết quả là
`RecoveryBlocked(DidNotConverge)` và không được tạo S3.

Outbound intent, ID, payload, precondition/base revision, source/upload
reference và submission state nằm ngoài cả hai activation path nên không đổi.
Phase này cố ý không quyết định intent đã giữ conflict thế nào với remote base
mới; Prompt 88 sở hữu conflict policy.

Restart proof là durable: descriptor/candidate đã persist được resume trước
khi create; candidate complete được activate; marker đã activate retry handoff
Prompt 85 idempotent; sau local finalization incremental resume strictly sau
cùng boundary server-proved. Trong recovery proof-loss, S2 stage dở có thể cùng
tồn tại với H1 sau restart; sau replacement activation invocation tiếp theo thấy
H2. Proof snapshot mới tiếp tục pin retention theo Prompt 86 trong suốt
download, activation và handoff.

## Push mutation

Subset push đã implement của Prompt 34 là route logical strict mô tả ở trên.
Profile content/namespace phong phú hơn trong phần còn lại là extension tương
lai; không phải lý do để thêm arbitrary JSON patch, file byte, object identity
hay automatic conflict resolution vào route hiện tại.

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

Retention publish sequence đã compact qua. Offline device không pin journal mãi
mãi; khi checkpoint tụt dưới floor, nó nhận rebaseline-required canonical trong
khi outbound mutation queue được bảo toàn. Handoff proof snapshot durable còn
giữ mới pin tạm thời vì client có thể đã apply snapshot nguyên tử.

Mỗi call retention one-shot explicit lock/check epoch và head, cap target tại
handoff proof tương thích nhỏ nhất, chỉ xóa bounded prefix và tăng
`minimum_retained_sequence` atomically cùng progress. Cursor tại boundary hợp
lệ; cursor thấp hơn là stale. Event
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

LOCKED DECISION OD-SYNC-002: service level retention journal Gen-1
Owner: Sync / Operations / Product
Decision: horizon tuổi tối thiểu 30 ngày, chỉ xóa prefix liên tục bounded, không pin bằng device checkpoint, và tạm cap tại handoff proof snapshot durable tương thích nhỏ nhất. Xem ADR-030. Policy capacity tương lai có thể supersede minimum này nhưng không được âm thầm làm yếu invariant no-gap.
Decision evidence: suite Prompt 86 large-journal, stale cursor, snapshot proof, rollback, restart và concurrency trên PostgreSQL 17

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

## Boundary apply inbound desktop đã implement (Prompt 36)

`synveil-client-sync` là sync core phía desktop có thể tái sử dụng đầu tiên.
Crate chỉ nhận inbound và không phụ thuộc Axum handler. Transport adapter
implement `SyncRemote` cho checkpoint read, feed page có giới hạn,
acknowledgement có ký, start/page/completion rebaseline và download current
content theo định danh logical. Test deterministic giữ cùng semantics
checkpoint/completion; Prompt 37 thêm HTTP adapter production dùng device auth
được mô tả bên dưới.

Mỗi lần gọi engine chỉ advance một page hoặc một local bootstrap batch có giới
hạn. Thứ tự feed bền vững là:

1. validate rồi persist page cùng event;
2. prepare local operation có type trước filesystem action;
3. stage/thực hiện và ghi receipt bền cho kết quả filesystem;
4. persist nguyên tử Node mapping, event evidence và `applied_sequence`;
5. persist opaque evidence `ACK_PENDING`;
6. acknowledge server;
7. persist `acknowledged_sequence` đã confirm rồi dọn page.

SQLite ép `acknowledged_sequence <= applied_sequence`. Crash sau local commit
retry đúng acknowledgement evidence đã che thay vì refetch hay apply mù page.
Mất response sau khi server ack vẫn an toàn vì acknowledgement idempotent. Sai
epoch/scope/schema/event kind, sequence lùi hoặc gap đều fail closed.

Bootstrap page được persist thành desired manifest, không chỉ giữ trong memory.
Terminal manifest chỉ được chấp nhận sau khi chứng minh đúng item count đã khai,
đúng một root, parent là directory đầy đủ và mọi Node reachable. Node được apply
parent-first. Sweep chỉ quarantine Node sạch đã track nhưng vắng trong generation
hoàn tất; local object không rõ không bao giờ bị sweep. Server completion được
retry từ completion evidence bền và handoff local đặt cả hai sequence đúng bằng
snapshot cut server trả về.

`NodeId` là identity local. Relative path, parent identity, revision, current
version, expected content length/hash, bootstrap generation, presence và
quarantine location là projection bền. Rename/move directory cập nhật path hậu
duệ denormalized bằng một statement SQLite set-based; offset Unicode do SQLite
đếm ký tự, không dùng số byte của Rust.

Logical segment portable được materialize nguyên vẹn. Policy chung Windows/
Linux bảo thủ chặn separator, control, ký tự Windows illegal, space/dot cuối,
reserved device, segment quá dài và control name `.synveil`. NFKC cộng lowercase
chỉ dùng làm collision key, không rewrite visible name. Collision case/
normalization giữa managed object hoặc object local không rõ trở thành
`LOCAL_NAME_COLLISION`.

Trước replace, rename, move, Trash, restore, purge hay bootstrap sweep, engine
verify fingerprint local bền gần nhất. Destination không rõ bị chiếm và object
bị sửa, mất, đổi type hoặc tree diverged đều thành local blocker bền. Không có
conflict copy, upload, merge hay last-write-wins. Server Trash chuyển object
sạch có thể quy thuộc vào quarantine có kiểm soát; metadata server hiện chỉ cho
Trash directory rỗng. Restore chỉ tin quarantine sau verify, nếu không sẽ dựng
lại trạng thái file/directory canonical. Purge chỉ bỏ local state sạch có thể
quy thuộc và vẫn giữ byte trong quarantine có kiểm soát ở phase này; chưa có
aggressive quarantine cleanup.

Current file content được stream tuần tự qua adapter chunk có giới hạn, ghi vào
staging file có operation ID, hash trong lúc stream, kiểm tra declared length và
SHA-256, flush/sync rồi mới expose. File visible cũ còn nguyên tới khi byte mới
được verify. Unix dùng atomic rename cùng filesystem và sync parent directory.
Code path Windows đóng staged handle rồi dùng backup/rename bảo thủ theo
operation vì semantics replace Windows khác; phase này đã audit compile nhưng
bằng chứng runtime Windows native được hoãn tới checkpoint platform Prompt 40.

Failure point deterministic bao phủ persist page, trước filesystem action, sau
stage/trước expose, sau filesystem receipt bền, sau local commit, sau server ack,
persist bootstrap page, local bootstrap complete và mất response server
bootstrap completion. Receipt bền dưới `.synveil/staging` phân biệt kết quả của
Synveil với object không rõ xuất hiện trong race. Không thể quy thuộc chính xác
trở thành `LOCAL_RECOVERY_AMBIGUOUS`; receipt chỉ bị xóa sau khi database
operation tương ứng đã commit.

Filesystem watcher, outbound mutation generation, automatic conflict
resolution, desktop UI/pairing UX, service installer,
WebSocket/SSE, backup và sharing không thuộc boundary này. Đồng bộ ACL, xattr,
permission và ép local mtime theo server timestamp được hoãn rõ ràng.

## Remote connection và credential lifecycle đã implement (Prompt 37)

Profile là cấu hình HTTPS origin-root bất biến với local UUIDv7
`ServerProfileId`. Adapter thêm route segment đóng `/api/v1/...` bằng URL
operation, không nhận path tùy ý từ caller. Userinfo, query, fragment, subpath,
port sai và parser repair đều fail closed. Numeric loopback HTTP chỉ được nhận
qua construction policy test explicit. Certificate verification bắt buộc; mọi
redirect bị chặn; không cookie jar hay ambient proxy; transparent decompression
không được thay đổi logical length/hash. Server chưa expose stable installation
identity; binding là origin/TLS đã verify, không suy luận từ hostname/LibraryId.

Browser owner đã authenticate tạo grant một lần, có giới hạn và TTL ngắn, qua
CSRF hiện có. Desktop exchange đúng một lần, nhận identity owner/Device/
credential cùng bearer secret đã che. Server chỉ lưu digest domain-separated.
Exchange cố ý không auto-retry: nếu đã commit nhưng mất response, owner phải
revoke credential chưa nhận/đã cấp rồi tạo grant mới. Process desktop mất trước
khi secure persistence hoàn tất cũng cần recovery explicit; SQLite không lưu
enrollment token hay bearer.

Lifecycle desktop:

1. persist cấu hình `ServerProfile` không bí mật;
2. exchange grant một lần qua HTTPS đã verify;
3. đưa opaque exchange receipt bind profile/origin vào `store_enrollment`
   (hoặc `replace_enrollment` explicit), ghi cleanup intent không bí mật,
   store/read-back bearer qua
   `PlatformRuntime::SecretStore`, rồi commit enrollment identity metadata;
4. sau restart load `LoadedDeviceCredential` bind profile, construct
   `HttpSyncRemote` từ profile, Device, credential và config có giới hạn;
5. initialize/open managed root V2 cùng profile rồi chạy engine inbound
   bootstrap/feed/apply/ack/download hiện có.

Không có public raw bearer-import API để đổi nhãn exchange Server A thành
enrollment Server B. SecretStore key dùng profile ID cộng credential ID, không
URL alias. Envelope có version/giới hạn còn bind canonical origin, transport
policy, profile, owner, Device và credential ID bên trong secure storage.
SQLite bị copy/reconstruct cùng ID nhưng origin khác không thể load, overwrite
hay xóa secure entry đó. HTTP constructor so sánh secure origin đã load với
profile được yêu cầu trước khi tạo bearer header. Envelope malformed, version
lạ, quá lớn hay entry raw secret cũ bị chặn, không fallback. Linux dùng
persistent native Secret Service; Windows dùng Credential Manager; native
builder được chọn không fallback mock backend. Secure store thiếu/bị khóa/lỗi
fail closed. In-memory store synthetic chỉ có trong test. Native Linux
persistence đã được chạy với test vault cô lập; cross-target compile không
tuyên bố native Windows credential-store/TLS đã chạy. Persistence macOS vẫn
Unsupported.

Migration SQLite V2 giữ nguyên schema/checksum V1. Nó chỉ lưu profile, ID
owner/Device/credential, timestamp enrollment/forget và cleanup intent của
secure entry. Replica row lẫn physical root marker V2 bind đúng một profile.
Profile khác, dù cùng LibraryId, không thể mở replica đó. Legacy unbound
replica không được biến thành production replica ngầm; chưa có auto-rebind.

Local forget ghi disconnected marker bền trước, rồi xóa secure entry; vẫn giữ
owner/Device binding cho re-enrollment explicit. Lỗi để lại cleanup metadata
retry được và không reload secret cũ sau restart. Replacement chỉ nhận cùng
owner/Device, stage/read-verify secret mới, commit credential ID mới rồi dọn key
cũ qua cleanup bền. Engine kiểm tra credential generation active trước mỗi lần
gọi nên engine cũ dừng sau forget/replacement. Caller phải bỏ direct HTTP object
đã giữ secret cũ. Forget không đồng nghĩa server revoke khi offline.

Device bearer chỉ cấp quyền checkpoint/feed/ack, rebaseline start/page/complete
và logical read/download cần cho inbound apply. Từng route ép authenticated
Device ID cùng Library thuộc owner. Browser-cookie write vẫn cần CSRF; chỉ
bearer authenticate thành công được miễn trên inbound route. Device bearer
không được gửi mutation Prompt 34, inspect/resolve conflict Prompt 35 hay dùng
browser administration. Device/credential revocation được kiểm tra lại ở
request tiếp theo.

HTTP error giữ phân loại authentication, revocation, checkpoint/rebaseline/
evidence, rate-limit, dependency/internal, protocol, offline, timeout, TLS,
body-limit và redirect. Health dùng readiness endpoint hiện có cộng checkpoint
đã authenticate; chặn HTML/payload không tương thích. Metadata body, download
byte, stream chunk đều có giới hạn, timeout hữu hạn. Adapter không auto-retry;
engine có thể retry cùng ack/completion proof bền sau gián đoạn. Auth,
revocation hay offline error giữ file local, sequence applied/acknowledged và
pending evidence; không wipe hay rebind replica.

## Xung đột outbound tất định (Prompt 88)

Xung đột nghĩa là precondition chuẩn trên server đã từ chối một intent cục bộ
bền vững, hoặc snapshot rebaseline authoritative vừa activate chứng minh chính
xác precondition đó chắc chắn thất bại. Remote base vẫn là chuẩn; intent gốc,
base revision, metadata payload và nguồn nội dung staged (nếu có) không đổi.
Timeout, lỗi authentication/authorization, rate limit, dependency failure và
response sai định dạng không phải xung đột.

Schema SQLite V6 lưu một bản ghi xung đột chỉ ở client cho mỗi outbound intent,
gồm kind, bằng chứng remote an toàn ban đầu, base cục bộ gốc, status, trường audit
phân giải và liên kết intent thay thế tùy chọn. Bản ghi không chứa byte nội dung,
credential, cookie, object key server hoặc đường dẫn vật lý. Bằng chứng ban đầu
bất biến dù inbound sau đó làm remote base tiến lên. Danh sách được scope theo
library và phân trang keyset theo `(detected_at, conflict_id)`, mặc định 100 và
tối đa 1.000.

Queue outbound hiện chưa có causal-dependency graph bền vững. Vì vậy một xung
đột chưa giải quyết tạm dừng bảo thủ toàn bộ outbound của library, kể cả intent
độc lập xuất hiện sau; library khác vẫn độc lập. Inbound và ACK tiếp tục. Với
xung đột nội dung, inbound chỉ đưa byte remote chuẩn mới ra replica sau khi xác
minh byte cục bộ xung đột đã nằm trong vùng staging bền vững với length và
SHA-256 đã ghi.

Phân giải luôn tường minh và chỉ cục bộ. `AcceptRemote` giải quyết record và hủy
intent cũ trong một transaction, không gọi/sửa server và không xóa đồng bộ nội
dung staged. `RetryLocalAgainstCurrentBase` kiểm tra operation vẫn hợp lệ và
nguồn staged cần thiết còn tồn tại, rồi atomically supersede intent gốc bất biến
và tạo đúng một intent mới có liên kết với revision node/parent chuẩn hiện tại.
Gọi lại phân giải trả cùng kết quả. Outbound engine bình thường gửi intent mới
sau đó, nên một thay đổi server khác vẫn có thể tạo xung đột mới. `KeepBoth`,
merge tổng quát, đặt tên conflict-copy, force overwrite, phân giải nền và tự
chọn bên thắng chưa được hỗ trợ.

Rebaseline, checkpoint handoff và conflict là cơ chế riêng. Activate snapshot
chỉ thêm conflict khi chứng minh được precondition node hoặc parent chính xác đã
cũ; không giải quyết conflict hay sửa intent. Conflict không trigger rebaseline
và không pin journal retention, payload snapshot, handoff proof hoặc checkpoint
thiết bị.

## Chu kỳ hai chiều bounded (Prompt 91)

`BidirectionalSyncCycleRunner::run_once(observed_at)` là composition one-shot,
trung lập transport, chuẩn cho một library. Các phase theo đúng thứ tự là:

1. inspect durable state cục bộ;
2. gọi `RebaselineConvergenceCoordinator::run_convergence_once()` một lần; và
3. sau lần kiểm tra eligibility cục bộ mới, gọi
   `OutboundSubmissionEngine::process_next_ready_intent()` tối đa một lần.

Convergence coordinator vẫn là owner duy nhất của inbound ordinary và
recovery retained-history/proof-loss Prompt 87. Outbound engine vẫn là owner
duy nhất của chọn intent bền vững, mutation/upload, idempotency,
reconciliation và conflict fence Prompt 88. Prompt 91 không duplicate hai máy
trạng thái và không tạo cycle journal.

Outbound chỉ đủ điều kiện sau `IncrementalReady`, incremental progress hữu
hạn an toàn hoặc `RebaselineConverged`, đồng thời lần inspect thứ hai không
còn candidate chưa hoàn tất, handoff đang chờ, bootstrap, inbound page/ACK
đang chờ, local issue hoặc root bị thiếu. Candidate và handoff vì thế được ưu
tiên trước outbound không liên quan. Inbound idle vẫn cho một cơ hội outbound.
Conflict Prompt 88 chưa giải quyết không chặn inbound; outbound engine trả
`BlockedByConflict` mà không submit mutation.

Kết quả typed là `SyncCycleResult { inbound, outbound }`, giữ nguyên kết quả
convergence và outbound, đồng thời cho biết progress durable, idle, khả năng
còn work, cần resolution hay cần authentication. Auth, transport failure,
rate-limit và `RecoveryBlocked` đều bỏ qua outbound. SQLite/protocol/invariant
failure bất ngờ vẫn là `Err`.

Inbound ordinary vẫn giữ luật overlap bảo thủ hiện có. Nếu page remote ảnh
hưởng intent outbound đang hoạt động, inbound engine giữ intent ở
`NEEDS_REBASE_VALIDATION` và ghi observation issue typed
`BASE_STATE_CHANGED` trước ACK. Cycle báo inbound unsafe và không tự tạo
classifier Prompt 88 thứ hai hoặc ghi đè local work. Conflict Prompt 88 chuẩn
tiếp tục do server-precondition hiện có hoặc phân loại exact của Prompt 87
rebaseline tạo ra.

Một cycle ordinary bị giới hạn ở một inbound feed page và một unit
intent/submission outbound. Khi cần recovery, Prompt 87 tự thực hiện download
rebaseline hữu hạn và tối đa một snapshot mới. Cycle không thêm outer loop,
recursion, retry, backoff, sleep, polling hoặc drain toàn bộ feed/queue. Cycle
không có scheduler/daemon; runtime Prompt 92 gọi nó và tự sở hữu lifecycle
policy riêng.

Runner restart-stateless. Page/cursor, candidate, handoff, intent, upload,
idempotency, reconciliation và conflict durable vẫn là source of truth khi
caller dừng giữa phase hoặc mất response. Caller cùng library dùng lại
convergence guard và replica-writer guard theo library hiện có; library khác
vẫn tiến độc lập. HTTP nằm ngoài SQLite writer transaction dài.

## Runtime đồng bộ chạy dài (Prompt 92)

`SyncRuntime` lặp lại cycle bounded Prompt 91 nhưng không trở thành state
machine sync thứ hai. Mọi execution delegate qua port `SyncCycleExecutor` do
`BidirectionalSyncCycleRunner` implement; runtime không gọi trực tiếp inbound
engine, outbound engine, checkpoint, snapshot hoặc handoff API và không mutate
sync state durable.

Registration là explicit. Embedder cung cấp một runner Prompt 91 đã có
transport authenticated cho mỗi Library và có thể unregister mà không xóa
local sync data. Điều này cần thiết vì row `replicas` durable không thể tự
dựng an toàn filesystem root, profile, secure credential, remote adapter và
runner. Register khi stopped được giữ cho lần start sau; register khi đang
chạy schedule Library ngay; register trùng giữ nguyên runner đầu tiên.

`start()` tạo đúng một supervisor và schedule mỗi Library đã đăng ký một lần.
`request_shutdown()` ngăn cycle mới. `stop()`/`shutdown()` gửi cùng graceful
stop và `join()` chờ supervisor. Prompt 91 bounded call đang chạy được hoàn
tất trước khi join trả; không abort future cấp dưới quan trọng. Shutdown/join
lặp lại an toàn. Restart không cần repair runtime: cursor, candidate, handoff,
intent, upload, idempotency, reconciliation và conflict durable Prompt 87/88
vẫn là source of truth.

Wake surface nhận `Startup`, `LocalChange`, `Manual`, `Periodic`,
`NetworkAvailable`, `CredentialChanged` và `PreviousProgress`. Mỗi Library có
một reason pending được coalesced. Wake trong cycle active không start call
đồng thời mà tạo follow-up opportunity. Wake local hoặc manual khi idle ngắt
idle poll. Wake manual, network-available và credential-change có thể bypass
transient backoff. Manual và credential wake resume auth suspension. Không wake
nào bypass auth, conflict, recovery hay outbound precondition của Prompt 91/88.
Với contract Prompt 91 hiện tại không có retry-after duration, runtime dùng
fallback rate-limit 30 giây đã validate.

Safety poll idle mặc định 30 giây, khoảng hợp lệ một giây đến một giờ. Work có
progress được fair follow-up tất định 1 ms thay vì drain không giới hạn trong
cùng tick. Supervisor chọn round-robin, tối đa một cycle mỗi Library, mặc định
bốn Library concurrent và hard maximum đã validate là 1.024. Transient hoặc
recovery-blocked backoff theo 1, 2, 4, 8, 16, 32 rồi 60 giây; không jitter và
không retry ngay. Auth-blocked không hammer server định kỳ. Library conflict-
blocked vẫn safety-poll inbound trong khi Prompt 88 fence outbound. Local
fault hoặc panic chỉ cô lập Library đó, không dừng peer.

Status và event chỉ có category bounded và duration tương đối. Không chứa
credential, cookie, token, absolute path, raw name, content, opaque evidence
hay conflict payload. Runtime cố ý không thêm scheduler migration/table,
systemd hoặc Windows Service, launchd/autostart, watcher wiring, SSE/WebSocket,
broker, server push, route, frontend persistence hay OS network monitor.

## Tích hợp signal runtime theo thứ tự durable-change trước (Prompt 93)

Prompt 93 nối producer local hiện có vào runtime Prompt 92 mà không đổi
semantics sync Prompt 91. Flow chuẩn là:

```text
filesystem/controller/platform event
  -> classify hoặc validate event
  -> commit local state durable
  -> release boundary writer/transaction theo Library
  -> gửi một runtime wake best-effort
  -> Prompt 92 schedule một cycle Prompt 91 bounded
```

`OutboundIntentProducer` trả durable result tách khỏi wake result. `Committed`
nghĩa intent được insert hoặc active intent được coalesce; exact semantic
duplicate là `NoChange`. Wake `RuntimeStopped` hoặc `UnknownLibrary` không phải
durable failure và không rollback intent. Notifier chỉ nhận Library ID cùng
reason wake closed; path, content, credential, cookie, token, checkpoint và
conflict evidence không đi vào runtime.

Observer giữ thứ tự đó cho local observation create, modify, rename, move,
delete và restore liên quan. Một poll/reconciliation bounded gom intent thay
đổi rồi emit tối đa một wake `LocalChange` cho Library. Rescan trải qua nhiều
unit bounded giữ pending change bit và emit một lần khi reconciliation hoàn tất.
Overflow detection, reinspection authoritative, rename attribution và
self-generated suppression durable không thay đổi. Observation no-op, control
path `.synveil/` bị ignore và result bị suppression không wake.

Credential storage vẫn là lifecycle SecretStore theo profile hiện có.
`CredentialChanged` chỉ emit sau enrollment hoặc replacement usable đã qua
secure-store verification và transaction enrollment metadata. Validation/
persistence fail không emit, removal/logout không emit synthetic wake. Danh sách
Library affected là explicit và deduplicate vì registration hiện tại sở hữu
mapping.

`network_available()` là hint từ controller/platform cho Library đã register.
Nó có thể release transient backoff nhưng không bypass authentication, conflict
fence, recovery rebaseline hay outbound precondition. `sync_now(library_id)`
cũng chỉ là scheduling request: đi qua Prompt 92 đến Prompt 91, trả status
bounded như `Queued`, `Coalesced`, không claim completion và không drain work
đồng bộ. Manual wake trong cycle active được giữ thành một follow-up opportunity.

Mọi clone runtime và notifier cùng trỏ tới một supervisor. Race registration/
unregistration có thể làm wake immediate unavailable nhưng không thể xóa
intent durable. Nếu process crash sau commit trước wake, hoặc notifier cố ý drop
signal, startup và periodic safety poll sẽ khôi phục work. Phase này không có
persistent wake queue, runtime migration, OS network monitor, service
integration, watcher mới, server push channel hay frontend change.

## Host đồng bộ desktop và composition lifecycle Prompt 94

`DesktopSyncHost` là composition root cấp application cho Prompt 91–93. Host sở
hữu một `SyncRuntime` cho context owner/Device được process hỗ trợ, giữ
registration explicit của Library/replica durable, runner Prompt 91, observer
tùy chọn, credential provider bind theo profile và controller handle narrow.
Host không phải state machine đồng bộ mới, lifecycle host không persist vào
SQLite hay PostgreSQL.

Library đầu tiên thiết lập context owner/Device đó. Registration tiếp theo phải
khớp context hoặc nhận lỗi typed `WrongScope`; một host không âm thầm trộn
credential domain của nhiều account/device.

Host có hai stage. Construction mở hoặc nhận `LocalStateStore` hiện có,
validate managed-root/profile binding, compose graph Prompt 91 (`InboundSyncEngine`
-> `RebaselineConvergenceCoordinator` cùng `OutboundSubmissionEngine` ->
`BidirectionalSyncCycleRunner`) và register mỗi Library đúng một lần vào một
runtime. Construction không start background work. HTTP Library được construct
khi enrollment/secret usable còn thiếu; credential chỉ được load qua
`SecretStore` bind profile khi cycle chạy. Với `HttpSyncRemote` immutable hiện
tại, credential ID replacement đã verify chỉ rebuild authenticated runner graph
của Library affected. Secret byte không vào runtime status, event hay
controller handle.

Startup start runtime duy nhất trước khi enable filesystem observer. Nhờ vậy
mọi `LocalChange` do observer tạo đều đi đến runtime đã register. Nếu wake bị
mất, intent vẫn đã commit durable và startup/periodic polling Prompt 92 là
đường recovery. Dynamic register khi host đang chạy dùng runtime đó và start
observer sau registration; unregister chỉ bỏ runtime entry ephemeral, không xóa
sync data.

Handle chỉ expose status/event bounded, `sync_now`, `network_available`,
credential-change scheduling và producer/controller Prompt 93 hiện có. Manual
và network chỉ là hint, không bypass authentication, rebaseline, conflict hay
outbound precondition Prompt 91/88. Adapter lifecycle/network Linux và Windows
có semantics giống nhau: process embedding deliver shutdown hoặc positive
network-available hint. Không có feed loop, retry/conflict policy, checkpoint
write, OS network monitor, service manager, autostart hay UI trong Prompt 94.

Shutdown đánh dấu host stopping, cancel observer polling, flush/mark observer
reconciliation, request Prompt 92 shutdown, cho bounded Prompt 91 active hoàn
tất, join runtime task và đóng state chỉ khi host sở hữu. Shutdown/join lặp lại
an toàn; Stopped là terminal. Process restart tạo host mới và register lại
Library trên durable state cũ để intent, candidate, handoff và conflict tiếp tục
theo owner hiện có. `Drop` chỉ cancellation best-effort; application phải
explicitly await shutdown/join. Readiness string của full gate là
`SYNVEIL_DESKTOP_SYNC_HOST_READY`, không tự thân là runtime health claim. ADR-036
khóa boundary composition này.

## Fence khi root mất và behavior process production (Prompt 95)

Process foreground `synveil-client` cung cấp một `DesktopSyncHost` và một
`SyncRuntime`. Linux `SIGINT`/`SIGTERM` và Windows Ctrl-C đi vào cùng
graceful shutdown path; network adapter best-effort chỉ cung cấp positive
hint `network_available()`, có periodic fallback bounded khi native inspection
không sẵn sàng. Adapter không gọi sync engine trực tiếp.

Root absence có lifecycle riêng theo Library:

1. Khi bootstrap, root hiện có chỉ được mở lại nếu managed marker
   canonical khớp durable profile/scope/binding. Root missing trở thành
   deferred replica `Unavailable`; bootstrap không tạo path hay marker.
2. Khi `Unavailable`, observer dừng và validate trước khi drain bất kỳ
   OS/manual hint nào đã queue. Runtime đánh dấu Library `RootBlocked`,
   vì vậy periodic, network hay manual scheduling không thể biến root
   unavailable thành delete/trash observation.
3. Khi configured path xuất hiện lại, host kiểm tra canonical identity,
   profile, scope owner/device/library và binding gốc. Mismatch vẫn
   unavailable, không silent rebind.
4. Reappearance hợp lệ chuyển qua `Recovering`, restart watcher hiện có
   một lần, chạy một canonical rescan bounded, sau đó emit một wake
   `RootAvailable`. Thay đổi tìm được trong rescan chỉ thành durable
   observation intent sau khi root hợp lệ.

Fence được áp dụng ở boundary observer và ngay trước cycle. Lifecycle
task và status tách theo Library, nên removable volume mất không làm
sibling healthy dừng. Root status là ephemeral, không phải checkpoint, feed
cursor, journal event, SQLite row hay deletion fact cho user. Rule an toàn
intent, conflict, rebaseline và upload Prompt 91 vẫn là authority sau khi
gate mở.

Process dùng `state.sqlite3.writer.lock` hiện có và không thêm process lock
hay database thứ hai. Wake mất, process crash hoặc restart graceful vẫn
để durable work cho startup và periodic polling khôi phục. Phase này không
thêm setup-secret, distributed limiter, service/autostart, tray/UI hay server
push channel.

## Command và event control cục bộ (Prompt 96)

IPC server `synveil-client` gọi `DesktopSyncHostHandle` hiện có; không gọi
trực tiếp `BidirectionalSyncCycleRunner`, `OutboundSubmissionEngine`, SQLite
hay credential provider. Vì vậy `SyncNow` đi cùng wake path Prompt 93/92 như
controller embedding và chỉ có thể trả `Queued`, `Coalesced`,
`AlreadyRunningFollowupRecorded`, `UnknownLibrary` hoặc `RuntimeStopped` trong
protocol an toàn. Success response là quyết định scheduling, không phải sync
result.

`GetProcessStatus`, `ListLibraries`, `GetLibraryStatus` là projection không
destructive. Root state lấy từ `Available`, `Unavailable`, `Recovering` hiện
có của host. Runtime phase/last outcome được translate thành category bounded;
không giữ mirror mới của correctness state. Auth chỉ là `Blocked` khi runtime
đã thấy outcome auth-blocked canonical, `Ready` khi có authenticated outcome
an toàn; còn lại là `Unknown`.

`SubscribeEvents` dùng event stream runtime hiện có cùng process/root signal và
translate thành invalidation như `LibraryStatusChanged`,
`RootAvailabilityChanged`, `SyncCycleCompleted`. Broadcast bounded và
best-effort. Subscriber lag nhận `Lagged`, phải reconnect hoặc refetch status;
mất event không đổi ordering durable intent, conflict fence, root fence hoặc
cycle correctness.

## Model desktop controller dựa trên IPC (Prompt 97)

Native UI tương lai dùng một `DesktopController` cho mỗi context
profile/process. Controller chỉ dùng `DesktopControlClient` Prompt 96; không
đọc synchronization SQLite database, probe root, load `SecretStore` hay link
đến `DesktopSyncHost`, `SyncRuntime` hoặc engine Prompt 91. Endpoint được
resolve qua Prompt 96/platform hiện có để khác biệt Linux UDS và Windows named
pipe nằm dưới model boundary.

Startup thực hiện handshake v1 mới, subscribe event bounded, fetch
`GetProcessStatus` và `ListLibraries`, fetch status của từng Library được list,
rồi publish một snapshot coherent. Model chỉ map process state an toàn và
category runtime/root/auth/conflict hiện có. Raw root và mọi credential/
transport material không xuất hiện. `watch` receiver expose snapshot mới nhất
mà không giữ callback queue hoặc block UI chậm.

Event là invalidation best-effort. Một pending-refresh bit và một event-reader
task coalesce burst, chỉ cho một refresh chạy cùng lúc và giữ một follow-up khi
signal đến trong lúc refresh. Refresh fail do connection rớt sẽ đánh dấu
snapshot giữ lại là stale và vào reconnecting; không bịa status fresh.
Reconnect dùng lịch bounded 250 ms, 500 ms, 1 s, 2 s, 4 s, 5 s, reset sau
success và tạo connection generation mới. Response cũ và event reader cũ bị
generation fence chặn.

`sync_now(library_id)` đi qua command path bounded của controller và gửi đúng
một `SyncNow` Prompt 96. `Accepted`, `Coalesced` và
`AlreadyRunningFollowupRecorded` vẫn chỉ là schedule. Khi disconnect, call trả
result disconnected/unavailable bounded; request đã nhận nhưng mất response
trả `OutcomeUnknown` và không resend. Quy tắc không replay cũng áp dụng cho
`RequestShutdown`. Chỉ command explicit đó mới yêu cầu process lifecycle stop.
`DesktopController::stop()` chỉ đóng và join IPC/event work của controller, nên
đóng UI tương lai vẫn để `synveil-client` và sync runtime chạy. Boundary này
không thêm durable state, migration, route, GUI, tray, service hay autostart.

## Boundary synchronization của shell native Qt 6/QML (Prompt 98)

Shell native là process presentation, không phải sync engine thứ hai. Một
`DesktopUiBridge` Rust sở hữu một `DesktopController` Prompt 97; main window
và tray dùng chung controller và latest snapshot:

```text
QML hoặc tray Sync Now
        -> DesktopUiBridge
           -> DesktopController::sync_now
              -> IPC control local Prompt 96
                 -> synveil-client / DesktopSyncHost / SyncRuntime
```

Qt GUI thread không thực hiện connect, read, reconnect hoặc command wait
blocking. Controller work và refresh chạy trên async runtime của controller.
Snapshot delivery là latest-state và bounded; snapshot complete được map
atomically, còn list giữ lại được đánh dấu `Stale` tới khi nhận generation
fresh. Generation cũ không thể overwrite state mới, và Library đã remove
không thể tiếp tục được chọn.

Shell chỉ hiển thị category an toàn của controller cho process, connection,
freshness, root, authentication, conflict, runtime và scheduling. Shell không
đọc SQLite synchronization, không inspect filesystem root, không access
credential và không mở transport Prompt 96. Shell cũng không spawn hoặc
autostart `synveil-client`; absence và restart được xử lý bằng reconnect
semantic Prompt 97 và vẫn giữ UI disconnected/reconnecting usable.

`Sync Now` đi qua bounded command path của controller. UI chỉ được báo
feedback scheduling generic: accepted nghĩa là đã request, coalesced nghĩa là
đã running/request được ghi nhận, còn response unknown không phải completion.
Click nhanh không tạo task vô hạn. Root unavailable, authentication
blocked/required, conflict attention, stale status và process unavailable phải
disable action hoặc trả feedback an toàn bounded; UI không bypass controller
gate.

Tray `Open`, `Sync Now` và `Quit Synveil Desktop` dùng chung model. Close-to-tray
bình thường sẽ hide window khi tray có mặt. Tray Quit và fallback không có tray
chỉ stop/join controller work do UI sở hữu. Chúng không gửi Prompt 96
`Shutdown`, không terminate `synveil-client` và không đổi durable sync
correctness. PostgreSQL chỉ liên quan đến acceptance live process/controller
cho request Sync Now thật; QML shell không có database dependency. Boundary
khóa được ghi trong [`ADR-040`](../adr/ADR-040-native-qt-desktop-shell.md).

## Boundary khởi chạy và supervision desktop production (Prompt 99)

Prompt 99 compose launch management lên trên controller hiện có mà không đưa
quyền sở hữu synchronization vào Qt process:

```text
synveil-desktop / DesktopUiBridge
        -> BackgroundClientManager (API process-management typed, bounded)
           -> systemd --user Linux hoặc Task Scheduler theo user Windows
              -> synveil-client
                 -> DesktopSyncHost / SyncRuntime / writer lock Prompt 95
```

Manager chạy sau controller startup và thực hiện một availability inspection
bounded. Nó có thể request start khi endpoint absent, client stopped hoặc user
supervisor inactive. Nó không start replacement cho endpoint security, protocol
incompatible, control malformed, writer conflict hoặc controller terminal state.
Gate theo profile dùng chung trả `AlreadyStarting` cho caller đồng thời và
cooldown bounded chặn spawn storm do reconnect. Reconnect controller, generation
fence và freshness vẫn do Prompt 97 quản lý.

Public result của manager là các category hữu hạn `AlreadyRunning`,
`StartRequested`, `StartedSupervised`, `StartedDirect`, `AlreadyStarting`,
`NotInstalled`, `SupervisorUnavailable`, `LaunchDenied`, `UnsafeState` và
`Failed`. Không result nào mang stderr, PID, path, URL, credential hoặc token.
Qt projection chỉ nhận launch label generic.

Linux dùng `/usr/lib/systemd/user/synveil-client.service` trong package, với
`Type=simple`, `ExecStart=/usr/bin/synveil-client`, `Restart=on-failure`,
`RestartSec` và `StartLimit*` bounded, cùng
`RestartPreventExitStatus=78` lấy từ source taxonomy. Không có dependency
`network-online.target`. Login autostart là explicit `systemctl --user enable`;
install hook và GUI startup không tự enable, disable explicit được tôn trọng.
Unit không bao giờ là system/root service.

Windows dùng Task Scheduler theo current user, least privilege, logon trigger,
đúng sibling canonical `synveil-client.exe`, `IgnoreNew` và restart finite.
Register dùng argv cố định tới `System32\\schtasks.exe`, không password lưu,
không `SYSTEM`, không yêu cầu administrator và không shell interpolation.
Writer lock Prompt 95 vẫn là ownership protection cuối cùng theo profile.

Linux package manifest hiện có client, desktop, user unit, desktop entry/icon,
maintenance payload hiện có và license/notice. Windows packager tạo ZIP
portable unsigned có hai executable, Qt platform/runtime/QML closure thật,
C++ runtime khi cần, `qt.conf` và notice. Script reject SDK/development file,
Linux library, developer path và non-system PE import thiếu. Autostart metadata
chỉ là process-management state: Prompt 99 thêm zero sync record, migration
client/server, HTTP route, OpenAPI operation hoặc web behavior.

Quyết định khóa nằm trong
[`ADR-041`](../adr/ADR-041-production-desktop-launch-orchestration.md). Quy
trình vận hành nằm trong [`DESKTOP_LAUNCH.md`](DESKTOP_LAUNCH.md).

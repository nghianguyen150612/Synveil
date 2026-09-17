# Chiến lược testing, verification và benchmark của Synveil

Trạng thái: **Blueprint chất lượng quy chuẩn; validation Prompt 27 planning và
Prompt 28 physical GC đã được thiết lập, còn gate worker nội bộ bounded Prompt
29 đã implement với evidence focused PostgreSQL/local-ObjectStore live**

Synveil test khả năng bảo toàn dữ liệu user và authorization khi failure, không
chỉ endpoint success. Suite storage, upload, version, synchronization, backup
và restore là release contract. Tính năng không thể thành `IMPLEMENTED` khi một
scenario bắt buộc bị skip, flaky hoặc chỉ được thể hiện bởi mock bỏ qua boundary
transaction/storage thực của nó.

Tài liệu này định nghĩa test oracle, suite và bằng chứng phase. Foundation gate
hiện tại chạy Rust format/check/test/clippy cùng `cargo deny check`, web
lint/typecheck/test/build nghiêm ngặt, OpenAPI validation và Python syntax/
import smoke. Test integration PostgreSQL cần disposable test database được
cấu hình tường minh. Adapter in-memory và production filesystem cục bộ hiện dùng
chung suite conformance độc lập backend, cùng fixture local cho managed layout,
corruption, incomplete state và containment symlink thực tế. Property/fuzz
runner, Playwright, bằng chứng crash/mất điện và cạn disk xác định, evidence trên
Windows runner cùng job recovery/upgrade Docker Compose biệt lập vẫn là bằng
chứng phase tương lai.

Subset upload-session hiện có thêm application test tập trung cho append
exact-offset, streaming nhiều frame và aggregate chunk limit, đối soát sau
restart, checksum failure, conflict revision khi replace và replay completion
exactly-once; cùng test local adapter cho append/finalize/promote bền vững qua
reopen. Test HTTP route bao phủ authentication, CSRF, header/media type strict,
che giấu cross-owner, offset conflict/recovery, retry completion/abort, safe
error/request ID và flow response mất rồi status/resume. Test browser helper
typed chứng minh transport `Blob`/`ArrayBuffer` raw và progress có thẩm quyền từ
server. Các test này không thay thế evidence PostgreSQL locking/integration khi
disposable database chưa có.

Regression capability của upload local mở production filesystem adapter, assert
profile bắt buộc gồm availability, promotion, checksum, read-after-write, file
flush, directory flush, atomic-rename và conditional-delete, sau đó tạo upload
session qua `UploadApplicationService`. Trên Windows, adapter test còn exercise
directory handle writable và flush được dùng trong capability probe cùng đường
promotion. Test vẫn bật trên mọi native CI runner; nếu durability probe fail,
upload phải giữ trạng thái unavailable thay vì đổi thành skip hoặc nới lỏng
contract.

Focused test của HTTP download route còn bao phủ authentication không cần
CSRF, owner scoping của current và historical, semantic full `200` cùng
single-range `206` chính xác, range open-ended/suffix, xử lý `416` deterministic
trước khi mở storage, `If-None-Match` strong trả `304` mà không stream, header/
filename encode an toàn, body nhiều frame có giới hạn và content-read service
thực qua local object store. Các test này không tuyên bố content resolution
end-to-end trên PostgreSQL khi disposable database gate chưa được cấu hình.

Version-history metadata boundary cần thêm focused test cho ordering newest-first
`(committed_at DESC, id DESC)`, tie-break cùng timestamp, cursor continuation
bounded có scope theo node và reject tamper, currentness lấy từ
`Node.current_version_id`, concealment owner/cross-owner, trạng thái
trashed/purging, xử lý directory, direct lookup tương thích với historical
download ID, allowlist field DTO an toàn, response private no-store và không có
ObjectStore read. PostgreSQL integration gate phải exercise join thực giữa
owner/library/node/version/object và báo status rõ ràng; `SYNVEIL_TEST_DATABASE_URL`
không được set không thể coi là bằng chứng PostgreSQL end-to-end.

Safe version-restore boundary còn có focused test cho authentication, CSRF,
`If-Match` bắt buộc, idempotency key bounded, chi tiết stale revision conflict,
concealment owner/cross-node, reject directory/trashed/current version,
allowlist response an toàn, replay một outcome với đúng một version mới và
idempotency-key conflict. PostgreSQL restore test bị ignore exercise transaction
thực cho chọn replica verified, reuse cùng object, parent là head trước
restore, advance node pointer, historical row bất biến, replay không duplicate,
stale failure không mutation, reject current version và failure khi thiếu
replica verified. Đây chưa phải bằng chứng cho tới khi cung cấp fresh
PostgreSQL disposable URL.

Policy Trash-retention có focused unit test cho default 30 ngày, validation
configuration, deadline dẫn xuất, boundary inclusive, eligibility dùng server
time, loại `ACTIVE`/restored/`PURGING`/root và clear timestamp khi restore. API
metadata test kiểm tra field additive `trashed_at`, `restore_deadline` và
`purge_eligible`. PostgreSQL retention test bị ignore nhưng exercise owner
concealment, directory rỗng, keyset continuation bounded ổn định, restore sau
deadline, reject trước deadline, begin `PURGING` chỉ metadata, repeated-begin
theo revision, locking restore-vs-purge và bảo toàn row version/object/replica.
Đây chưa là evidence PostgreSQL cho tới khi `SYNVEIL_TEST_DATABASE_URL` trỏ
đến database disposable mới.

Metadata-purge PostgreSQL test cũng bị ignore nếu thiếu gate này. Test bao phủ
boundary nội bộ cho state active/trashed/PURGING, root, parent và child; reject
revision cũ; replay cùng revision; duplicate worker concurrent; xóa nguyên tử
Node cùng mọi row FileVersion; clear current-version pointer và
restore-operation; hạch toán reference object dùng chung, gồm reference tạo bởi
restore; bảo toàn row Object/ObjectReplica; clear candidate khi FileVersion mới
reference lại object; defer FK upload-parent; rollback sau lỗi completion-record
muộn; cùng staging và zero-byte. Đây chưa là evidence PostgreSQL cho tới khi
`SYNVEIL_TEST_DATABASE_URL` trỏ đến database disposable mới.

Content-read service trung lập transport còn có focused application test cho
concealment owner/missing/cross-owner, directory, node trashed, historical
version bất biến, stream full và zero-byte, range interior/final/past-end,
tạo range rỗng/overflow, replica missing/chưa verify, metadata length/SHA-256
mismatch và bỏ stream không mutation. Một local `ObjectStore` integration test
seed object đã commit rồi chứng minh một full read và một range read qua cùng
service. Fake và local adapter này validate service boundary; chúng không thay
thế test content-resolution PostgreSQL bị gate theo môi trường.

## Nguyên tắc chất lượng

1. **Bất biến trước ví dụ.** Mỗi ví dụ có oracle state/authorization/durability;
   “HTTP 200” không đủ.
2. **Dependency thực tại boundary.** Test transaction/locking PostgreSQL dùng
   PostgreSQL được hỗ trợ, và gate adapter production dùng implementation
   filesystem/S3-compatible thực.
3. **Fault injection xác định.** Mọi khoảng trống không an toàn giữa object I/O,
   database commit, response và background work là test point có thể định địa chỉ.
4. **Retry là bình thường.** Request, change page, webhook và job có thể lặp;
   test chứng minh outcome idempotent thay vì giả định giao đúng một lần.
5. **Crash recovery là tính năng.** Kill/restart tại state có tên và assert
   durable state đã recovery, không phải in-memory state trước khi chết.
6. **Dữ liệu adversarial là input thông thường.** Tên, size, cursor, MIME type,
   media, archive, content repository và timestamp không tin cậy.
7. **Tùy chọn nghĩa là có thể loại bỏ.** Core suite chạy khi AI, thumbnailing và
   Forgejo absent, down và slow.
8. **Restore chứng minh backup.** Manifest đã lưu hoặc backup job xanh không đủ
   tới khi clean restore verify byte và báo phạm vi.
9. **Compatibility bền vững.** Migration, object, cursor/fixture và client được
   hỗ trợ đã phát hành còn trong ma trận upgrade/conformance.
10. **Adoption là bằng chứng.** Install cho người không chuyên, chọn storage,
    pairing, health message, update, uninstall, migration và recovery là product
    workflow phải test riêng, không suy ra từ runbook Compose của operator.
11. **Bằng chứng tái lập được.** Ghi seed, commit/image digest, config, version
    dependency, hardware/backend, fixture và failure/skip chính xác.

## Layer test

| Layer | Mục đích | Ví dụ | Cách dùng khi phát hành |
|---|---|---|---|
| Static/contract | Từ chối drift dependency, type, schema, secret và docs trước runtime | Rust format/lint/deny, TypeScript strict, OpenAPI diff, checksum migration, dependency/license/SBOM, link/parity docs | Mọi thay đổi |
| Unit | Exercise nhanh pure function/state transition | name key, state machine, policy, parse range, error, chọn retention | Mọi thay đổi |
| Property/model | Sinh sequence operation và assert bất biến | model sync, part upload, graph directory, reference GC, retention backup | Mọi thay đổi cho seed có giới hạn; nightly mở rộng |
| Fuzz | Tìm defect parser/decoder/panic/resource-bound | cursor, path/name, header, manifest, codec, webhook, wrapper media | Regression corpus trên PR; nightly/release có time budget |
| Integration | Exercise PostgreSQL thực và ngữ nghĩa adapter thực | transaction/lock, lifecycle upload, lease outbox, conformance local/S3 | Mọi thay đổi liên quan |
| Crash/recovery | Terminate tại failpoint xác định và restart | split object/DB, migration, job, snapshot/restore, GC | Bắt buộc phase/nightly/release |
| Protocol conformance | Giữ server và client theo fixture có version | cursor/page/conflict sync, manifest backup, retry upload | Bắt buộc trước promote protocol/client |
| Security | Negative authorization, abuse, secret/egress và isolation parser | IDOR, CSRF/XSS/SSRF, token replay, bomb, no-egress | Mọi thay đổi boundary; full suite khi release |
| End-to-end | Chứng minh user path tích hợp | browser/API/DB/object, login/upload/trash/restore/share/backup | Merge/release; critical smoke trên PR |
| Platform/distribution | Chứng minh host, installer, service, storage, connectivity, accessibility, update, uninstall và migration | Matrix Windows/macOS/Linux, storage picker/capability probe, pairing, service crash/reboot, signed update, uninstall/migration bảo toàn data | Cross-platform foundation và release từng host được claim |
| Deployment/upgrade | Chứng minh clean install, health, backup, restore và migration | Compose, fixture previous-version, disaster restore | Release candidate |
| Performance/soak | Thiết lập capacity envelope và phát hiện regression/leak | stream, listing, backlog, checksum, queue, restore | Môi trường riêng; release gate |

Mock/fake hữu ích cho pure application logic và failure xác định. Chúng không
chứng nhận isolation PostgreSQL, hành vi fsync/rename/symlink local, hành vi
read-after-write/checksum S3, streaming Caddy, privilege container hay ngữ nghĩa
browser/nền tảng thực.

## Kiến trúc test và harness

### Component harness bắt buộc

- **PostgreSQL ephemeral:** một database/schema biệt lập cho mỗi nhóm test, với
  exact version server/extension được hỗ trợ và migration path thực.
- **Temporary local object root:** tạo dưới test directory riêng, không bao giờ
  là path workspace/home/root; có thể kiểm soát permission, exhaustion space/
  inode, bit flip, file thiếu và test thay symlink.
- **`ObjectStore` fault-injecting:** decorator adapter trung thành contract,
  fail hoặc crash trước/sau operation có tên mà không phát minh ngữ nghĩa mạnh
  hơn adapter thực.
- **Port clock/random/ID xác định:** cho phép test expiry, retention, UUIDv7 và
  race trong khi production dùng provider secure/random/system. ID không được
  dùng làm order hay authorization trong assertion.
- **Transaction failpoint:** hook chỉ dùng test quanh boundary lock/allocation/
  insert/commit/response. Không bao giờ phơi failpoint trong endpoint production.
- **Reference sync model/client:** state machine cố ý nhỏ, dùng fixture có version
  độc lập với server implementation.
- **Virtual backup source:** file identity ổn định cùng content, rename/delete/
  unreadable/symlink/change-during-scan và metadata platform có kiểm soát.
- **Network fault proxy:** drop, duplicate, delay, truncate và reorder traffic
  client, object backend, AI, Forgejo theo script có giới hạn, tái lập được.
- **Compose release lab:** clean install, scan private-port, proxy/TLS, restart
  process/host, system backup/restore và upgrade version được hỗ trợ. Compose
  là profile Advanced / Server, không thay thế evidence Personal / Home.
- **Real-OS release lab:** runner hoặc hardware lab được ghi nhận cho Windows,
  macOS, Linux Desktop và Linux Server để test native/guided install, storage
  discovery/picker, service lifecycle, sleep/reboot/crash recovery, signed
  update, uninstall bảo toàn data, PostgreSQL managed và machine migration. Một
  pass Linux Compose không đại diện cho các profile khác.
- **Filesystem capability probe lab:** NTFS/ReFS, APFS, ext4, XFS, Btrfs và
  profile NAS/object-store có tên phải ghi lại `StorageCapabilities`; test
  portable fallback khi accelerator tùy chọn không có.
- **Accessibility/onboarding harness:** keyboard/screen reader/contrast,
  assertion error plain-language-to-action, progressive-disclosure snapshot,
  vòng đời pairing code và state remote connectivity.
- **Artifact/privacy probe:** canary secret/content/name rồi scan log, metric,
  trace, error, diagnostic, remote-network và image-layer.

Mọi destructive fault test resolve và validate target temporary biệt lập trước
mutation. Chúng không bao giờ dùng storage root user thực.

### Inspection state bắt buộc

Test cần helper read-only an toàn có thể assert:

- tree library hiển thị, current version và immutable history;
- state object/replica, protected reference, lease và checksum stored/plain;
- upload session/part và persisted idempotency outcome;
- epoch/head library và ordered change event;
- backup snapshot/entry và kết quả operation restore;
- attempt/lease/dead letter job/outbox và audit fact;
- source version/freshness/provenance của derived record;
- inventory namespace storage và candidate orphan/quarantine.

Helper là tooling test/internal, không phải debug API production chưa xác thực.
Assertion ưu tiên public behavior cộng invariant audit; direct database read
giải thích failure và verify atomicity nhưng không được bình thường hóa API hỏng.

## Bất biến domain chung

Mọi mutation/fault suite assert bất biến áp dụng:

1. Không `FileVersion` hiển thị nào tham chiếu object partial, missing,
   unverified hay chỉ temporary.
2. Canonical byte, length và hash `sha256:` không bao giờ đổi cho object/version/
   snapshot entry bất biến.
3. Metadata mutation, `ChangeEvent`, `AuditEvent` bắt buộc, outbox work và
   persisted idempotency outcome hiện diện hoặc vắng mặt nguyên tử.
4. Cùng identity idempotency cộng cùng fingerprint trả một stored outcome;
   fingerprint khác bị từ chối và không đổi state.
5. Authorization dẫn xuất từ quan hệ principal/resource hiện tại; ID, storage
   key, hash, cursor hay stale index record không cấp gì.
6. Retry/crash có thể để lại staging/orphan work nhưng không bao giờ tạo logical
   outcome thứ hai hay committed reference không thể truy cập.
7. Restore tạo current state/output tường minh mới và không rewrite immutable
   history.
8. Garbage collection không bao giờ xóa object được tham chiếu hoặc leased;
   state không chắc được giữ/quarantine, không đoán rồi loại.
9. Failure optional worker/provider/connector chỉ đổi freshness dẫn xuất/tích
   hợp, không đổi correctness core.
10. Log/error/metric/trace/diagnostic không chứa raw secret hay fixture content
    và không tiết lộ sự tồn tại cross-user.

## Validation static và contract

Mỗi pull request chạy hoặc ghi applicable check:

- Rust formatting, lint với warning policy đã review, unit/integration test,
  unsafe-code policy và phân tích dependency/license/advisory;
- Typecheck TypeScript strict, lint, unit/component test, production Vite build
  và kiểm tra drift API được sinh;
- Format/type/test/dependency/model-license Python cho path AI tùy chọn;
- Syntax/lint OpenAPI, annotation stable error/idempotency/auth và review
  compatibility diff có chủ ý;
- Ordering migration, checksum release bất biến, fresh apply và upgrade apply;
  schema model drift khi dùng;
- Validation container/config/Compose/Caddy, secret scan, SBOM và policy
  vulnerability image;
- Link Markdown, parse Mermaid khi hỗ trợ, quy tắc terminology/status và sự có
  mặt của document pair Anh/Việt bắt buộc;
- không scaffold client/service aspirational rỗng hay promote feature status
  khi thiếu bằng chứng gate.

File sinh ra khai báo source cùng command regeneration. Test fail nếu phát hiện
hand edit hoặc generation stale.

## Catalogue unit-test

Coverage pure/unit tối thiểu gồm:

- parse/serialization chuẩn UUIDv7 đồng thời chứng minh không hành vi order/
  authorization nào phụ thuộc timestamp bit;
- normalization Unicode/name key portable, forbidden character, bounded length,
  reserved name, uniqueness sibling và conflict name xác định;
- revision/ETag và parse `If-Match`, mapping stable error và safe error redaction;
- parse byte range, overflow, unsatisfiable và policy multipart/range;
- state auth/session/device/share/recovery, expiry, lineage rotation, scope và
  quyết định authorization policy;
- bảng transition `UploadSession`/part và transition invalid;
- phát hiện ancestry/cycle directory, policy move/rename/copy/delete/restore;
- classification conflict và metadata bảo toàn xác định;
- kiểm tra encode/decode/version/integrity/scope/epoch cursor;
- bảng transition backup snapshot/entry/restore, chọn retention và hold;
- reference/lease/eligibility GC object và storage accounting;
- attempt/backoff/jitter/lease-generation/dead-letter/idempotency job;
- eligibility/metadata compression và bounded decode policy;
- provenance, freshness và optional-degradation AI/photo/Git.

Test state-transition liệt kê mọi state và bảo đảm transition undefined fail mà
không mutation.

## Validation change journal bền vững của Prompt 31

Foundation journal bền vững có live-PostgreSQL gate tường minh trong
`crates/metadata/tests/postgres.rs`. Trên database disposable mới, suite ignored
được enable chứng minh migration apply, schema typed round-trip, một event cho
mỗi logical mutation được hỗ trợ, replay idempotent của upload và restore
version, tombstone purge còn lại sau khi xóa Node, owner/library scope, paging
bounded có thứ tự, cursor resume, interleaving reader/write, writer đồng thời,
rollback khi lỗi journal ở cuối transaction và append-only history được enforce
bởi database. Test finalize upload còn chạy hai completion đồng thời và assert
một completion được lưu cùng một journal fact.

Unit test riêng cho foundation cover vocabulary change canonical, xử lý
version/length/integrity/overflow của cursor, boundary PostgreSQL `BIGINT` và
việc tách namespace với cursor file-history/purge. Live suite không chạy trên
database development persistent. Một lệnh explicit đại diện là:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  --ignored --test-threads=1
```

Đây chỉ validate foundation journal. Test checkpoint/feed Prompt 32,
rebaseline server-side Prompt 33 và client mutation Prompt 34 được mô tả bên
dưới. Client-side staged installation và automatic conflict resolution vẫn là
test của sync phase sau.

## Trạng thái và validation synchronization Prompt 35

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
| automatic conflict resolution | `NOT IMPLEMENTED` |
| desktop sync agent | `NOT IMPLEMENTED` |

Live PostgreSQL test Prompt 32 dùng database disposable mới và cover tạo/unique
checkpoint, create đồng thời, concealment owner/library, epoch và sequence zero
khởi tạo deterministic, feed page bounded tăng dần, repeat read ổn định trước
ack, signed delivery evidence, compare-and-set monotonic, replay cùng page và
token cũ, reject gap/out-of-order và future progress, device/library độc lập,
reader đồng thời, ack đồng thời, writer chạy chồng feed, history expiry, epoch
rollover và từ chối device revoked. API test cover session auth, CSRF bất đối
xứng, header private/no-store, body/limit bounded, safe error, scope
concealment, DTO chỉ logical và không lộ token. Lệnh live bắt buộc là:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  --ignored --test-threads=1
```

Suite API và metadata của Prompt 34 đã implement và test logical mutation
boundary authenticated. Local watcher, staged desktop agent, conflict
auto-resolver, WebSocket/SSE và broker vẫn nằm ngoài phase này.

## Validation snapshot/rebaseline logical Prompt 33

Unit coverage chứng minh vocabulary state bootstrap đóng; invariant projection
logical có type; shape canonical root/directory/file; cursor round trip, scope
binding, reject tamper/oversize; completion-token round trip và binding claim
owner/device/library/session/generation; HMAC domain cursor/token riêng; encoding
terminal manifest rỗng; cùng `Debug` secret đã redact. API coverage kiểm tra auth
và CSRF cho start/complete, auth GET không CSRF, body 2 KiB, strict JSON field,
bound page limit, cursor/completion token malformed/tampered/oversized,
cross-owner concealment, từ chối device revoked/unknown, response success/error
private/no-store, mapping page/retry tất định, terminal proof, exact checkpoint,
response-loss replay và DTO allowlist không có field storage vật lý.

Live PostgreSQL test ignored
`postgres_sync_rebaseline_bootstrap_is_coherent_bounded_and_fenced` là bắt buộc
trên database disposable mới. Test apply migration Prompt 33, verify schema
state/manifest đúng scope và không có physical column, rồi chứng minh:

- library rỗng theo nội dung user vẫn complete canonical root tại sequence zero;
  Node `ACTIVE`/`TRASHED` hiện tại được include; `PURGING`/đã purge bị exclude;
  current version/length/hash được project an toàn;
- epoch/resume cut nhất quán được capture cùng manifest bất biến, keyset order
  Node-ID ổn định, page read bounded `limit + 1`, retry first/terminal page tất
  định, tiếp tục sau service restart và checkpoint giữ nguyên từng byte trong
  mọi page read;
- concurrency PostgreSQL thật cho directory create, rename, move, Trash,
  metadata purge, upload finalization/content replacement đã verify và version
  restore. Mỗi assertion yêu cầu outcome committed nằm trong projection đúng
  khi journal event ở trước/tại cut; nếu không event phải nghiêm ngặt sau cut;
- snapshot nhiều page vẫn trả projection cũ đã capture sau khi rename, move,
  Trash, create và content finalization đồng thời xảy ra sau page 1. Feed sau cut
  reconcile các change đó và mọi sequence trả về đều lớn hơn resume sequence;
- completion reject evidence không terminal mà không đổi checkpoint, rồi thay
  epoch/acknowledged sequence nguyên tử đúng cut; replay sau mất response là
  idempotent;
- checkpoint đã đi trước, generation expired/replaced, epoch rotation hay
  minimum-retained-sequence invalidation đều fail closed không rewind; start
  đồng thời trả cùng một bootstrap `OPEN`; và
- cleanup session retired có giới hạn chỉ xóa state bootstrap/manifest, giữ
  canonical root cùng journal fact.

Các lệnh live focused chính xác là:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_change_journal_is_ordered_scoped_resumable_and_atomic \
  --ignored --exact --test-threads=1
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_device_sync_checkpoints_and_feed_are_bounded_monotonic_and_scoped \
  --ignored --exact --test-threads=1
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_sync_rebaseline_bootstrap_is_coherent_bounded_and_fenced \
  --ignored --exact --test-threads=1
```

Fake backend hoặc test ignored chưa chạy không phải evidence PostgreSQL/no-gap
Prompt 33. Gate cuối còn chạy lại upload finalization, version restore,
Trash/restore, metadata purge, journal atomicity, sync feed/ack, GC
planning/execution/worker, local ObjectStore conformance và regression upload
durability Windows hiện có.

## Validation consistency logical snapshot Prompt 81

Unit test trong `synveil-core` validate aggregate `LogicalSnapshot` theo scope
Library: đúng một root directory active, order bằng immutable Node ID, validate
topology parent, state current `ACTIVE`/`TRASHED`, reject duplicate/missing
parent và loại shape nội bộ `PURGING`. Unit test metadata validate mapping
journal sequence có checked arithmetic và snapshot boundary có type.

Suite PostgreSQL ignored `snapshot_postgres` chạy trên database PostgreSQL 17
mới. 13 test cover state empty/root, hierarchy một node, isolation owner và
Library, fixture 250 node có order deterministic, current rename/move và
Trash/restore, concurrent create/rename/move/Trash/restore với cut trước/sau,
continuation sau cursor trả về không gap hay duplicate event trước snapshot và
checkpoint device không đổi. Mỗi case concurrent chỉ accept hai outcome hợp lệ:
mutation đã được snapshot phản ánh ở trước/tại journal event, hoặc vắng khỏi
snapshot và event strictly sau boundary. Query service theo set, không include
physical storage identity.

Chạy live focused suite:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test snapshot_postgres --locked -- \
  --ignored --test-threads=1
```

Prompt 81 tự nó cố ý không thêm migration, HTTP route, client snapshot apply,
checkpoint completion, journal retention, conflict policy, cache hay background
runtime.

## Artifact snapshot rebaseline durable Prompt 82

Unit test `synveil-core` nay chứng minh `RebaselineSnapshotId` UUIDv7,
page cursor scope theo snapshot khác type với `JournalCursor`, descriptor
construction, page-size bound (1..=1000, default 256) và expiry boundary bao
gồm (`observed_at == expires_at` là expired). Unit metadata còn kiểm tra
boundary có type của descriptor và so sánh expiry an toàn.

Suite PostgreSQL 17 ignored `durable_snapshot_postgres` tạo migration 35 từ
rỗng và cover materialization root/hierarchy, descriptor count chính xác,
equivalence với Prompt 81, keyset reconstruct fixture 1.001 entry, reject page
size, owner concealment/library isolation, reject cursor giữa artifact khác,
expiry exact, paging cross-connection, restart thật với pool mới, immutability
sau materialization, journal continuation sau cut đã lưu, checkpoint invariance,
rollback copy entry deterministic, create/mutation concurrent lặp lại có báo
stress `40P01=0`, và upgrade từ schema lịch sử 34 migration vẫn giữ data owner/
Library/Node/journal/checkpoint.

Chạy suite durable tuần tự trên database PostgreSQL 17 disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test durable_snapshot_postgres --locked -- \
  --ignored --test-threads=1
```

Fixture upgrade Prompt 82 vẫn dựng lại boundary lịch sử 35 migration tại
`20260908000000_rebaseline_durable_snapshots.sql` trước khi áp dụng migration 36
hiện tại, không làm drift checksum lịch sử. Bản thân Prompt 82 không thêm
HTTP/OpenAPI/SSE/WebSocket, client application,
complete rebaseline, journal retention, cleanup daemon, retry loop, global
serialization hay deployment runtime. Creation validate aggregate đầy đủ với
memory tạm O(total-node-count); chỉ page read sau đó mới có transfer bound
O(page-size).

## Boundary abuse durable snapshot và direct HTTP proof Prompt 83C

Rule admission khi tạo durable được chứng minh trong cùng suite PostgreSQL 17
`durable_snapshot_postgres`. Suite seed bảy artifact active, reject boundary
thứ tám trở đi một cách atomic, verify header/entry count, library journal head,
journal row và device checkpoint không đổi sau rejection, rồi chứng minh
isolation owner/Library và re-admission đúng expiry. Barrier start mười hai
caller không retry tại boundary `N-1`; kết quả kỳ vọng là một admission, mười
một `ActiveArtifactLimitReached` typed result, zero deadlock (`40P01`), zero
unexpected database error và zero timeout. Stress coherence snapshot/mutation
15 round hiện báo riêng bảy admission rejection dự kiến với database failure.

Chạy durable và direct HTTP suite tuần tự trên database PostgreSQL 17
disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test durable_snapshot_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-api --test rebaseline_snapshot_http_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
```

File direct HTTP có 25 ignored test: 21 test Prompt 83C và 4 test handoff
Prompt 85. Ngoài descriptor/page, auth, cache, cursor, body, checkpoint và coherence case hiện có, suite chứng
minh authorized expired descriptor/page trả `410 snapshot_expired`, foreign
expired descriptor/page trả `404 not_found` thay vì `410`, device đã revoke
trả `device_revoked` canonical trên create/descriptor/page, và concurrent HTTP
admission tại durable limit trả `429 rate_limited` cùng `Retry-After: 1`.
Outbound desktop rename/content round trip exact là live regression riêng bắt
buộc phải chạy:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-api --test desktop_remote \
  outbound_rename_and_content_round_trip_converges_without_watcher_bounce \
  --locked -- --ignored --nocapture
```

## Handoff checkpoint bền vững và resume incremental crash-safe Prompt 85

Prompt 85 hoàn tất fence `AppliedPendingHandoff` do Prompt 84 tạo ra. Server
derive boundary từ handoff proof bền vững, immutable và theo owner (trước Prompt
86 là snapshot header), chỉ lock checkpoint
của đúng device, chỉ install boundary khi checkpoint thiếu hoặc còn thấp hơn,
và trả typed conflict nếu checkpoint đã ahead hoặc ở epoch mới hơn. Handoff
không đọc snapshot entries, không dùng ordinary ACK evidence để advance
checkpoint, không coi proof còn tồn tại là expired, và không mutate payload
snapshot hay journal.

Chín test tập trung `client85_...` cover bảy crash boundary: fence restart
trước request; server failure trước commit; response loss sau server commit;
crash sau server success nhưng trước local commit; SQLite rollback trước
commit; và cặp restart/retry sau local commit. Test còn cover caller concurrent
cùng snapshot, insert outbound intent khi network request bị block, và reject
snapshot sai mà không ghi đè marker. Local transaction cuối set
epoch/applied/acknowledged cursor, đưa replica về `IDLE`, và xóa marker cùng
nhau; network request nằm ngoài writer lock local.

Chạy client proof deterministic:

```text
cargo test -p synveil-client-sync --lib client85_ --locked -- --nocapture
```

Target PostgreSQL ignored `rebaseline_handoff_postgres` chạy hai test. Test
S1-S15 chứng minh service matrix thật: checkpoint thiếu/cũ/exact, epoch cũ/mới,
proof đã hết hạn nhưng vẫn còn, concealment missing/foreign, caller
concurrent, device và library độc lập, cùng snapshot/journal rows không đổi.
Test bổ sung chứng minh race khởi tạo checkpoint thiếu với 8 caller, handoff
đồng thời C1/C2 không rewind, conflict C1 stale sau C2, timing no-gap NG1-NG4
deterministic, feed strictly sau C, empty feed và ordinary ACK sau C. Bốn HTTP
test bổ sung chứng minh scope device-only, empty body strict, response private/no-store,
idempotency, unknown boundary bị reject, payload expired nhưng proof còn vẫn
handoff được,
missing/foreign concealment, ahead-checkpoint conflict, browser bị reject, và
device đã revoke bị reject. Direct HTTP target có tổng cộng 25 ignored test.

Full live chain phải chạy tuần tự trên PostgreSQL 17 disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test rebaseline_handoff_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-api --test rebaseline_snapshot_http_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-api --test rebaseline_client_e2e_84c_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
```

`rebaseline_client_e2e_84c_postgres` dùng API thật và `HttpSyncRemote` để
activate snapshot, chứng minh inbound fence, complete device handoff, restart
client, rồi apply và ACK một feed event mới chỉ sau C. Workflow PostgreSQL 17
chạy các command này với `set -euo pipefail`; đây là hard-fail gate, không dùng
`|| true` hay continue-on-error. Token acceptance cuối
`SYNVEIL_REBASELINE_CHECKPOINT_HANDOFF_READY` chỉ hợp lệ khi migration, Rust,
client, direct HTTP và live PostgreSQL gates đều pass.

## Journal retention và physical cleanup bounded Prompt 86

Prompt 86 thêm maintenance one-shot, transport-neutral, dùng thời gian explicit
qua `SyncRetentionService`; không thêm route, daemon, timer, retry loop, client
recovery hay conflict policy. Policy gen-1 đã validate giữ journal 30 ngày, giữ
handoff proof immutable đến 30 ngày sau khi payload snapshot hết hạn, và mặc
định batch 10.000 journal row, 32 snapshot artifact, 128 proof row. Maximum
tương ứng là 100.000, 1.024 và 4.096; giá trị zero hoặc lớn hơn bị reject.

Migration 36 tái sử dụng `libraries.minimum_retained_sequence` làm boundary
compacted-through bền vững cho epoch hiện tại và backfill proof immutable, theo
owner cho mọi snapshot của migration 35. Khi tạo snapshot, header, entries và
proof được ghi trong cùng transaction. Payload cleanup chỉ được xóa header đã
hết hạn cùng entries khi proof tồn tại; proof không có foreign key đến header
nên vẫn còn sau đó. Proof cleanup yêu cầu đồng thời đạt deadline inclusive và
payload không còn. Khi proof còn tồn tại, boundary nhỏ nhất cùng scope cap
journal compaction. Device checkpoint không pin retention; cleanup không đổi
checkpoint, journal head hay epoch.

Target PostgreSQL 17 ignored `sync_retention_postgres` có bảy nhóm fixture bao
phủ toàn bộ retention matrix: upgrade 35 lên 36 với dữ liệu owner, Library,
journal, checkpoint và snapshot đại diện cùng proof backfill; tạo snapshot/proof
atomic cùng rollback; proof immutable; boundary hết hạn payload/proof chính
xác; concealment owner; handoff và feed sau khi xóa payload; journal 25.005 row
được compact theo ba bước 10.000/10.000/5.005; floor persist và monotone; rule
feed below/at/above/no-cursor cùng journal vật lý rỗng; timestamp không monotone
được xử lý bảo thủ; pin single/multiple/oldest/cross-scope; batch payload và
proof; năm payload, mỗi payload 301 entry, được xóa theo batch artifact 2/2/1
(602/602/301 entry) và batch proof 2/2/1; fail closed khi thiếu proof; failure
injection rollback cho cả ba operation; race feed/append/handoff với cleanup
deterministic, gồm cả compaction đồng thời bị cap bởi handoff proof còn giữ; và
tám vòng, bốn caller đồng thời, zero `40P01`, unexpected error, timeout hay retry.

Chạy focused gate tuần tự trên database disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test sync_retention_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
```

Contract stale-feed chính xác: position được yêu cầu thấp hơn durable floor trả
typed `RebaselineRequired` hiện có; position đúng bằng floor hợp lệ và resume từ
floor + 1. Không cursor có nghĩa position zero, nên stale khi floor khác zero kể
cả khi không còn journal row vật lý. Selection là prefix liên tục đủ tuổi bắt
đầu ngay sau floor cũ, bị cap bởi batch size và proof tương thích cũ nhất. Xóa
row và advance floor cùng commit hoặc cùng rollback. Feed giữ library share
lock xuyên suốt đọc floor/head/row; append và cleanup lấy exclusive library
lock tương ứng, nên feed/cleanup chỉ có thể trả page cũ đầy đủ hoặc rebaseline
theo floor mới, không thể silently omit event.

Migration gate hiện tại báo 36 attempted/36 successful, zero failed,
`is_current true`, latest
`20260910000000_sync_retention_handoff_proofs.sql`, với checksum của 35
migration đầu không đổi. Workflow PostgreSQL 17 chạy focused suite này thành
hard-fail step dưới `set -euo pipefail`.

## Bounded rebaseline convergence Prompt 87

Module unit client-sync `rebaseline_convergence` chạy production
`RebaselineConvergenceCoordinator`, `InboundSyncEngine`, `RebaselineApplier` và
SQLite state v5 qua remote scripted transport-neutral. Nó chứng minh incremental
healthy tạo zero snapshot; invalidation retained-history và epoch mismatch
tạo/handoff đúng một snapshot server-authored; signal server explicit no-cursor
bắt đầu recovery mà không tạo legacy bootstrap state; candidate complete được
resume trước POST; pending handoff hợp lệ finalize không POST; proof thiếu chỉ
thay H1 bằng transaction activation H1-to-H2 nguyên tử; và checkpoint conflict
thứ hai dừng với `DidNotConverge` sau đúng một replacement. Checkpoint-ahead
conflict đầu tiên cũng được chứng minh recovery bằng đúng một snapshot current.

Module cũng chứng minh boundary non-trigger/failure: authentication failure giữ
H1 và không create, revoked-device/internal handoff failure cũng không create, 429 trả
`RateLimited` không candidate durable/retry, create response mất không để lại
candidate không có snapshot ID, candidate corrupt fail closed, và page transport
failure giữ đúng candidate để resume mà không có POST thứ hai. Nó còn check
single-create ownership cùng Library, state độc lập giữa Library và preservation
byte-for-byte của pending outbound intent qua replacement H1-to-H2. HTTP adapter
coverage verify request authenticated hiện có
`POST /api/v1/libraries/{library_id}/rebaseline-snapshots`, status 201 chính
xác, descriptor decode strict và boundary chỉ đến từ server response.

Chạy focused local gate:

```text
cargo test -p synveil-client-sync --lib --locked rebaseline_convergence
cargo test -p synveil-client-sync --lib --locked durable_rebaseline_snapshot_create
cargo test -p synveil-api --test rebaseline_convergence_postgres --locked -- --ignored --test-threads=1 --nocapture
cargo test -p synveil-api --test rebaseline_client_e2e_84c_postgres --locked -- --ignored --test-threads=1 --nocapture
```

`rebaseline_convergence_postgres` là target acceptance live riêng của Prompt 87B.
Mỗi ignored test tạo child database mới, verify PostgreSQL 17, start Axum router
thật và device-auth exchange, rồi chạy `HttpSyncRemote` thật qua loopback proxy
để đếm call. Target chứng minh retained-floor kể cả khi journal vật lý rỗng,
negative control tại floor và trên floor, epoch recovery, replacement khi mất
proof, H1 fence trong lúc page S2, thay H1-to-H2 nguyên tử, replacement khi
checkpoint-ahead, proof pin retention trong lúc download S2, candidate resume
sau page failure thật, bounded 429 chỉ một POST, revoked-device không trigger,
và concurrent cùng Library chỉ có một recovery owner. Assertion gồm count chính
xác snapshot/page/handoff, equality checkpoint server/local tại boundary do
server cấp, cleanup candidate/marker, feed và ACK sau recovery, cùng pending
outbound intent không đổi. Handoff recoverable lần hai bị ép fail trả
`DidNotConverge` và chứng minh không tạo S3.

E2E PostgreSQL hiện có vẫn là proof riêng cho pending handoff hợp lệ: coordinator
complete handoff sẵn có mà không replacement POST và Prompt 85 vẫn là path độc
quyền cho checkpoint/finalize. Test client-sync deterministic tiếp tục bao phủ
create-response loss, malformed response, local corruption và transport boundary
không cần fixture raw-socket để suppress response. Workflow PostgreSQL 17 chạy
target dedicated thành hard-fail dưới `set -euo pipefail`; chỉ sau khi mọi live
suite trước đó pass mới emit
`SYNVEIL_SYNC_REBASELINE_CONVERGENCE_READY`.

## Validation client mutation submission Prompt 34

Unit coverage trong `synveil-core` và `synveil-metadata` chứng minh mutation
vocabulary đóng, identity UUIDv7 canonical, typed payload construction,
boundary decimal epoch/sequence, fingerprint SHA-256 có version và deterministic,
parse conflict reason, exact logical result replay và marker `replayed` rõ ràng.
Contract không chứa arbitrary JSON patch, byte payload, object identity hay
physical storage field.

API coverage chứng minh allowlist field strict cho envelope và mọi payload typed,
parse kind chính xác, authentication và CSRF bind session, reject body quá 16
KiB, response success/error private/no-store, concealment owner/device/library,
applied result logical an toàn, conflict detail an toàn, mapping mutation-ID
conflict và rebaseline-required.

Hai live PostgreSQL test
`postgres_client_mutations_are_atomic_idempotent_scoped_and_bootstrap_safe` và
`postgres_client_mutations_are_race_safe_for_duplicate_and_stale_pairs` chạy
trên database disposable mới và chứng minh:

- cả năm namespace/state mutation được hỗ trợ commit một canonical Node change,
  một journal event và một operation row terminal trong cùng transaction;
- retry sau lost response trả đúng Node/event hoặc conflict đã persist với
  `replayed: true`; checkpoint của device gửi không đổi và device khác đọc
  được event qua feed;
- stale revision, parent/name thay đổi, future base sequence, sai epoch,
  retention expiry và resource đã purge đều thất bại deterministic;
- dùng lại ID với payload khác bị từ chối, scope cross-owner/library/device bị
  conceal, transient rollback không để lại operation hay journal fact;
- duplicate đồng thời, rename-vs-rename, move-vs-move, Trash-vs-rename,
  create directory trùng tên và stale write từ hai device được serialize bởi
  namespace guard theo library mà không deadlock hay duplicate fact; và
- bootstrap cut rồi client mutation tạo event post-cut cho feed trong khi
  manifest bootstrap bất biến không đổi.

Lệnh live chính xác:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  --ignored --test-threads=1
```

Focused test dùng row lock PostgreSQL, foreign-key scope, advisory namespace
guard theo transaction và journal/feed thật, không dùng fake backend. Automatic
conflict resolution, staged local apply, content-byte mutation và desktop sync
vẫn chưa implement.

## Validation durable conflict và manual resolution Prompt 35

Unit coverage chứng minh parse conflict/resolution UUIDv7 có type, vocabulary
lifecycle/action đóng, fingerprint SHA-256 typed canonical ổn định và đổi khi
semantic đổi, bound page limit, DTO resolution strict deny-unknown-fields, cùng
cursor bind owner/device/library round trip và reject oversize/sai scope/tamper.
API coverage chứng minh auth cho list/detail, GET không CSRF, POST cần
session-bound CSRF, reject body quá 16 KiB, success/error private/no-store,
concealment cross-scope/Device revoked, envelope stale/terminal ổn định, exact
replay projection và DTO allowlist không có field storage vật lý.

Hai test PostgreSQL disposable mới
`postgres_sync_conflicts_are_durable_inspectable_and_manually_resolvable` và
`postgres_sync_conflict_resolution_is_fenced_race_safe_and_replayable` chứng
minh:

- managed terminal conflict Prompt 34 và operation row link one-to-one trong
  cùng transaction, original replay trả cùng conflict ID, managed reason được
  persist và non-managed failure không tạo conflict;
- evidence lịch sử bất biến, page keyset OPEN bounded có descending order ổn
  định, detail lookup, concealment owner/Device/Library, từ chối Device revoked
  và evidence sống qua rebaseline cùng Node purge;
- `ACCEPT_SERVER` chuyển OPEN thành DISMISSED mà không đổi Node/journal, replay
  chính xác theo ID/fingerprint và fence decision thứ hai khác;
- `APPLY_CLIENT_INTENT` bắt buộc fresh current revision đúng theo mutation, dùng
  executor Prompt 34 thật, commit đúng một Node change/event cùng RESOLVED
  linkage, giữ operation gốc CONFLICT và replay timestamp/event gốc sau mất
  response;
- winner revision/rename/move/purge làm stale persist explicit stale result,
  giữ conflict OPEN và tạo zero resolution mutation/event;
- race APPLY/APPLY và ACCEPT/APPLY cho tối đa một terminal decision và một
  resource event; replay original mutation trong resolution không duplicate
  conflict hay đổi canonical state; và
- apply success nhìn thấy từ Device khác qua feed Prompt 32, checkpoint nguồn
  giữ nguyên từng byte, accept-server không có feed event, và fresh apply sau
  rebaseline success hoặc conflict an toàn.

Test Prompt 35 dùng advisory/row lock PostgreSQL thật cùng timeout 10 giây cho
mỗi race và assert không deadlock. Test còn inspect migration schema tìm column
vật lý bị cấm và thử rewrite evidence trực tiếp để chứng minh trigger database
reject. Lệnh focused là:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_sync_conflicts_are_durable_inspectable_and_manually_resolvable \
  --ignored --exact --test-threads=1
SYNVEIL_TEST_DATABASE_URL=postgresql://<fresh-disposable-loopback-db> \
  cargo test -p synveil-metadata --test postgres --locked -- \
  postgres_sync_conflict_resolution_is_fenced_race_safe_and_replayable \
  --ignored --exact --test-threads=1
```

Gate cuối còn chạy tuần tự mọi test metadata PostgreSQL ignored trên một
database mới, suite auth và storage-GC ignored, toàn workspace non-live,
Clippy strict, cargo-deny, OpenAPI lint, diff check và scan field security/scope
bị cấm. Automatic conflict resolution và desktop sync agent vẫn chưa implement.

## Integration test PostgreSQL

Dùng version PostgreSQL được hỗ trợ, không dùng SQLite hay repository mock đơn
giản, để chứng minh:

- constraint foreign/unique/check và boundary ownership;
- isolation transaction, thứ tự acquisition lock và retry/deadlock có giới hạn;
- allocation clock theo library commit theo visibility order và rollback không
  publish/advance cursor position không dùng được;
- winner của name/move/cycle và content-completion đồng thời;
- atomicity metadata/journal/audit/outbox/idempotency;
- claim job với skip-locked/lease generation, expiry, renewal và conditional
  completion qua replica worker;
- pagination keyset cursor/snapshot khi row được insert/change;
- query retention/GC/reference trên mọi relation được bảo vệ;
- advisory lock migration, interruption transactional/nontransactional và
  checksum bất biến;
- runtime role không thể migrate, đọc secret material không liên quan hay bỏ qua
  giả định authorization row/application.

Mỗi concurrency test dùng barrier để ép interleaving nguy hiểm thay vì dựa vào
may mắn timing.

## Conformance adapter `ObjectStore`

Mọi adapter production—local, profile NAS, S3/MinIO và tương lai—đạt một suite
có version:

Helper dùng chung v1 đã check-in hiện cover identity staging, integrity check
streamed, trạng thái vô hình trước promotion, promotion/conflict create-only,
object zero/large, range full/edge/invalid, metadata, exists, abort, delete
thường/có điều kiện và delete lặp cho cả adapter in-memory lẫn local. Integration
test local bổ sung fixture managed layout, incomplete state, corruption, marker
và containment symlink/reparse thực tế. Ma trận rộng hơn bên dưới vẫn là release
contract; pass unit/integration hiện tại không tuyên bố crash injection, cạn tài
nguyên, race giữa process, cancellation/backpressure hay platform lab.

| Area | Case và oracle |
|---|---|
| Tạo staging | Locator được sinh độc quyền; stream zero/one/large; declaration short/long; cancel; writer crash; không committed visibility. |
| Hoàn tất | Length/checksum mong đợi; duplicate same finalize; concurrent finalize; path capability adapter; success được verify read-after-write. |
| Đọc bất biến | Full và range valid/invalid/edge; concurrent reader; cancellation; byte/header chính xác; không partial nào được phục vụ như hợp lệ. |
| Head/metadata | Kết quả length/checksum/encoding/capability ổn định; distinction missing/permission/network map an toàn. |
| Hủy/cleanup | Abort lặp; abort trong upload failed/expired; không bao giờ xóa dữ liệu committed/shared. |
| Xóa | Exact key do server kiểm soát; delete lặp/missing; gate lease/reference; không traversal prefix/glob/symlink. |
| Corruption | Truncate, bit-flip, metadata mismatch, object thiếu; quarantine/fail, không silent content. |
| Capacity | Exhaustion disk/inode/quota ở partial write/finalize; headroom recovery reserve và không version hiển thị. |
| Retry/network | Timeout trước/sau remote completion, duplicate part, response mất, reconnect; outcome xác định. |
| Enumeration | Listing chỉ hỗ trợ reconciliation; listing eventual/duplicate/missing không thể authorize deletion nếu thiếu reference DB/grace. |
| Durability | Crash process/host tại boundary write/flush/promote/directory-sync dưới profile công bố. |
| Migration | Copy, verify, switch, rollback window, copy bị gián đoạn, outage source/target và retire về sau. |

Suite S3 không bao giờ coi ETag là canonical SHA-256 trừ khi adapter thiết lập
tường minh ngữ nghĩa tương đương cho đúng operation đó. Suite local thực hiện
test symlink/junction/TOCTOU/root-identity.

## Test upload, version, trash và GC

### Ma trận lifecycle upload

| Scenario | Bước | Kết quả bắt buộc |
|---|---|---|
| Replay khi khởi tạo | Gửi initiation giống hệt hai lần với một idempotency key | Một `UploadSession`; trả cùng negotiation/outcome. |
| Key khởi tạo không khớp | Tái sử dụng key với target/size/hash khác | Stable idempotency conflict; không session thứ hai hay target mutation. |
| Part không theo thứ tự | Upload part number/range đã negotiation theo thứ tự xáo | Mỗi part được verify độc lập; completion chỉ thành công khi coverage chính xác. |
| Part trùng, cùng nội dung | Mất response part và gửi lại byte/fingerprint giống hệt | Trả stored verified part; không duplicate staged byte/accounting. |
| Part trùng, khác nội dung | Tái sử dụng part identity với một byte đổi | Conflict/checksum error; verified part ban đầu và session không hỏng. |
| Gap/overlap/overflow | Gửi range missing, overlap, negative/overflow hoặc quá nhiều | Từ chối trước assembly/allocation không an toàn; không file hiển thị. |
| Race expiry | Completion và expiry worker race dưới barrier | Chính xác một terminal state; committed outcome vẫn committed, expired data không thể commit. |
| Completion đồng thời | Hai process complete một session | Một object/version/event/outcome được serialize; loser trả cùng result hoặc stable in-progress retry. |
| Checksum mismatch | Mọi part tới nhưng canonical length/SHA khác | Không version hiển thị; session terminal/retry state theo contract; byte xấu quarantined/cleanable. |
| Hết disk giữa part | Cạn capacity sau partial write | Failure có giới hạn; verified part trước an toàn; không unverified part/version hiển thị. |
| Hết disk lúc promote | Hoàn tất assembly rồi fail final durability | Không DB reference; resumable/failure state rõ; recovery reserve nguyên. |
| Object hoàn tất, DB rollback | Fail trước/trong metadata transaction | Object unauthorized/unreferenced; không node/version/event/outcome; orphan được bảo vệ rồi đối soát. |
| DB commit, mất response | Kill connection/process sau commit trước response | Retry trả đúng node/version/ETag đã commit; không duplicate object reference/event/audit/outbox. |
| Restart trong lúc verify | Kill process khi hash/assembly | Session recovery về retryable/failed state; không partial metadata hiển thị; lease expiry an toàn. |
| Cleanup đã abort/expiry | Chạy cleanup lặp và đồng thời | Staging reclaim một lần; byte committed/reference không chạm; audit/metric ổn định. |
| Race quota | Hai upload mỗi cái riêng lẻ đủ quota nhưng cùng nhau vượt | Transaction/lock chỉ admit kết quả được phép; không bypass quota hay stranded visible reference. |

### Version, trash và restore

- Thay content tạo `FileVersion` bất biến mới; hash/byte cũ và attribution không
  đổi.
- Rename/move không bao giờ tạo/rewrite object byte. Copy có hành vi logical
  identity/version/reference và authorization tường minh.
- Restore version cũ tạo head mới liên kết source đã chọn; retry là một outcome;
  current conflicting mutation dùng base precondition.
- Soft-delete tạo tombstone/trash state và journal fact đã định nghĩa; recursive
  work có giới hạn/restart được. Restore xử lý collision original-parent/name
  xác định.
- Retention version/trash chỉ chọn reference đủ điều kiện. Share, backup snapshot,
  restore, download lease, legal hold hoặc migration đồng thời chặn deletion.
- Mark và sweep GC tách bởi grace/checkpoint. Reference mới trong một trong hai
  phase không thể trỏ tới object đã xóa. Sweep idempotent và dùng exact validated
  key.
- Dry-run báo candidate/reason mà không mutation. Invariant audit so database
  reference, replica và storage inventory trong khi bảo thủ với eventual listing.

### Ma trận planning GC metadata-only của Prompt 27

Integration gate PostgreSQL cho GC planning phải bao phủ boundary đã implement:

| Scenario | Bằng chứng bắt buộc |
|---|---|
| Policy/default | Grace mặc định 24 giờ, lease 15 phút, batch mặc định 100, hard maximum 500, và reject duration hoặc batch zero/không hợp lệ. |
| Grace boundary | Candidate trước grace không claim được; boundary exact `now >= unreferenced_at + grace` và candidate cũ hơn claim được theo giờ PostgreSQL. |
| Claim bounded ổn định | `FOR UPDATE SKIP LOCKED` không claim quá batch cấu hình và theo thứ tự `(unreferenced_at, object_id, dedup_domain_id)`. |
| Reference truth | Candidate có `FileVersion` committed bị cancel bởi recheck `NOT EXISTS` đúng identity `(object_id, object_dedup_domain_id)` và không thành leased/ready. |
| Lease fence | Claim còn hạn bị block; lease hết hạn reclaim được; generation tăng; opaque ID/generation cũ không renew, release hay revalidate successor. |
| Renew/release | Lease live matching renew được; release an toàn/idempotent; stale, expired và cancelled được trả outcome rõ. |
| Re-reference cancellation | Reference mới clear candidate `ELIGIBLE`, `LEASED`, `READY`; re-reference rồi purge tạo lifecycle mới. |
| Ready planning | `READY` chỉ metadata, revalidation lặp được, và reference sau ready xóa row trước mọi physical action. |
| Crash/reconnect | Claim đã commit còn sau mất kết nối, worker khác bị block tới khi hết hạn, worker reconnect reclaim được; ready vẫn có thể revoke. |
| Race thực | Claim/claim, claim/reference và ready/reference đồng thời để lại lease disjoint và không candidate nào có committed reference. |
| Preservation | Count và row đại diện của Object/ObjectReplica không đổi; không gọi ObjectStore delete hay xóa byte. |

### Ma trận physical-GC execution Prompt 28

Gate disposable PostgreSQL cùng production local ObjectStore chỉ chạy với
database fresh do caller khai báo và managed object root tạm riêng. Nó chứng
minh behavior đã implement mà không cần public API:

| Scenario | Bằng chứng bắt buộc |
|---|---|
| Entry và durable plan | Cần `READY` và lease matching còn hạn; operation/action deterministic tồn tại trước storage effect. |
| Final fence | Candidate, lifecycle Object, lease/generation, zero FileVersion và zero active hold được check trong transaction ngắn trước mỗi delete/completion. |
| Replica | Mỗi call xóa đúng một replica verified với evidence key/hash/length/version; metadata còn tới confirmed absence. |
| Local deletion | Adapter local production conditional delete theo version opaque bằng rename-to-tombstone managed; tombstone ngắt là `InProgress`, không phải absent. |
| Ambiguity/absence | Lost response đối soát thành exact-key absence hoặc presence retryable; replica absent sẵn hội tụ an toàn; mismatch fail closed. |
| Recovery | Crash trước persistence, sau một trong nhiều replica, sau lease expiry/reclaim, reconnect và sau physical absence cuối đều resume xác định. |
| Concurrency | GC worker race hội tụ một operation; lifecycle trigger ngăn FileVersion, restore-equivalent reference, active hold hay replica writer mới làm content đã xóa usable. |
| Completion | Mọi action và ObjectReplica phải absent trước candidate/Object removal; replay completed xác định. |

Service không bao giờ đọc full object byte để quyết định delete; test chứng minh
reconciliation path có zero `get` call. Scheduling, rate control và
reconciliation operation bền có giới hạn được bao phủ bởi gate Prompt 29 bên
dưới; inventory reconciliation storage, producer hold backup/share/sync và
public GC API không thuộc gate đã implement nào.

### Ma trận orchestration GC worker nội bộ Prompt 29

Gate worker gọi trực tiếp `run_once()` cho test cycle xác định. Test live yêu
cầu database PostgreSQL disposable fresh do caller khai báo và local ObjectStore
root managed tạm unique; database/path storage người dùng không bao giờ là input
test hợp lệ.

| Scenario | Bằng chứng bắt buộc |
|---|---|
| Configuration/disabled mode | Worker mặc định disabled; duration zero/invalid, concurrency mâu thuẫn, bound invalid và retry policy invalid bị reject trước work. `run_once()` disabled rẻ và không có metadata effect. |
| Cycle bounded | Claim candidate, active operation slice, replica action, task concurrency và storage-delete concurrency tuân thủ cap cấu hình. Library không có loop sleep hay lặp unbounded. |
| Ưu tiên recovery | Operation incomplete đến hạn, gồm lease expired/released và terminal cleanup incomplete, được claim oldest-first trước work mới. Recovery claim làm deferred destructive claim mới sang cycle kế tiếp. |
| Retry bền | Outcome local-store transient/ambiguous tăng `attempt_count`, persist `next_attempt_at` theo clock PostgreSQL, release slice an toàn và không retry trước due time. Delay exponential cap và jitter xác định có giới hạn. |
| Intervention | Evidence mismatch, durable state không an toàn, routing không hỗ trợ và retry budget cạn thành `NEEDS_ATTENTION` đã fenced; chúng không hot-loop hay báo complete. |
| Reconciliation | Discovery set-based bounded báo `GC_DELETING` không operation, operation không candidate, `READY` expired không incomplete work, terminal action cleanup pending và intervention state. Re-reference cancel progress không an toàn qua fence có sẵn. |
| File vật lý không rõ | Worker không recursive-list storage root và không delete file vật lý không rõ. Byte missing ngoài active action vẫn là finding integrity/reconciliation, không phải xóa Object metadata. |
| Outage và shutdown | Database failure không advance destructive work mới; ObjectStore failure persist recovery thay vì cleanup sai. Shutdown dừng claim mới, chỉ drain cycle bounded theo config và để fenced work timeout cho reconciliation sau restart. |
| Race multi-worker | Worker process-equivalent concurrent dùng PostgreSQL `SKIP LOCKED`, candidate lease generation và fence Prompt 28 để hội tụ một physical operation bền, không cần leader election. |
| Integration thực | Candidate `READY` do worker dẫn qua Prompt 28 và local adapter production tới exact physical absence và metadata completion cuối. |

## Contract test synchronization

Synchronization nhận testing sâu bất thường vì defect order/retry có vẻ nhỏ có
thể âm thầm làm mất thay đổi trên mọi thiết bị.

Phạm vi Prompt 31 đã implement là foundation journal server-side: publication
transaction-local, projection có type, ordering theo library, cursor read
bounded và evidence rollback/idempotency/concurrency. Prompt 32 thêm server
feed một chiều đã authenticate, checkpoint bền theo device/library, signed ack
evidence và outcome rebaseline-required tường minh. Prompt 33 thêm manifest
logical bất biến server-side, coherent cut, page bounded retryable, terminal
proof và exact checkpoint handoff. Prompt 34 thêm authenticated typed logical
mutation submission, durable identity/fingerprint, optimistic precondition và
deterministic conflict outcome. Client staged install, arbitrary content-byte
mutation và automatic conflict resolution vẫn là kế hoạch.

### Bất biến sync

Với một `Library` và journal epoch:

1. Mọi namespace/content mutation client nhìn thấy đã commit phát projection
   `ChangeEvent` định nghĩa trong cùng transaction. Mutation rollback không phát.
2. Event sequence unique và phản ánh commit visibility. Khi server issue cursor
   qua sequence `n`, client không phát hiện về sau event committed trước đó bị
   ẩn ở `≤ n`.
3. Page có thứ tự và có thể lặp event đã gửi; không bao giờ âm thầm skip event
   giữa cursor supplied và returned.
4. Cursor được authenticate, versioned, bind với user/library đã authorize cùng
   epoch và mờ đục với client. UUIDv7/time không bao giờ là fallback ordering.
5. Client persist returned cursor chỉ sau khi apply complete page nguyên tử vào
   durable local state. Crash trước đó lặp an toàn.
6. Initial snapshot/rebaseline cùng server checkpoint hội tụ dưới concurrent
   write: mọi node state được snapshot hoặc event về sau biểu diễn, không bao
   giờ bị bỏ giữa chúng.
7. Cùng `client_mutation_id`/fingerprint tạo một mutation/event. Tái sử dụng
   payload khác bị từ chối.
8. Mutation boundary Prompt 34 chỉ nhận namespace/state operation có type; không
   nhận file byte, object/replica identity hay storage locator. Content protocol
   tương lai phải giữ cả incoming/current byte, không làm mất byte theo
   last-writer-wins.
9. Precondition fail trả projection conflict expected/current đã persist và
   không silent overwrite, merge hay tạo conflict copy. Directory ancestry vẫn
   acyclic và sibling name vẫn unique theo portable profile.
10. Tombstone/history tồn tại lâu hơn active cursor window được ghi. Client dưới
    retention hoặc sai epoch nhận rebaseline tường minh, không bao giờ empty page
    hàm ý deletion convergence.
11. Clock/timezone client không quyết định journal order. Timestamp client vẫn
    là metadata không tin cậy.
12. Thiết bị revoked không thể fetch/apply thay đổi server mới hay gửi mutation;
    byte đã tải nằm ngoài khả năng thu hồi.

### Ma trận scenario sync cốt lõi

Mỗi scenario chạy qua HTTP API và reference client, assert hành vi public tree/
version/cursor rồi chạy invariant inspection. `A` và `B` dùng durable local state
và credential độc lập.

| ID | Setup và operation | Hội tụ và bằng chứng bắt buộc |
|---|---|---|
| `SYNC-001` Đồng bộ ban đầu rỗng | Library mới; B yêu cầu snapshot/checkpoint phân trang, rồi change | Tree local bền vững rỗng/root metadata khớp; checkpoint hợp lệ; lặp vô hại. |
| `SYNC-002` Tạo trên A → B | A tạo folder và upload file; B dùng page | B có hierarchy, version ID, length/hash/metadata chính xác; một logical create; byte verify. |
| `SYNC-003` Tạo subtree lồng nhau | A tạo directory/file sâu/rộng với page boundary | Parent prerequisite apply xác định; không orphan local node; replay page an toàn. |
| `SYNC-004` Sửa content trên A → B | B có v1; A commit v2 với base v1 | B advance cùng `Node` tới v2 bất biến, retain/xử lý v1 theo policy, tải v2 đã verify. |
| `SYNC-005` Rename trên A → B | Rename file lớn không đổi content | B cập nhật name/name key; identity content object/version không đổi; một rename fact. |
| `SYNC-006` Move trên A → B | Move file và directory subtree giữa parent | Hierarchy B hội tụ; byte object descendant không re-upload; không descendant trùng. |
| `SYNC-007` Collision tên portable | A/B tạo tên bằng nhau theo normalization/case fold | Chính xác một placement bình thường; cái kia nhận conflict/error đã định nghĩa; không platform collision vô hình. |
| `SYNC-008` Cycle directory | Concurrent/stale move thử đặt parent dưới descendant | Mutation gây cycle fail transactionally; không event/partial ancestry change. |
| `SYNC-009` Xóa trên A → B | A trash active file sau checkpoint B | B thấy transition tombstone/trash và theo local policy; version/history server vẫn retained. |
| `SYNC-010` Xóa folder đệ quy | Trash subtree lớn phân trang | Ngữ nghĩa event/projection có giới hạn đã định nghĩa hội tụ; restart/replay không để mixed invisible state. |
| `SYNC-011` Restore trên A → B | Restore node trashed, original parent trống | B thấy identity/revision và content được restore; event mới, không re-upload/rewrite object cũ. |
| `SYNC-012` Collision tên khi restore | Original parent nay có tên collision | Hành vi conflict/name/destination server đã định nghĩa; cả hai resource được bảo toàn/addressable. |
| `SYNC-013` Purge so với client stale | Retention purge tombstone/history khi B dưới retained cursor | B nhận cursor-expired/rebaseline; không resurrect hay âm thầm giữ sai live state. |
| `SYNC-014` Conflict content/content offline | Server v4; A offline tạo v5A, B v5B; reconnect A rồi B, sau đó đảo thứ tự ở run khác | First valid head và incoming stale byte cùng persist dưới grouping/copy conflict xác định; không mất byte; mọi device hội tụ. |
| `SYNC-015` Content so với rename | A edit từ base trong khi B rename cùng node | Operation tương thích rebase/merge theo spec hoặc trả conflict tường minh; content và current name dự kiến vẫn giải thích được và journaled. |
| `SYNC-016` Content so với delete | A edit offline trong khi B trash node | Conflict edit/delete định nghĩa bảo toàn incoming byte và deletion history; không silent resurrection/overwrite. |
| `SYNC-017` Restore so với edit mới | A restore version cũ trong khi B advance head | Base precondition chọn một head; stale outcome tường minh/được bảo toàn conflict; history cũ bất biến. |
| `SYNC-018` Rename so với rename | A/B offline chọn tên khác từ một revision | Một commit; cái kia nhận current/rebase hoặc conflict xác định; không last arrival nondeterministic. |
| `SYNC-019` Move so với move | Cùng node được move tới hai parent đồng thời | Một commit; loser nhận current state/rebase; ancestry/uniqueness nguyên. |
| `SYNC-020` Move so với xóa parent | A move vào parent mà B đồng thời trash | Transaction lock/precondition ngăn active child dưới invalid parent; winner/error và journal rõ. |
| `SYNC-021` Request mutation trùng | Gửi `client_mutation_id` giống hệt đồng thời và tuần tự | Một domain revision/event/audit/outbox; mọi response resolve stored outcome. |
| `SYNC-022` Payload mutation ID không khớp | Tái sử dụng ID với name/base/content đổi | Stable idempotency conflict; không outcome thứ hai hay rò prior data cross-principal. |
| `SYNC-023` Mất response thành công | Commit mutation, drop response, reconnect/retry | Trả exact prior result; không duplicate version/change; page sau chứa logical event một lần (transport có thể lặp page). |
| `SYNC-024` Change page trùng | Client nhận/apply cùng page hai lần | Local state và conflict record không đổi sau apply lần hai; cursor vẫn hợp lệ. |
| `SYNC-025` Client crash trước commit page | Kill sau apply subset trong memory nhưng trước atomic local DB commit | Restart dùng cursor cũ và reapply full page; final state đúng một lần logic. |
| `SYNC-026` Client crash sau commit page | Commit local page/cursor, kill trước acknowledgement/request kế | Restart tiếp từ stored cursor; không missed event hay duplicate conflict user nhìn thấy. |
| `SYNC-027` Write trong change feed phân trang | Barrier write/move/delete giữa request page | Cursor/page trả về bao phủ mọi event theo order, có thể ở page sau; không event xuất hiện dưới boundary đã advance. |
| `SYNC-028` Write trong initial snapshot | Mutate node khi B scan nhiều page | Protocol snapshot/checkpoint tạo old state cộng later event hoặc new state nhất quán; B đạt authoritative final tree. |
| `SYNC-029` Backlog lớn | B inactive trở lại sau tập event lớn được retain | B catch up với page/memory giới hạn, chịu duplicate/retry, báo lag/progress và hội tụ. |
| `SYNC-030` Cursor dưới retention | Advance retention qua checkpoint B | Stable cursor-expired error với route rebaseline phân trang; không đoán reset-to-zero và không bỏ tombstone. |
| `SYNC-031` Cursor sai library/user | Đưa cursor A/library-1 cho B/library-2 hoặc user khác | Từ chối không tiết lộ existence/content library; không cursor progress/mutation. |
| `SYNC-032` Cursor bị sửa/không rõ version | Flip bit/truncate/đổi version/epoch | Stable invalid/unsupported cursor error; không panic, fallback timestamp hay data leak. |
| `SYNC-033` Rollover epoch/rebaseline | Event administrative/protocol đổi journal epoch theo quy trình ghi | Cursor cũ bị từ chối rõ; authoritative snapshot trả checkpoint mới; client không thể gộp epoch. |
| `SYNC-034` Thiết bị revoked/paused | Revoke sau khi lấy cursor/upload, trước page/mutation kế | Request tương lai bị từ chối theo pause/revoke policy; không event/object access unauthorized. |
| `SYNC-035` Clock skew cực lớn | Clock A/B lệch nhiều ngày và client mtime đảo | Server commit sequence và base version quyết định hành vi; mtime chỉ được bảo toàn như metadata. |
| `SYNC-036` Network reorder/loss | Delay response mutation, fetch change page nơi khác, duplicate/reorder request | Fact idempotency/journal hội tụ; không phụ thuộc arrival order ngoài committed winner. |
| `SYNC-037` Server restart giữa upload | Restart API/worker sau part, khi verify, trước retry completion | Upload resume/status đúng; không partial file hiển thị; final commit tạo một event. |
| `SYNC-038` Server restart sau commit | Kill sau DB commit/trước response/outbox consumption | Retry trả result; worker sau đó xử lý at-least-once; một current version/change. |
| `SYNC-039` Disk full/checksum mismatch | Fail incoming conflict upload hoặc normal update trong staging/finalize | Current server head không đổi; không event cho content lỗi; client nhận retryable/terminal error chính xác. |
| `SYNC-040` PostgreSQL unavailable | Thử mutation/fetch change khi DB outage | Không mutation thành công mà không tracking; hành vi staged transport rõ; retry sau recovery hội tụ. |
| `SYNC-041` Object store unavailable | Yêu cầu content update/download và metadata-only operation | Không content commit tham chiếu byte thiếu; read error ổn định; chỉ metadata operation an toàn tường minh có thể commit/journal. |
| `SYNC-042` Authorization đổi trong backlog | Revoke share/device access sau khi event tồn tại nhưng trước fetch | Authorization hiện tại thắng; cursor không rò metadata/byte event cũ; grantee nhận removal/access state định nghĩa. |
| `SYNC-043` Tương thích event schema | Client cũ được hỗ trợ thấy projection event additive/mới | Nó apply, invalidate/re-fetch hoặc báo unsupported an toàn; không map unknown state thành delete/purge. |
| `SYNC-044` Nhiều library | Write nặng ở library X trong khi Y sync | Scope order/cursor độc lập; không event cross-library hay serialization global/data leak có thể tránh. |

### Bằng chứng concurrency journal

Test ép các interleaving database sau bằng barrier:

1. transaction T1 sửa metadata trước khi acquire library clock; T2 tới cùng
   clock; commit order và sequence được allocate vẫn thẳng hàng;
2. T1 allocate rồi rollback; sequence/head/cursor committed tiếp không thể làm
   client chờ hoặc skip fact vô hình;
3. một mutation phát nhiều event định nghĩa; boundary cursor không bao giờ tách
   atomic fact set theo cách tạo local state bất khả thi;
4. page query chạy khi mutation commit trên supplied cursor; returned cursor chỉ
   advance qua event thực được page contract bao phủ;
5. retention xóa event cũ khi fetch/rebaseline bắt đầu; nó trả complete page/
   snapshot hợp lệ hoặc cursor-expired tường minh, không partial silent success;
6. hai worker/task thử cleanup journal hoặc đổi epoch; một serialized result và
   audit fact thắng.

### Testing sync dựa trên model

Sinh sequence operation dài trên nhiều thiết bị:

- create file/directory, replace byte, rename, move, copy, trash, restore,
  eligibility purge, share/revoke, offline/online, pause/revoke thiết bị;
- drop/duplicate/delay request và page, crash trước/sau boundary durable local/
  server, advance retention và skew clock;
- chọn base revision, normalized name, parent và cursor valid/invalid.

Sau mỗi quiescent point, so server và client với reference model đơn giản. Mọi
client online được authorize phải hội tụ về hierarchy active/trash, current
version/hash, conflict tường minh và cursor/epoch. History cùng protected object
thỏa bất biến chung. Generator in và persist seed rồi shrink failure thành
regression fixture.

### Test sync client/nền tảng

Suite server là cần nhưng chưa đủ cho native client. Mỗi profile Windows,
macOS, Linux Desktop và Linux Server được tuyên bố phải test:

- normalization Unicode, case-only rename, reserved name, trailing dot/space,
  component dài, collision và encoding local xác định;
- policy symlink/reparse/junction, hard link, sparse file, hỗ trợ permission/
  xattr và file identity;
- watcher overflow/lost notification/coalescing cộng authoritative rescan;
- test watcher deterministic Prompt 38 cho create/modify/delete, paired
  rename/move, fallback ambiguous cho remove+create unpaired, suppression inbound,
  race user-edit-after-inbound, bounded queue overflow, restart scan recovery và
  loại trừ control-path;
- test native Linux `notify` cho live create cùng disposable-root rename/move,
  editor-style temp replace, delete và safety symlink/special-entry khi host có
  semantics đó; bằng chứng Windows là cross-target compile trừ khi chạy thật trên
  host Windows native;
- temp-write/hash/replace local nguyên tử, file locked/open, process kill và low
  disk/inode;
- transaction, corruption, backup/rebuild local state database và apply cursor;
- lưu credential Keychain/OS, redaction log/diagnostic và revoke;
- state capability files-on-demand khi hỗ trợ; local eviction không bao giờ
  thành server deletion.

Platform/distribution matrix còn test:

- clean install và reinstall discover storage identity đã giữ lại;
- service start/stop/drain, process crash, host reboot, sleep/wake, update một
  phần, preflight thất bại và recovery có giới hạn mà không reset data;
- storage candidate selection, capacity/inode, root thiếu/removable, path/
  junction safety, capability downgrade và Btrfs/WinBtrfs acceleration bị tắt;
- tạo pairing code sống ngắn, single-use, expiry, sai user/device, replay,
  concurrent claim, revoke và remediation rõ ràng;
- self-hosted LAN/direct và remote access do operator cấu hình, chẩn đoán TLS/
  proxy, không bắt buộc relay và hành vi offline/air-gapped;
- presentation health/error riêng cho user, administrator, developer, gồm
  stable code, correlation ID, remediation và redact secret/path;
- verify signed artifact, update gián đoạn, migration không tương thích, yêu
  cầu rollback/restore, uninstall chỉ application, xác nhận xóa data, machine
  migration plan/verify/resume và coexistence instance cũ.

Metadata/capability unsupported được báo; không âm thầm bỏ theo cách ngăn restore
về sau.

### Performance/soak sync

Đo, không bịa universal target:

- throughput mutation và lock wait theo active device trên một và nhiều library;
- tail latency listing/initial snapshot/change page theo shape directory/event;
- event/byte catch-up backlog, chi phí apply local client và memory;
- upload/download đồng thời cộng latency change-feed;
- tăng trưởng storage journal, thời gian retention/cleanup và tác động thiết bị
  inactive;
- soak sync random 24 giờ hoặc lâu hơn cùng restart/network fault, kiểm tra hội
  tụ và drift object/reference định kỳ.

Công bố environment và regression budget được chấp thuận. Kết quả nhanh hơn
không thể đánh đổi bất biến commit-order, checksum, conflict hay authorization.

## Contract test backup và restore

Testing backup cố ý tách khỏi sync. Tái sử dụng sync test rồi gọi hành vi upload-
only là “backup” không chứng minh retained history.

### Bất biến backup

1. Snapshot `BUILDING`, `VERIFYING`, `FAILED` hoặc expiry không được cung cấp để
   restore. Chỉ manifest `COMMITTED` nguyên tử là restore anchor.
2. Mỗi committed entry tham chiếu immutable object đã verify hoặc ghi rõ kết quả
   non-content được manifest contract cho phép.
3. Source missing/deleted/unreadable/excluded ảnh hưởng observation mới; không
   bao giờ mutate retained snapshot cũ hay phát live `Node` deletion.
4. Submission/idempotency backup lặp cho một snapshot outcome; tái dùng content
   unchanged không ghép lifecycle snapshot.
5. Consistency snapshot được gắn nhãn trung thực (`FILESYSTEM_CONSISTENT`,
   `CRASH_CONSISTENT` hoặc `BEST_EFFORT`) từ bằng chứng client.
6. Retention trước tiên chọn complete snapshot theo policy/hold, rồi GC xét mọi
   entry/reference còn lại. Nó không thể xóa content cần cho retained snapshot
   hay active restore.
7. Restore mặc định tới destination mới/không phá hủy. Overwrite đòi collision
   policy và precondition tường minh.
8. Kết quả restore thành công verify membership manifest, required entry count/
   skip policy, length và canonical hash sau write.
9. Thiết bị/credential nguồn mất không ngăn owner được authorize restore retained
   server backup qua device/session mới.
10. UI/status backup phân biệt last attempt, last complete snapshot, consistency,
    age, error và restore verification.

### Ma trận scenario snapshot và retention

| ID | Setup và operation | Kết quả bắt buộc |
|---|---|---|
| `BACKUP-001` Backup đầy đủ ban đầu | Chọn source nested có file empty, small, large; upload/build/verify/commit | Một snapshot `COMMITTED`; hierarchy/count/hash/byte manifest chính xác; mọi object verify; hiện consistency label. |
| `BACKUP-002` Source rỗng | Commit selected folder rỗng hợp lệ | Root manifest rỗng có thể restore, không phải ambiguity failed/missing-source. |
| `BACKUP-003` Backup lặp không thay đổi | Chạy lại với stable identity/content giống hệt | Snapshot mới theo policy tái dùng object/entry đã verify; logical history riêng; accounting/physical use đúng. |
| `BACKUP-004` Xóa file local | Xóa một source file sau snapshot 1; tạo snapshot 2 | Snapshot 1 vẫn restore file; snapshot 2 ghi absence theo ngữ nghĩa manifest; không phát live sync deletion. |
| `BACKUP-005` Xóa folder local | Xóa populated directory giữa snapshot | Subtree cũ còn nguyên theo retention; omission snapshot mới có giới hạn/tường minh, không phải recursive server purge. |
| `BACKUP-006` Rename/move | Rename/move source không đổi byte | Manifest mới phản ánh relative hierarchy và có thể tái dùng object; path snapshot cũ không đổi; không rewrite object. |
| `BACKUP-007` File nhỏ đã đổi | Modify giữa snapshot | Entry mới tham chiếu object/version mới đã verify; byte cũ còn restore được. |
| `BACKUP-008` File lớn đã đổi | Modify đầu/giữa/cuối input lớn | Ban đầu whole-object path upload/verify toàn bộ byte mới; transfer gián đoạn resume; snapshot cũ an toàn. Chunk tương lai là gate riêng. |
| `BACKUP-009` File đổi trong lúc đọc | Mutate/truncate/replace trong lúc scanner read | Client phát hiện đổi identity/size/mtime/hash và retry hoặc ghi failure/`BEST_EFFORT` trung thực; không gắn nhãn mixed byte đã verify. |
| `BACKUP-010` Tree đổi trong lúc scan | Rename/delete/create qua scan phân trang | Snapshot dùng consistency model công bố và error tường minh; không bịa claim `FILESYSTEM_CONSISTENT`. |
| `BACKUP-011` File không đọc được | Permission/open error cho một file được chọn | Entry/result và completeness snapshot theo policy ghi; prior copy retained; UI báo path an toàn; không silent success. |
| `BACKUP-012` Policy exclusion | Include/exclude rule file type/path, rồi đổi policy | Manifest ghi policy/version và exclusion mong đợi; dữ liệu excluded không upload; retained data cũ theo retention, không delete ngay. |
| `BACKUP-013` Source symlink/reparse | Include link tới trong/ngoài source và loop | Policy ghi/skip metadata link không traverse ra ngoài hay loop; restore không thoát destination. |
| `BACKUP-014` Hard link/sparse/special file | Entry source riêng nền tảng | Hành vi được hỗ trợ và verify hoặc ghi rõ unsupported/metadata-limited; không hứa complete restore sai. |
| `BACKUP-015` Scan bị gián đoạn trước upload | Kill client giữa enumeration | Không partial snapshot committed; restart/retry có giới hạn; state staging/BUILDING được reclaim về sau. |
| `BACKUP-016` Upload bị gián đoạn | Kill sau một số object/part mới | Resume tái dùng verified part; không partial manifest committed; snapshot trước không ảnh hưởng. |
| `BACKUP-017` Server crash trong verify/commit | Kill tại object finalize, manifest verify, DB commit và response | Chính xác một committed snapshot hoặc explicit noncommitted state; orphan/lease reconcile; retry trả stored outcome sau commit. |
| `BACKUP-018` Submission snapshot trùng | Mutation identity backup đồng thời/lặp | Một outcome/manifest và repeated response định nghĩa; không duplicate retention anchor/accounting. |
| `BACKUP-019` Cạn quota | Cạn quota trong content và tại commit manifest | Snapshot không commit với reference thiếu; prior snapshot/restore hoạt động; error/status chỉ remediation. |
| `BACKUP-020` Hết disk/inode | Fail staging/finalization/server manifest work | Không partial snapshot committed; headroom recovery/read/restore còn; retry sau capacity hoạt động. |
| `BACKUP-021` Stored object hỏng | Bit-flip entry object trước verify/restore | Snapshot/entry được báo bị ảnh hưởng; byte không trả như hợp lệ; chỉ recovery verified redundant/system backup. |
| `BACKUP-022` Thiết bị nguồn paused/revoked | Revoke trước/trong scan/upload và sau commit | Submission tương lai unauthorized dừng; retained snapshot đã commit vẫn owner-restorable; không tuyên bố OS wipe. |
| `BACKUP-023` Thiết bị bị loại/mất | Delete/retire registration device theo policy | Ownership/restore retained snapshot còn; không cần credential riêng thiết bị cho authorized recovery; deletion policy rõ. |
| `BACKUP-024` Chọn retention | Seed tuổi hourly/daily/monthly và advance clock | Exact policy/hold chọn snapshot mong đợi xác định; timezone/DST không đổi UTC retention bất ngờ. |
| `BACKUP-025` Retention với object dùng chung | Snapshot/live version dùng chung object bằng nhau | Expire một reference không xóa byte cần bởi cái khác; accounting logical/physical đối soát. |
| `BACKUP-026` Retention so với restore | Barrier active restore lease khi retention/GC chạy | Object được bảo vệ tới khi verified restore/lease completion; retry an toàn. |
| `BACKUP-027` Retention crash/retry | Kill sau mark snapshot, trước/sau removal reference/sweep | Restart resume idempotent; không retained snapshot mất entry; audit/status nhất quán. |
| `BACKUP-028` Hold pháp lý/administrative | Apply/remove hold cùng authorization | Snapshot held bị loại khỏi retention; action audited; removal không bypass grace/verification tức thì. |
| `BACKUP-029` Drift reconciliation | Remove/add storage object hoặc reference trong fixture biệt lập | Auditor báo missing/orphan/uncertain chính xác; không auto-delete uncertainty hay rewrite manifest xanh. |
| `BACKUP-030` Nhiều file nhỏ | Snapshot entry count rất lớn theo page/batch có giới hạn | Size memory/transaction/manifest có giới hạn; progress restart ổn định; count/root hash đúng. |

### Ma trận scenario restore

| ID | Setup và operation | Kết quả bắt buộc |
|---|---|---|
| `RESTORE-001` Một file tới path mới | Chọn retained entry và destination rỗng | Byte/length/hash đã verify chính xác và metadata portable; báo success bền vững. |
| `RESTORE-002` Subtree directory | Restore subtree nested có file empty và object lớn | Hierarchy/required count chính xác; batch có giới hạn; verification/report theo entry. |
| `RESTORE-003` Snapshot đầy đủ tới thiết bị sạch | Không old device credential/cache; authenticate owner và restore | Mọi required entry restored/verified; skip/unsupported metadata rõ; chứng minh device-loss recovery. |
| `RESTORE-004` Version file trước | Restore immutable version được chọn vào live library | Current version/source attribution mới và journal event; old/current history không đổi; base precondition cưỡng chế. |
| `RESTORE-005` Collision destination: rename | File newer/different tồn tại; chọn keep-both | Portable conflict name xác định; không byte stream nào mất. |
| `RESTORE-006` Collision destination: skip | Chọn skip policy | Byte hiện có không chạm; skipped result rõ và có trong final summary. |
| `RESTORE-007` Collision destination: overwrite | Overwrite tường minh với base precondition | Chỉ destination matching được replace nguyên tử; stale precondition conflict; history overwritten retained khi áp dụng. |
| `RESTORE-008` Restore bị gián đoạn | Kill sau boundary entry/write/verify tùy ý | Operation restartable; verified completed entry tái dùng; partial temp byte không báo complete. |
| `RESTORE-009` Response trùng/mất | Retry cùng mutation restore sau commit/response loss | Một restore operation/outcome mỗi entry; không duplicate version/file. |
| `RESTORE-010` Source object hỏng/thiếu | Restore gồm entry bị ảnh hưởng | Fail affected result rõ/quarantine; không write destination hỏng hay đánh dấu toàn restore success. |
| `RESTORE-011` Hết quota/disk tại destination | Capacity hết giữa restore | Partial report và restart an toàn; verified prior output nguyên; temp partial cleaned; không fallback collision phá hủy. |
| `RESTORE-012` Tên manifest độc hại | `..`, separator, absolute/reserved/Unicode collision và symlink entry | Server/client map/từ chối an toàn theo portable policy; không thoát selected destination. |
| `RESTORE-013` Permission/metadata không hỗ trợ | Restore metadata POSIX sang Windows hoặc ngược lại | Byte vẫn đúng; metadata unsupported được báo, không silent claim hay escalate. |
| `RESTORE-014` Restore so với retention/GC | Start restore, expire source snapshot đồng thời | Lease/reference serialize an toàn; restore complete hoặc cancellation rõ trước deletion. |
| `RESTORE-015` Authorization/revocation | Credential share/read-only/device thử restore; revoke giữa plan | Mutation unauthorized bị từ chối; operation owner theo boundary initiation/recheck ghi; không object leak. |
| `RESTORE-016` Verification sau restore | Hash/read độc lập mọi required output hoặc deterministic sample policy | Final status success chỉ theo verification chỉ định; report có byte, entry, skip/conflict/error. |

### Testing backup property/model

Sinh sequence create/modify/rename/delete/unreadable/exclude source, start/
interrupt/commit snapshot, retention/hold, revoke thiết bị, corruption object,
restore/interrupt và GC. So với model immutable-manifest đơn giản:

- mỗi retained committed snapshot dựng lại observation công bố;
- source absence không thể mutate manifest/live node trước;
- mọi object retained/active-restore còn protected;
- snapshot expired/non-held cuối cùng hết bảo vệ sau grace;
- output restore bằng byte manifest đã chọn hoặc có explicit accepted result cho
  mỗi required entry.

Seed shrink thành fixture vĩnh viễn. Dùng clock UTC xác định và gồm instant
boundary retention.

### Performance/soak backup

Benchmark shape source riêng: nhiều file cực nhỏ, dữ liệu home-directory hỗn
hợp, ít file rất lớn, tỷ lệ unchanged cao, churn cao và backend chậm. Ghi
throughput scan/hash/upload/manifest/commit/retention/restore, memory, operation
database/object, unique/logical byte, tuổi queue và failure. Soak lặp tạo/retain/
expire snapshot trong khi restore snapshot sample và chạy GC/invariant audit.

Không kết quả performance nào cho phép incomplete snapshot xuất hiện committed
hay âm thầm disable integrity verification sau restore.

## Test system-backup và disaster-recovery Synveil

Test `BackupSnapshot` sản phẩm không thay test system-backup server. Release lab
phải:

1. seed user, session/device, library/node/version/trash, share, upload, cursor
   sync, backup snapshot, job/audit và integration secret đã encrypt tùy chọn;
2. vào mutation fence được ghi và capture PostgreSQL, namespace committed object,
   configuration/storage identity, metadata release/migration và master key
   không thể thay qua phương pháp hỗ trợ;
3. restore tới path database/object mới trên host/project sạch biệt lập;
4. chạy invariant và authenticate bằng credential/quy trình recovery đã khôi phục;
5. hash/range-read version đại diện và boundary-size, list journal, restore old
   version cùng complete user backup, process/reconcile job và decrypt/rotate
   integration secret;
6. chứng minh DB thiếu, object set thiếu, storage identity sai, key sai, manifest
   corrupt và release unsupported fail rõ trước writer start;
7. ghi time, size và mọi transient bị loại (ví dụ staging incomplete), rồi test
   post-restore state được ghi.

Ít nhất một full clean restore là bằng chứng release. Exit code backup command
không có restore thì không phải.

## Suite test security

[SECURITY.md](SECURITY.md) sở hữu ma trận threat/control hoàn chỉnh. Bằng chứng
automated và manual gồm:

### Authentication và authorization

- enumeration username, brute force/backoff, resource/concurrency Argon2,
  bootstrap race, reuse recovery-code, fixation/replay rotation session, expiry,
  logout/reset password/session epoch và revoke thiết bị; failure browser
  refresh trước commit có thể retry, còn response loss sau commit cộng reuse sẽ
  đặt family thành `REVOKED` tất định, ghi `REFRESH_REPLAY_DETECTED` và yêu cầu
  login mới;
- redaction list session và revoke độc lập (gồm clear cookie current session);
  one-time display recovery set, response generation bị mất, race/expiry khi
  activate pending, race exchange cùng code nguyên tử, response loss exchange
  trước/sau commit, replace transaction không thể tiếp cận qua `PENDING` →
  `EXPIRED`, reservation expiry không consume code, reset code cuối, exchange
  bằng code set cũ rồi replace set, race common-lock giữa set activation và
  reset chỉ có một winner, reset one-use, canonical epoch invalidate mọi grant
  `WEB`/`API`/`DEVICE`, giữ device record/data và yêu cầu xác thực mới;
- enforcement least-scope/expiry API grant, identity chính xác
  `ApiGrant`/`Session`, response issue/rotation một lần,
  `one_time_secret_unavailable`, replace/expiry pending khi mất response,
  activation race, retire old generation, revoke Session-family duy nhất,
  replay và bằng chứng grant không khuếch đại access owner; cùng ma trận
  initial/rotation/activation bao phủ device credential, gồm activation chỉ bởi
  owner, từ chối pending-secret tự activate, generation cũ tồn tại tới
  activation, pause do reset password với family cũ vẫn `REVOKED`, re-enrollment
  fresh family `PENDING` rồi activation thành `ACTIVE`, từ chối hồi sinh device
  đã revoke tường minh và atomic device revoke;
- IDOR cross-user/cross-library/cross-object/cross-upload/cross-backup/cross-photo/
  cross-repository; ma trận share read-only/writable/inherited; hành vi
  administrator so với owner;
- isolation `NodeFavorite` theo user, mutation đối nghịch idempotent, hành vi
  Trash/restore/purge và bằng chứng favorite không cấp access hay giữ content;
  discovery share received/sent authorize lại mọi projection, ẩn ancestor
  không đọc được, list pending sent link an toàn cho owner và loại grant
  received revoked/expired giữa pagination;
- entropy/lưu verifier public share token, creation pending inert,
  lost-response/idempotency replay không lộ capability, activation tường minh
  recovery revoke-rồi-tạo-mới, chạy đua với revoke/expiry, limit guessing,
  expiry/password, revoke, byte/concurrency và error tương đương thông tin.

### Browser/API

- CSRF với token missing/wrong, cross-site origin và trusted same-origin; CORS
  chính xác, spoof proxy-header và hành vi host-header;
- corpus XSS trong filename/tag/error/text repository/AI; active SVG/HTML;
  content disposition/type/nosniff/CSP/frame/referrer header;
- corpus SQL injection cho filter/sort/search/ID và hành vi safe error/stack trace;
- bound request/header/body/JSON depth/count/range/pagination/idempotency và
  timeout client chậm.

### Storage và parser

- traversal/absolute/NUL/Unicode/separator/reserved name, race symlink/junction/
  reparse/hard-link, root identity và exact-key deletion;
- oversize, zip/decompression bomb, dimension/page/frame cực lớn, malformed
  image/video/PDF/archive, timeout/OOM/crash và isolation parser no-network;
- misuse direct object key/presigned URL, foreign reference, policy bucket/prefix,
  corruption/quarantine và presence/timing/accounting dedup cross-owner.

### SSRF, webhook, Git và AI

- alternate URL encoding, userinfo, scheme/port, redirect chain, DNS rebinding,
  loopback/link-local/cloud metadata/private address; positive case explicit
  private Forgejo allowlist;
- webhook signature missing/bad, timestamp old/future, replay/duplicate, payload
  oversized/misbound và reconciliation polling sau;
- argument/shell injection Git, tên/ref/submodule/LFS URL repository độc hại,
  process limit, redaction credential và từ chối overwrite restore;
- network capture chứng minh không egress AI/model/provider ở `DISABLED` và
  `LOCAL`; consent/withdrawal remote, exact provider, giảm payload, log, hành vi
  stale/delete/ACL/prompt-injection và provider-failure.

### Canary secret và telemetry

Seed password, session/device/share/recovery/provider/Git/database secret tổng
hợp unique cùng name/content nhạy cảm. Exercise success và failure, rồi scan
log, trace, metric, audit, HTTP error, diagnostic, crash output, container
inspect/history và outbound capture. Raw canary không được xuất hiện ngoài
authorized boundary. Audit phải chứa bằng chứng mờ đục an toàn cho security action.

## Test job/outbox

Với mỗi handler và schema version job:

- mutation commit cùng outbox, rollback không có nó và hoạt động khi worker absent;
- hai worker race claim; một lease generation thực thi tại một thời điểm;
- worker crash trước work, sau external/byte work, sau commit output và trước
  acknowledgement job; retry idempotent và gắn source-version;
- lease expire/renew/steal dưới clock xác định; stale generation không thể đánh
  dấu claim mới hơn complete;
- retryable so với terminal error, exponential backoff/jitter, max attempt và
  dead-letter; poison job không spin;
- optional class disabled vẫn queued/coalesced/cancelled theo policy không chặn
  required job; re-enable/replay an toàn;
- mismatch payload/version fail rõ; không panic deserialization hay accidental
  destructive default;
- metric queue age/attempt/dead-letter/heartbeat và restricted operator control
  chính xác, đồng thời audit action replay/pause.

## Test ảnh và media

- Hash upload/download/backup/restore original không đổi qua mọi derivative job
  và edit metadata.
- Orientation/timezone/location EXIF và metadata missing/malformed; ACL location
  cùng derivative đã strip khi share; không rewrite original.
- Dimension pixel, frame/page count cực lớn, truncated/polyglot/active SVG,
  HEIC/HEVC unsupported và video lớn; parser có giới hạn và hành vi no-thumbnail
  graceful.
- Job thumbnail/rendition duplicate, source stale, crash, corrupt output,
  purge/rebuild và storage accounting.
- Exact duplicate trong/ngoài dedup domain; false positive perceptual suggestion
  không bao giờ auto-delete.
- Role still/video/resource được nhóm kiểu live-photo, partial group upload,
  duplicate client import và restore.
- Xóa nguồn điện thoại dưới upload-only policy bảo toàn original server retained;
  mirror mode tương lai nào cũng có test tường minh riêng.
- Pagination timeline/album/favorite/search và authorization dưới thay đổi
  metadata/ACL/delete đồng thời.

## Test AI

- Chạy complete core regression suite không AI service/schema extension khi hỗ
  trợ, với AI disabled, unreachable, slow và trả error.
- Job output gắn exact source version/model/config; stale result không thể thành
  current sau update/delete/restore/thay đổi ACL.
- Kết quả OCR/embedding/tag có thể thay và gắn nhãn provenance; user tag không
  bị overwrite; output confidence/unsupported có giới hạn.
- Index query cưỡng chế authorization hiện tại kể cả khi authorization lúc index
  khác. Fixture cross-user/project và share revoked không trả snippet, name,
  distance hay tín hiệu existence dựa timing vượt bound chấp nhận.
- Delete/exclude/provider withdrawal dừng work mới và purge/rebuild derived state
  dưới lag ghi tài liệu; canonical content còn nguyên.
- Text document/repository giống prompt không thể invoke shell, network, SQL,
  storage/admin tool, disclose system secret hay đổi policy.
- Chất lượng model local/remote được đánh giá trên corpus có tài liệu, có license,
  privacy-safe. Threshold chất lượng riêng tính năng và không bao giờ thay
  deterministic metadata search.
- Download/hash/license model, resource exhaustion, rate limit/timeout provider,
  rotation credential và hành vi no-payload-log đều đạt.

## Test Forgejo và repository

Dùng compatibility matrix Forgejo được hỗ trợ cùng instance và repository biệt
lập bao phủ history empty/large, ref/name bất thường, submodule, LFS, release
artifact và permission.

- Inventory/pagination poll, rate/backoff, credential expiry và outage phơi state
  fresh/stale/error chính xác mà không lỗi core.
- Webhook chỉ là hint: event lost/duplicate/reordered/forged hội tụ sau polling.
- Backup manifest nói chính xác Git ref/reachable data, LFS, artifact và metadata
  được include/omit. Verify integrity Git bằng tool được hỗ trợ đã chọn và so hash
  LFS/artifact.
- Kill tại export/upload/commit manifest; repeat/response mất là idempotent;
  repository backup trước vẫn restore được.
- Restore vào destination mới rỗng, verify ref/object/LFS/artifact và mặc định
  từ chối target incompatible/unauthorized/occupied.
- Content repository không thể tạo arbitrary network fetch/subprocess argument,
  và credential Forgejo không bao giờ đi vào Git output/log/error.
- Forgejo absent/bad version không thể phá Files, sync, user backup hay read
  repository backup đã verify hiện có.

## End-to-end web và accessibility

Playwright hoặc real-browser suite tương đương chạy trên Caddy/API/PostgreSQL/
object storage cho:

- bootstrap/login/logout/recovery đầu tiên và hành vi secure cookie/CSRF;
- Personal / Home onboarding, storage picker/capacity explanation, pairing,
  health/error remediation bằng ngôn ngữ dễ hiểu, progressive disclosure và
  handoff Advanced / Server mà core flow không yêu cầu terminal;
- list/pagination/sort/filter Files, folder/create/move/rename, resumable upload
  drag/drop, progress/retry, range/download, bound multi-select/bulk;
- history/restore version, xác nhận restore/purge Trash và trạng thái response mất;
- Favorites cá nhân qua rename/Trash/restore và mất access; discovery Shared
  received/sent không cần biết trước ID; create/copy/expiry/password/revoke
  share và public view chưa xác thực;
- watermark/order pagination Recent dưới mutation đồng thời, lọc auth/Trash
  tức thì, tối thiểu shared path và bằng chứng read/download không tạo view-
  tracking state ẩn;
- list/pause/revoke device, sync conflict/activity/freshness;
- backup set/snapshot/consistency/error/restore và report clean-destination;
- page Photos/AI/Code tùy chọn trong state ready/degraded/disabled/stale.

Keyboard navigation, focus order/restoration, semantic label, contrast, screen-
reader announcement, reduced motion và progress transfer/error lớn là release
gate theo ADR-009. UI không được gắn nhãn snapshot pending/failed, index stale,
codec unsupported hay placeholder simulated là complete.

## Test deployment và health

Với mỗi profile Personal / Home, Advanced / Server hoặc developer được claim:

- native/guided hoặc operator install sạch, config validation/bootstrap và
  restart idempotent;
- chỉ port Caddy truy cập external; user/capability/mount/read-only root/private
  network container khớp policy; không Docker socket/secret image;
- hành vi TLS/redirect/host/proxy/header/streaming/range/cache Caddy;
- `/health/live`, `/health/ready`, restricted detail và worker heartbeat khi
  outage API/DB/object/worker/AI/Forgejo; không probe tốn kém hay info leak;
- exhaustion capacity reserve/inode, storage identity missing/wrong mount và
  failure permission secret/master-key;
- SIGTERM/drain/restart cùng active upload/download/job, mô phỏng host reboot và
  reconciliation startup;
- native service lifecycle trên Windows Service, launchd, systemd hoặc adapter
  đã khai báo; crash, reboot, sleep/wake, permission/elevation và update recovery;
- provision/discover/start/backup/restore/upgrade PostgreSQL managed khi profile
  claim điều đó, không có SQLite fallback contract;
- storage picker safety, capability probe, root thiếu/removable, boundary NAS/S3,
  accelerator filesystem tùy chọn và validation path migration;
- pairing expiry/replay/revoke một lần và state direct/LAN/remote access mà
  không bắt buộc hosted relay;
- signed artifact/compatibility preflight, backup đã verify trước migration rủi
  ro, uninstall chỉ application/reinstall discover data, machine migration/
  recovery có evidence inspect/plan/validate/execute/verify;
- cardinality/label metric, alert/runbook, rotation/disk use log và mặc định
  không telemetry egress;
- system backup phối hợp và clean restore như đặc tả trên.

Optional failure không làm core readiness false. Failure PostgreSQL/object bắt
buộc không bao giờ tạo content mutation thành công sai.

## Testing migration, release và upgrade

### Fixture version

Mỗi version đã phát hành được hỗ trợ đóng góp fixture bất biến chứa:

- dump/snapshot database và checksum/version migration;
- representation object và storage identity;
- schema configuration với secret reference đã redact;
- user/session/device/share, library/name, version/trash, journal/cursor, upload,
  backup/restore, job/audit và optional feature state;
- edge case được giới thiệu/sửa trong bản phát hành đó.

Không bao giờ dùng production user data làm fixture.

### Ma trận upgrade

Với mỗi path source → target được hỗ trợ:

1. restore source fixture và verify trên source release;
2. validate target preflight/config và tạo system backup được hỗ trợ;
3. apply target migration đúng một lần và thử migrator thứ hai đồng thời để
   chứng minh locking;
4. inject interruption tại mỗi migration/backfill nontransactional/resumable;
5. start target API/worker, chạy invariant cùng read/write/sync/backup/restore
   old/new đại diện;
6. verify object format cũ và version payload job/event vẫn đọc được hoặc đã
   hoàn tất migration readers-before-writers được ghi;
7. chỉ test binary rollback được ghi khi schema compatible; nếu không restore
   complete pre-upgrade backup và verify source release lần nữa;
8. từ chối unsupported skip upgrade, schema mới unknown, checksum migration đã
   phát hành bị đổi, object/backend identity thiếu và master key thiếu trước
   writer start.

Backfill lớn test progress/resume, lock/load có giới hạn và quy tắc mixed reader/
writer. Không test nào “sửa” upgrade bằng wipe database.

### Gate release-candidate

- Mọi suite static/unit/integration/conformance/security/E2E bắt buộc xanh trên
  matrix được hỗ trợ; không skip/flaky quarantine cần cho release.
- Full crash suite storage/sync/backup và ít nhất một randomized soak xanh.
- Clean install, system backup, independent clean restore và bằng chứng upgrade/
  rollback được hỗ trợ xanh.
- Performance regression được review trên môi trường dedicated so sánh được.
- Phát hiện image/source/checksum/SBOM/license/provenance và dependency đã review.
- Status, limitation, migration và runbook Anh/Việt thống nhất.
- Không phát hiện high-severity chưa giải quyết về data-loss, authorization,
  secret hay upgrade; critical exception không thể âm thầm waive.

## Phương pháp benchmark performance

Benchmark trả lời câu hỏi capacity, không phải marketing claim. Mỗi report ghi:

- commit/release, version compiler/runtime/dependency và exact configuration;
- CPU/core, memory, disk/filesystem/mount, database, network/TLS, object backend
  và state cache warm/cold;
- distribution dữ liệu: file zero/small/medium/large, depth/width directory,
  entropy/compressibility, event/backlog, churn backup và concurrency;
- warm-up, sample count/duration, error/retry, percentile/distribution, CPU/RSS/
  allocation, blocking thread, DB pool/query, byte disk/network và tăng trưởng
  queue/storage;
- so sánh baseline, xử lý statistical/noise và regression được chấp nhận.

Category benchmark bắt buộc:

- concurrent resumable upload và range/full download;
- memory mỗi active stream và độc lập với total file size;
- SHA-256, stored checksum, compression/decompression và lookup dedup;
- listing/search/move metadata và pagination directory lớn;
- latency mutation/change-feed/backlog theo library và multi-library;
- build/commit/retention snapshot và clean restore cho mix small-file/large-file;
- verification/reconciliation/GC object và storage migration;
- claim/throughput/fairness queue job và isolation AI/photo/Git tùy chọn;
- interaction initial/load web và initial scan/apply/hydration client khi hỗ trợ.

Ban đầu performance gate dùng baseline tái lập và bất biến bounded-resource.
Product SLO chỉ được đặt sau bằng chứng deployment-class. Disable checksum,
fsync, authorization, bảo toàn conflict, verification sau restore hay publish
durable job không phải tối ưu được chấp nhận.

## Cadence CI và ownership

`.github/workflows/ci.yml` cung cấp baseline portability Rust ban đầu. Job
quality chạy format và Clippy; các job check và test dùng Cargo native trên
`ubuntu-latest`, `windows-latest` và `macos-latest`. Các job này kiểm tra source
Rust hiện tại có thể giữ tính portable; tự chúng không tuyên bố Synveil product
deployment hay native installer đã được support trên hệ điều hành nào.

| Cadence | Scope tối thiểu | Owner/action khi failure |
|---|---|---|
| Local/pre-submit | Unit/property seed module đổi, static/type/lint, targeted real integration | Implementer sửa hoặc báo exact block; không hoàn tất kiểu “works on my machine” |
| Pull request | Static/unit workspace, integration Postgres/local adapter, check contract/migration/docs/security, E2E targeted | Component owner; contract owner review thay đổi public/schema |
| Main/merge | Full integration, Compose smoke, critical path browser, generated drift, invariant audit | Integration owner revert/fix trước merge phụ thuộc tiếp |
| Nightly | Extended fuzz/property, crash matrix, lab S3/MinIO/NAS khi hỗ trợ, randomized sync/backup soak, scan dependency/image | QA triage với seed/artifact lưu; regression data-safety chặn release branch |
| Release candidate | OS/backend/browser/client được hỗ trợ, full security/recovery/upgrade, performance, clean system restore, provenance artifact | Release owner và reviewer security/data-safety độc lập ký bằng chứng |
| Periodic | Soak dài, restore retained backup cũ, compatibility dependency/model/Forgejo, incident game day | Domain/operations owner cập nhật support matrix/runbook |

Flaky test là defect. Test quarantined bảo vệ bất biến storage, sync, backup,
auth hoặc migration chặn gate tới khi có bằng chứng xác định tương đương thay thế.
Lưu random seed lỗi, request ID, log sanitized, manifest database/object và exact
environment; không lưu user secret/data.

## Chất lượng coverage và mutation

Line coverage là diagnostic, không phải release oracle. State machine critical,
nhánh authorization, failure transaction và decoder error đòi bằng chứng branch/
transition. Mutation testing có thể được đưa vào pure domain policy để chứng minh
test fail khi điều kiện authorization, checksum, retention, cursor hay idempotency
bị đảo/xóa. Percentage repository-wide tùy ý không bao giờ thay coverage scenario
và invariant.

## Protocol regression bug

Mỗi defect data-loss, corruption, authorization, retry, migration hoặc privacy
thêm:

1. reproduction deterministic sanitized nhỏ nhất hoặc random seed đã lưu;
2. assertion chống invariant và public behavior bị vi phạm;
3. interleaving fault/concurrency nếu timing góp phần;
4. fixture upgrade/backward khi released data có thể chứa state;
5. logic recovery/detection cho installation đã bị ảnh hưởng;
6. cập nhật security/runbook/docs và link bằng chứng.

Code patch không có regression failing-before/passing-after là ngoại lệ và đòi
lý do viết rõ cùng phương pháp verification thay thế.

## Các mục `OPEN DECISION` về testing

### OPEN DECISION OD-T01: cơ chế fault-injection

- **Owner:** QA, Architecture, Storage, Database
- **Needed by:** `SV-G1-STORAGE`
- **Options:** failpoint trait/decorator; fault proxy/filesystem cấp process;
  named hook test-build; cách kết hợp
- **Recommendation:** kết hợp contract adapter và named crash hook test-build,
  được verify bằng process-kill test trên PostgreSQL/local storage thực. Không
  endpoint failpoint remote production.
- **Decision evidence:** coverage mọi commit boundary, determinism, isolation
  binary và maintenance burden.

### OPEN DECISION OD-T02: ngôn ngữ implementation reference sync model

- **Owner:** Sync, QA, Clients
- **Needed by:** công việc protocol fixture Phase 4
- **Options:** module pure Rust độc lập; fixture generator language-neutral cộng
  model ngôn ngữ thứ hai; tool model-checking cho bounded state
- **Recommendation:** model pure nhỏ cộng fixture JSON language-neutral và ít
  nhất một consumer được triển khai độc lập; thêm formal/model checker có giới
  hạn nếu tìm interleaving hiệu quả hơn.
- **Decision evidence:** độc lập với production code, shrinkability, tái sử dụng
  desktop/Apple và chi phí CI.

### OPEN DECISION OD-T03: fault lab filesystem/NAS được hỗ trợ

- **Owner:** Storage, DevOps, QA
- **Needed by:** claim production NAS đầu tiên
- **Options:** baseline local ext4/XFS; profile vendor NFS/SMB; lab faulted
  filesystem userspace
- **Recommendation:** profile filesystem/mount hẹp có tên cùng test power-loss/
  network/capability tái lập; không claim “NAS support” chung chung.
- **Decision evidence:** adapter conformance, fsync/rename/locking, outage,
  permission và hành vi name.

### OPEN DECISION OD-T04: môi trường performance regression

- **Owner:** QA, Release, DevOps
- **Needed by:** performance gate alpha đầu tiên
- **Options:** reference host dedicated; class self-hosted runner; manual release
  lab cùng statistical comparison
- **Recommendation:** một môi trường reference kiểm soát cộng script portable
  và config công bố; CI shared noisy cung cấp functional bound, không precise
  regression claim.
- **Decision evidence:** nghiên cứu variance, chi phí và reproducibility.

### OPEN DECISION OD-T05: real-OS và release-lab matrix

- **Owner:** QA, Release, Platform / Distribution
- **Needed by:** `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` và mọi claim hỗ trợ
  first-class host
- **Options:** runner Windows/macOS/Linux riêng; hardware lab với evidence thủ
  công được ghi nhận; gắn nhãn community/experimental cho tới khi host có
  coverage
- **Recommendation:** dùng runner tự động khi package/service behavior tái lập
  được và hardware lab nhỏ có khai báo cho filesystem, sleep/reboot, storage
  removable và migration; không suy ra parity host từ Linux Compose.
- **Decision evidence:** runner availability, kết quả filesystem/NAS capability,
  coverage installer/service/update/recovery, accessibility evidence và chi phí
  duy trì.

### OPEN DECISION OD-T06: boundary test package và installer

- **Owner:** Installer / Updater, Security, Release, QA
- **Needed by:** implementation Personal / Home và `SYNVEIL_DEPLOYMENT_READY`
- **Options:** chỉ package-level smoke; full install/upgrade/uninstall lab;
  signed artifact cộng isolated VM/snapshot matrix
- **Recommendation:** đòi signed artifact cùng test clean-host, upgrade-failure,
  uninstall-preservation, reinstall-discovery và migration trước khi gọi native
  package là supported.
- **Decision evidence:** elevation/IPC threat, rollback/restore behavior,
  service crash/reboot/sleep, key/data retention và clean-host result tái lập.

## Checklist exit chất lượng phase

Trước khi gate owner ghi roadmap token bất kỳ:

- case unit/integration/property/fuzz/crash/conformance/security/E2E bắt buộc cho
  phase xanh trên support matrix của nó;
- invariant đạt sau test, sau restart và sau cleanup/retention;
- không kết quả bắt buộc nào chỉ dựa vào mock hoặc manual happy path;
- bound memory/count/time/disk/network và phương pháp performance được ghi;
- log/audit/metric/health phơi bằng chứng failure an toàn, có thể hành động;
- hành vi install/upgrade/backup/restore cho data/schema/config mới được test;
- mọi test skipped/flaky có release disposition và không test nào bảo vệ
  release-blocking invariant;
- status và docs Anh/Việt mô tả chính xác support, degradation, residual risk và
  non-goal.

## Bằng chứng inbound desktop Prompt 36

Suite client-sync dùng file SQLite disposable và managed root dưới temporary
directory của OS. Test không đụng root thật do user chọn. `SyncRemote`
deterministic in-process ép semantics rebaseline completion, checkpoint theo
device, feed/ack có giới hạn và download content logical theo chunk mà không cần
internet ngoài.

Coverage trực tiếp hiện gồm:

- migration từ zero, schema version, reopen, pragma WAL/FULL/foreign-key/
  busy-timeout, reject foreign-key/check, restart pending ack/operation/
  bootstrap, scope theo library và chặn writer thứ hai bằng exclusive lock;
- relative path strict, name Windows reserved/control/trailing/separator,
  Unicode/case collision key, cập nhật path hậu duệ Unicode, root marker cùng
  wrong scope/root binding và reject symlink redirect Linux;
- bootstrap root-only và multi-page, manifest page bền, chứng minh topology
  terminal, materialize parent-first, retry mất response, recovery local-
  complete, sweep generation tracked và giữ unknown file;
- feed page rỗng, single-event, multi-event, đủ tám journal kind hiện tại,
  transition applied/acknowledged chính xác, độc lập cross-device, wrong epoch,
  sequence gap, unknown kind, replay page/event và offline state;
- replay create/rename/move/Trash/restore/purge directory, collision case managed
  lẫn unknown, destination occupied, source missing, quarantine stale, file
  divergence khi replace/purge và attribution unknown-directory trong race;
- download multi-chunk tuần tự, mismatch length/SHA-256, giữ file visible cũ,
  restart sau stage, recovery replace trước database và cleanup receipt cuối; và
- failure deterministic sau page intent, trước filesystem action, sau staged
  content, sau filesystem receipt nhưng trước SQLite state, sau local event
  commit, sau server ack, sau bootstrap page, giữa bootstrap apply, sau local
  bootstrap complete và sau server completion trước local handoff.

Command local có thẩm quyền là:

```text
cargo test -p synveil-client-sync --all-targets --locked
cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings
```

Handoff release còn rerun toàn bộ locked workspace gate và integration case
PostgreSQL ignored Prompt 31-35 hiện có trên database disposable mới. Linux cung
cấp filesystem runtime evidence cho Prompt 36. Không được suy diễn native
Windows execution, power-loss test hay hostile same-user path-race test từ
Linux; chúng vẫn là gate platform/release-lab sau. Lệnh locked `cargo check
-p synveil-client-sync --target x86_64-pc-windows-gnu` với official Rust 1.98
toolchain khớp đã pass, nên Rust path conditional Windows đã được compile-
audit; check này không claim link hay runtime Windows native.

## Bằng chứng remote desktop Prompt 37

[`PROMPT37_CONTINUATION_AUDIT.md`](../PROMPT37_CONTINUATION_AUDIT.md) ghi riêng
worktree dirty kế thừa, phân loại requirement, lỗi có bằng chứng và kết quả cuối.
Kết quả test cũ không thay thế việc rerun source tree cuối.

### Coverage client, protocol và credential

Suite client-sync Linux có 55 unit test (23 HTTP adapter, 21 profile/SecretStore,
11 local-state/path hiện có) cùng 12 inbound integration test. HTTP test dùng
fixture Axum/TCP/TLS local thật và adapter reqwest production; policy HTTP
numeric-loopback explicit không làm yếu constructor HTTPS production. Coverage:

- method, route, query, DTO, bearer header, scope và evidence chính xác cho đủ
  bảy method `SyncRemote`; ID/decimal canonical, unknown field/kind, epoch/
  sequence/generation, Node revision zero, revision drift, tombstone và proof
  terminal rebaseline;
- giới hạn JSON/error khi có hoặc không có `Content-Length`, body JSON lỗi/HTML,
  content encoding nén hoặc lặp, typed HTTP error, deadline connect/header/
  metadata/idle/total, không auto-retry, tối đa 500 feed event hoặc 1.000
  manifest Node cho mỗi request;
- chunk streaming có giới hạn, immutable version/validator chính xác, mismatch
  length/SHA-256, EOF sớm, byte thừa, deadline độc lập với idle progress; không
  buffer toàn bộ file response;
- fixture redirect hai server với destination nhận zero request, không bearer/
  cookie; sai profile/owner/Device bị chặn trước network; fixture TLS self-signed
  tạm thời bị reject trước khi gửi HTTP authentication;
- normalize/reject URL strict, profile và replica/root binding bền, migration
  từ SQLite schema đầu, secure-store envelope có version, isolation khi copy/
  reconstruct SQLite, scan database/WAL/SHM tìm plaintext secret synthetic;
- lỗi store/read-back/delete, restart sau cleanup intent, replacement và forget
  explicit, secure storage unavailable không fallback plaintext, store đồng
  thời, engine đang sống bị chặn sau forget hoặc re-enroll; và
- bảo toàn managed byte, bootstrap state, sequence applied/acknowledged, pending
  ACK evidence và local issue khi offline/auth/revocation failure.

Core test kiểm tra syntax secret 256 bit có version, digest vector khác domain,
ID canonical, Debug redact và parse có giới hạn. API test kiểm tra tách principal
browser/device, CSRF, duplicate/mixed auth, route giới hạn, enrollment JSON
strict, error và tracing không lộ secret. Test trace chạy trong process test mới
để tracing callsite cache của test song song không làm mất capture. Test bắt
buộc quan sát trace event thật và không được lộ secret, kể cả bearer/grant bị
copy vào `X-Request-Id` hoặc URI không match route.

### PostgreSQL mới và acceptance HTTP thật

Có 27 PostgreSQL case ignored. Mỗi case phải chạy trên database disposable vừa
tạo riêng, được test apply toàn bộ forward migration. Dùng
`SYNVEIL_TEST_DATABASE_URL`, không `DATABASE_URL`; tái sử dụng database làm sai
giả định single-owner/count trong fixture. Continuation dùng container
PostgreSQL 17 mới, chỉ bind loopback, giữ nguyên container của session bị ngắt.

| Package / integration target | Số case |
|---|---:|
| `synveil-metadata` / `postgres` | 14 |
| `synveil-auth` / `postgres` | 2 |
| `synveil-storage` / `gc` | 5 |
| `synveil-metadata` / `device_credentials_postgres` | 1 |
| `synveil-auth` / `device_credentials_postgres` | 3 |
| `synveil-auth` / `bootstrap_precision_postgres` | 1 |
| `synveil-api` / `desktop_remote` | 1 |

Lấy tên case bằng `cargo test -p <package> --test <target> --locked -- --list
--ignored`, rồi dùng database mới cho từng lần gọi:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_test_database \
  cargo test -p <package> --test <target> --locked -- <exact_test_name> \
  --ignored --exact --test-threads=1
```

Credential case kiểm tra schema constraint, chỉ lưu digest, sai owner/Device,
owner/Device inactive, revoke một/toàn bộ credential, concurrent exchange chỉ
một winner, reject replay khi mất response và recovery bằng grant mới. Test
lock-expiry giữ Device row lock tới sau expiry; production exchange đang chờ
phải giữ Device PENDING, không tạo credential hay consumption record. Timestamp
trước khi chờ lock không đủ để kiểm tra expiry.

Acceptance dùng router thật, PostgreSQL mới, logical storage/upload service
thật, `HttpSyncRemote` production, SQLite mới và managed root tạm. Chỉ platform
SecretStore là implementation test explicit. Test bootstrap owner/session
browser, tạo và exchange grant, download file 192 KiB, bootstrap replica,
consume/ACK browser mutation, replace content 128 KiB, rồi inject failure sau
local apply trước ACK. Test revoke credential, xác nhận mọi route sync/
rebaseline/content từ chối, giữ byte và pending progress, re-enroll explicit
cùng Device và recover ACK. Cùng router thật còn chặn sai owner/library/Device/
content, thiếu browser CSRF, mixed auth, và device gọi upload/mutation/conflict/
admin route.

### Secure storage native và bằng chứng Windows

Persistence Linux native là test ignored riêng, không suy ra từ test store.
Chạy trong `dbus-run-session` riêng với `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
`XDG_RUNTIME_DIR` tạm mới và vault GNOME Secret Service synthetic đã unlock.
Không trỏ fixture tới keyring cá nhân. Chờ `org.freedesktop.DBus.NameHasOwner`
cho `org.freedesktop.secrets`; introspection có thể vô tình auto-activate daemon
thứ hai trước khi daemon của fixture sẵn sàng. Password unlock synthetic đi qua
stdin, không argv hay file.

```text
cargo test -p synveil-platform --lib --locked -- \
  native_secrets::tests::native_secret_survives_backend_recreation_and_is_deleted \
  --ignored --exact
```

Test ghi key opaque mới, mở process mới để đọc entry qua native adapter, sau đó
xóa và verify không còn entry. Child chỉ nhận tên key không bí mật; secret không
đi trong argument hay environment. Sau test chỉ dừng daemon/session do fixture
tạo.

Với official Rust 1.98 host/Windows standard library khớp nhau, MinGW GCC,
header, library và binutils, đặt `RUSTC`, `CARGO_TARGET_DIR`,
`CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER`, `CC_x86_64_pc_windows_gnu` và
`AR_x86_64_pc_windows_gnu` tới toolchain cô lập, rồi chạy:

```text
cargo check --workspace --all-targets --locked --target x86_64-pc-windows-gnu
cargo test -p synveil-client-sync -p synveil-platform --all-targets --locked \
  --target x86_64-pc-windows-gnu --no-run
```

Đây là workspace check và link Windows test executable thật, gồm bundled
SQLite và adapter Windows Credential Manager. Không dùng workaround pkg-config
trỏ vào host SQLite để chỉ type-check. Không claim runtime Windows native,
Credential Manager persistence, TLS, NTFS, reboot hay power-loss; đây vẫn là
gate platform/release-lab sau.

### Validation cuối và dependency policy

Sau code change cuối, rerun các case database/native mới ở trên cùng:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p synveil-client-sync --all-targets --locked
cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings
cargo deny check
npx --yes @redocly/cli lint api/openapi.yaml
git diff --check
```

License TLS mới đã review được giới hạn bằng exception đúng version trong
`deny.toml`: ISC cho `ring@0.17.14`, `rustls-webpki@0.103.15`,
`untrusted@0.9.0`, CDLA-Permissive-2.0 cho `webpki-roots@1.0.9`. Global allowlist
giữ nguyên; upgrade phải review lại. Bản phân phối phải giữ license notice và
root-data agreement tương ứng từ upstream. Không suppress advisory. Warning
OpenAPI lint về 4xx của public probe/status và shared component chưa dùng được
báo riêng, không sửa bằng cách bịa hành vi API. DeviceBearer không được quảng bá
cho system health có đặc quyền.

### Bằng chứng xung đột Prompt 88

Target `sync_conflict_postgres` của `synveil-api` bị ignore trừ khi
`SYNVEIL_TEST_DATABASE_URL` trỏ đến PostgreSQL 17 disposable mới. Target dùng
migration thật (vẫn 36), Axum router, hai device bearer đã enroll,
`HttpSyncRemote`, filesystem replica, SQLite store, precondition mutation
metadata và đường resumable upload. Test tạo phân kỳ rename, move, remote-trash
và content thật,
kiểm tra conflict typed bền vững và không tự gửi lại, cho inbound tiến lên trong
khi giữ byte cục bộ staged đã xác minh hash, đồng thời chạy `AcceptRemote` tường
minh và content `RetryLocal` idempotent.

Test client `conflict_policy` bao phủ phân loại chuẩn, durability qua restart,
resolution atomic/idempotent, thiếu nguồn content, concurrent duplicate
detection và migration V5 sang V6 thật vẫn giữ outbound upload, rebaseline
candidate và pending-handoff. Test state phân trang 2.000 conflict record mà
không tải toàn bộ lịch sử. Test rebaseline chứng minh chỉ bằng chứng node/parent
cũ chính xác mới tạo proactive conflict và intent gốc giữ nguyên từng trường.

Chạy target live với một database mới và một thread:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_p88 \
  cargo test -p synveil-api --test sync_conflict_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
```

### Kiểm chứng đối kháng Prompt 89

Target `sync_adversarial_postgres` (`synveil-api`, bị ignore trừ khi
`SYNVEIL_TEST_DATABASE_URL` trỏ đến PostgreSQL 17 disposable mới) là cổng
đối kháng cuối cùng cho Prompt 81–88. Target dùng PostgreSQL thật, Axum router
thật, device auth thật, `HttpSyncRemote` thật, SQLite client thật, retention
service thật, rebaseline coordinator thật, outbound submission thật và
conflict persistence/resolution thật. Không fake chuyển đổi đúng đắn trung tâm nào.

Topology: một owner, một library cho mỗi nhóm scenario, hai device hoạt động
(A chính, B gây nhiễu từ xa), cộng device thứ ba khi cần mutation độc lập.
State của client A dùng persistence thật (mirror, cursor, outbound intent,
upload/source, conflict, candidate/handoff).

Tám test live bao phủ ADV89-01..72, scenario 1–45 và chuỗi chaos 25 bước:
happy-path, concurrent feed, stale recovery, mutation trong transfer,
retention-vs-proof, payload-cleaned handoff, proof-loss S1→S2,
checkpoint-ahead, crash/restart, handoff mất response, rename/content conflict,
conflict×rebaseline, AcceptRemote/RetryLocal, 429/500/revoked, journal 25k sự
kiện bounded, snapshot ~200 entry, ledger 200 conflict phân trang bounded,
3 vòng rebaseline không leak, hội tụ hai device, stress 10 vòng live-PG + 100
vòng SQLite, failure-injection rollback xác định, invariant STATE-01..15,
đơn điệu checkpoint/floor/head, 40P01=0, không retry loop, không sleep.

Cổng local đầy đủ:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5439/postgres \
  cargo test -p synveil-api --test sync_adversarial_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
```

CI chạy cùng target dưới dạng subset bounded hard-fail với `set -euo pipefail`,
không `continue-on-error` hay `|| true`. Yêu cầu PostgreSQL 17, server
migration vẫn 36, client schema vẫn 6, không migration/route/OpenAPI/daemon/
scheduler/polling/retry/global-lock mới.

### Cycle hai chiều bounded Prompt 91

Module focused `sync_cycle` chạy `BidirectionalSyncCycleRunner` production cùng
engine Prompt 87 và Prompt 88 hiện có. SC1–SC22 bao phủ idle, inbound-only,
outbound-only, progress kết hợp, overlap cùng node, conflict hiện có và conflict
mới, recovery retained-floor/proof-loss, precedence candidate/handoff,
authentication, transport failure, snapshot-create 429, concurrency cùng
Library, Library độc lập, response loss và bound một page/một submission.
Crash matrix còn bao phủ caller dừng sau inbound, conflict đã persist và local
transition `SERVER_APPLIED` sau khi server commit.

Chạy focused gate deterministic:

```text
cargo test -p synveil-client-sync --lib sync_cycle --locked -- --nocapture
cargo clippy -p synveil-client-sync --all-targets --locked -- -D warnings
```

Target ignored `sync_cycle_postgres` chạy runner qua Axum router thật, device
authentication, `HttpSyncRemote`, filesystem replica, SQLite state và loopback
counting proxy. Tám case Prompt 91 kiểm tra progress hai chiều healthy, inbound
conflict fence, retention recovery, proof-loss recovery, conflict hiện có vẫn
cho inbound, revoked-device fail-closed, một submission cho caller cùng Library
và progress độc lập giữa hai Library. Target dùng lại fixture Prompt 87, nên
một lần chạy đầy đủ báo cả hai nhóm nhưng mỗi case vẫn bounded và serial.

Chỉ chạy với PostgreSQL 17 disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_p91 \
  cargo test -p synveil-api --test sync_cycle_postgres --locked -- \
  --ignored --test-threads=1 --nocapture
```

Workflow PostgreSQL 17 chạy target này như hard-fail step với
`set -euo pipefail`, rồi chạy focused client-sync gate. Prompt 91 không thêm
server migration, route, operation OpenAPI, frontend persistence, deployment
unit, cycle table, scheduler, daemon, retry loop, polling loop hay global lock;
server vẫn migration 36 và client vẫn schema 6.

### Runtime đồng bộ chạy dài Prompt 92

Unit suite runtime dùng Tokio paused virtual clock và các
`SyncCycleExecutor` controlled. Suite kiểm tra lifecycle single-supervisor,
start/shutdown idempotent, register/unregister, initial schedule startup,
manual/local wake, coalescing wake storm, giữ wake trong cycle, tối đa một
cycle active mỗi Library, global concurrency, fairness round-robin, idle poll,
backoff transient tất định và cap, delay rate-limit, auth suspension/resume,
follow-up sau progress, outbound chỉ bị conflict block, delay
recovery-blocked, fault isolation và graceful drain cycle đang chạy. Production
executor type-erased chỉ ở boundary scheduler nhưng vẫn gọi Prompt 91.

Chạy deterministic gate:

```text
cargo test -p synveil-client-sync --lib runtime::tests --locked -- --nocapture
cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings
```

Target PostgreSQL bị ignore `sync_runtime_postgres` bọc runtime lên fixture
thật hiện có: PostgreSQL 17, Axum route, device authentication,
`HttpSyncRemote`, filesystem replica, SQLite state và runner Prompt 91
production. Acceptance case bao phủ startup qua registration explicit, inbound
và outbound progress thật, local wake cho cycle thứ hai, graceful shutdown và
restart từ durable state cũ. Target chỉ chạy khi
`SYNVEIL_TEST_DATABASE_URL` trỏ đến PostgreSQL 17 disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_p92 \
  cargo test -p synveil-api --test sync_runtime_postgres --locked -- \
  --ignored --test-threads=1 --nocapture live_pg17_runtime_
```

Workflow PostgreSQL tạo child database cô lập bằng `set -euo pipefail`, chạy
live runtime target như hard-fail rồi chạy focused runtime unit suite. Live
target là một bounded integration case, không phải bằng chứng mọi permutation
fault/network/auth đã chạy trên PostgreSQL; các scheduling permutation đó do
unit deterministic kiểm tra. Chỉ được phát readiness khi live target và toàn
bộ gate regression Prompt 81–91 cùng repository pass. Server phải giữ 36
migration và client schema 6.

### Tích hợp signal runtime theo thứ tự durable-change trước Prompt 93

Focused signal suite Prompt 93 kiểm tra thêm producer boundary bên cạnh
regression scheduler Prompt 92. Suite chứng minh outbound intent visible trước
khi wake callback có thể quan sát, writer theo Library đã release trước khi
notify, notifier stopped/drop không xóa intent, exact no-op và observation bị
suppression không wake, còn observation batch/rescan bounded chỉ phát một wake.
Suite cũng bao phủ credential usable commit trước `CredentialChanged`,
credential persistence fail và removal không wake, status stopped/unknown typed,
clone runtime cùng identity, manual scheduling, network/auth safety,
follow-up coalesce khi cycle active và global bound nhiều Library.

Lệnh focused chính:

```text
cargo test -p synveil-client-sync --lib signals::tests --locked -- --nocapture
cargo test -p synveil-client-sync --lib durable_observation_batch_wakes_once_and_noop_does_not_wake --locked
cargo test -p synveil-client-sync --lib rescan_durable_work_emits_one_wake_after_bounded_reconciliation --locked
cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings
```

Live target `sync_runtime_signals_postgres` reuse fixture Prompt 87 thật và
thêm PostgreSQL 17, Axum, device authentication, `HttpSyncRemote`, SQLite,
cycle Prompt 91 production và runtime Prompt 92 để kiểm tra đủ mười case
LIVE-SIG: durable local intent wake, wake unavailable được periodic polling
khôi phục, observation submission và burst coalescing, credential resume,
network recovery, manual `sync_now`, follow-up coalescing khi cycle đang chạy,
restart recovery sau lost wake và isolation giữa nhiều Library. Target bị ignore
nếu `SYNVEIL_TEST_DATABASE_URL` chưa chỉ đến PostgreSQL 17 disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_p93 \
  cargo test -p synveil-api --test sync_runtime_signals_postgres --locked -- \
  --ignored --test-threads=1 --nocapture live_pg17_signal_
```

Workflow PostgreSQL chạy target này như hard-fail step Prompt 93, sau đó chạy
focused producer và observation tests. Target không claim có OS network
monitor, service manager, UI, server push channel hay persistent wake queue.
Live signal acceptance vẫn unverified khi chưa cấu hình disposable PostgreSQL
17 bắt buộc; evidence focused SQLite/virtual-clock không thay cho live gate.

Prompt 93 thêm zero server migration, zero client migration, zero route, zero
OpenAPI operation và zero persistent runtime table. Invariant là durable state
trước, writer release kế tiếp, wake best-effort sau đó; startup và periodic
polling vẫn là recovery correctness.

### Gate host desktop và lifecycle tiến trình Prompt 94

Focused module `host` trong `synveil-client-sync` kiểm tra composition root cấp
application mà không cần desktop executable hay server live. Module chứng minh
construction không có worker, identity của một runtime duy nhất giữa host,
handle, notifier, credential controller và observer, registration trước
observer start, duplicate registration/start, routing wake manual/network/
credential, status stopped typed, forwarding lifecycle adapter, HTTP startup
không enrollment, restart từ durable state, join/shutdown lặp lại an toàn và
stress 1.000 vòng start/shutdown/drop. Matrix focused cũng từ chối registration
khác owner/Device bằng `WrongScope` typed. Suite Prompt 91–93 hiện có vẫn sở
hữu correctness cycle, ordering durable signal, recovery candidate/handoff/
conflict và fairness scheduler.

Chạy gate composition local:

```text
cargo test -p synveil-client-sync --lib host --locked -- --nocapture
cargo check -p synveil-api --test desktop_sync_host_postgres --locked
cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings
```

Target ignored riêng `desktop_sync_host_postgres` chọn mười case
`live_pg17_host1` đến `live_pg17_host10`. Target dùng lại fixture hiện có và
exercise PostgreSQL 17, Axum router, device authentication, `HttpSyncRemote`,
SQLite, runner Prompt 91, runtime Prompt 92 và producer/controller Prompt 93
thật qua một `DesktopSyncHost`. Các case bao phủ startup có inbound + outbound,
missing credential `AuthBlocked` rồi replacement trên cùng host, observation
filesystem, manual sync, network recovery, graceful shutdown khi cycle active,
durable intent sau shutdown, process restart, ba Library chung một runtime và
100 lifecycle live lặp lại.

Chỉ chạy với PostgreSQL 17 disposable mới:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_p94 \
  cargo test -p synveil-api --test desktop_sync_host_postgres --locked -- \
  --ignored --test-threads=1 --nocapture live_pg17_host
```

Khi biến database chưa set, target live được ignore có chủ ý; kết quả là
unverified, không phải pass. Host không thêm server/client migration, route,
OpenAPI operation, frontend code, service unit, autostart hook hay UI. Linux và
Windows dùng chung host code cùng adapter semantics; chạy native Windows vẫn
là compile/CI concern nếu local không có Windows runner.

Lifecycle matrix của host cũng ghi nhận boundary crash/restart: crash trước
start không có cycle, wake mất vẫn để durable work cho startup polling, còn
recovery candidate/handoff/conflict/idempotency thuộc lower layer hiện có.
Status/event của host không là durability oracle và không chứa secret, cookie,
token, content hay raw local path.

### Gate process desktop production và root lifecycle Prompt 95

Package production là `synveil-client`. Unit suite của nó kiểm tra parser/
redaction manifest không bí mật, network interval bounded, lifecycle và
periodic network adapter, shutdown một process/một host, error category ổn
định và writer lock SQLite liền kề hiện có. Host test client-sync thêm
bootstrap khi root missing, fence root loss trước queued hint, reappearance
cùng binding, watcher restart một lần, rescan bounded và isolation sibling
healthy. Bốn target adversarial/deployment chỉ dành cho Linux vẫn có
`#![cfg(target_os = "linux")]` tường minh:

```text
crates/metadata/tests/linux_deployment_adversarial_units.rs
crates/metadata/tests/linux_install_lifecycle.rs
crates/metadata/tests/linux_native_packaging_units.rs
crates/api/tests/linux_deployment_adversarial_postgres.rs
```

Lệnh focused local:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test -p synveil-client --lib --locked
cargo test -p synveil-client-sync --lib host::tests --locked -- --nocapture
cargo test -p synveil-client-sync --lib runtime::tests --locked -- --nocapture
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo deny check
```

Target live ignored riêng
`crates/api/tests/desktop_process_postgres.rs` tạo child PostgreSQL mới,
Axum route thật, device authentication, test secret store bind profile,
`DesktopClientProcess` production, một `DesktopSyncHost`, filesystem replica,
SQLite V6 và HTTP sync transport thật. Target assert bootstrap process,
profile/root binding, schema V6, từ chối single-writer, feed activity thật,
runtime identity ổn định và graceful shutdown idempotent:

```text
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_p95 \
  cargo test -p synveil-api --test desktop_process_postgres --locked -- \
  --ignored --test-threads=1 --nocapture live_pg17_process
```

PostgreSQL workflow tạo child database cô lập và chạy target này như
hard-fail step Prompt 95, sau đó chạy process/host/runtime focused tests.
Target mặc định ignore và không được xem là pass khi
`SYNVEIL_TEST_DATABASE_URL` unset. Evidence integration PostgreSQL phải có
disposable PostgreSQL 17 thật; SQLite/unit không thay thế gate này.

Cross-platform check phải compile workspace trên Windows native và, trong
environment có target/linker, chạy target check explicit:

```text
cargo check --workspace --all-targets --locked --target x86_64-pc-windows-gnu
```

Trên Linux, binary smoke test chỉ được dùng
`SYNVEIL_CONFIG_DIR`/`SYNVEIL_DATA_DIR` temporary mới và `client.conf`
disposable; không trỏ vào profile, credential store hay Library của user.
Systemd/service, installer/package và migration guard vẫn là gate riêng.
Không có marker `SYNVEIL_DESKTOP_PROCESS_BOOTSTRAP_READY` hợp lệ cho
đến khi repository gate local, live PostgreSQL process target và
cross-platform deployment check đều pass thật.

## Gate IPC control cục bộ Prompt 96

Target Linux disposable tập trung:

```text
cargo test -p synveil-client --test desktop_control_ipc --locked
```

Target bind runtime root temporary ngắn và kiểm tra Unix socket type, owner,
control directory `0700`, socket `0600`, handshake v1, Ping/process status,
library listing bounded, request ID, unknown-command/version error,
malformed/oversized/truncated frame, reconnect, event subscription, thứ tự
Shutdown acknowledgement và remove endpoint sạch. Unit test còn kiểm tra active
endpoint collision, stale socket recovery an toàn và từ chối symlink/
regular-file/directory.

Target live Prompt 95 hiện kết nối production process bằng control client thật,
kiểm tra endpoint dùng được trước `Running`, serialize library status an toàn,
gửi `SyncNow` chỉ scheduling và kiểm tra socket bị remove sau graceful
shutdown. Chỉ chạy với PostgreSQL 17 disposable mới:

```text
cargo test -p synveil-api --test desktop_process_postgres --locked -- \
  --ignored --test-threads=1 --nocapture live_pg17_process
```

Windows native matrix và explicit `x86_64-pc-windows-gnu` phải compile nhánh
named-pipe security-descriptor thật. Evidence local Linux không tuyên bố native
Windows execution. Readiness marker đầy đủ là
`SYNVEIL_DESKTOP_CONTROL_IPC_READY`, chỉ hợp lệ sau khi tất cả gate Rust,
dependency-policy, web, OpenAPI, deployment, Windows và live regression pass
thật. PostgreSQL target bị ignore vì `SYNVEIL_TEST_DATABASE_URL` chưa set là
unverified, không phải pass.

## Gate core desktop controller Prompt 97

Focused unit suite của controller nằm trong package `synveil-client`, kiểm tra
construction không side effect, reconnect backoff deterministic có cap,
serialization/redaction latest-state, generation fence, validation timing và
fold 10.000 signal thành một pending refresh bit:

```text
cargo test -p synveil-client --lib controller::tests --locked -- --nocapture
cargo clippy -p synveil-client --all-targets --all-features --locked -- -D warnings
```

Target acceptance Linux disposable exercise Prompt 96 server và Unix-domain
socket thật. Target kiểm tra handshake v1 cho command/status/event connection,
một snapshot `Fresh` coherent, process state an toàn, map command unknown
Library, command disconnected bounded, giữ snapshot stale sau server loss,
automatic reconnect có generation mới và controller stop độc lập với process:

```text
cargo test -p synveil-client --test desktop_controller --locked -- --nocapture
```

Implementation có một manager task, một event-reader task cho mỗi generation
active, mỗi lần chỉ một refresh status, command channel tám item và `watch`
latest-state không có history nội bộ. Target PostgreSQL của process Prompt 95
hiện cũng embed một acceptance slice Prompt 97 cho process endpoint thật:
handshake, library projection an toàn, `SyncNow` chỉ qua IPC, redaction và
controller stop độc lập với process. Đây vẫn là target hard-fail và không được
coi là pass khi `SYNVEIL_TEST_DATABASE_URL` unset. Focused Linux target không
claim full chain PostgreSQL sync Prompt 91–96. Test Linux không claim native
Windows execution; named-pipe controller path vẫn phải qua gate compile
native/cross-target Windows bắt buộc.

Controller thêm zero server migration, zero client migration, zero HTTP route,
zero OpenAPI operation, zero web change và zero artifact GUI/tray,
autostart/service/installer. Readiness marker đầy đủ là
`SYNVEIL_DESKTOP_CONTROLLER_CORE_READY`, chỉ hợp lệ sau khi toàn bộ gate
repository, regression Prompt 81–96, live, Windows, dependency, web, OpenAPI
và deployment pass thật. Focused pass riêng không đủ cho claim đó.

## Gate shell desktop native Prompt 98

Gate UI Linux dedicated là script repository
[`scripts/test-desktop-ui.sh`](../../scripts/test-desktop-ui.sh). Script format,
test, lint và build riêng Qt target, sau đó tìm QML module metadata do Qt 6
generate, chạy `qmllint` Qt 6 trên hình dạng embedded module và load toàn bộ
application tree bằng smoke test offscreen:

```text
./scripts/test-desktop-ui.sh
```

Focused Rust suite kiểm tra projection Prompt 97 sang QML an toàn và semantics
UI1–UI40: disconnected/connecting/reconnecting/fresh, protocol/security
failure, category root/auth/conflict/runtime, stable Library identity, update
selection atomically, redaction, Sync Now chỉ qua controller, action từng
Library đủ điều kiện, feedback chỉ scheduling, rapid click bounded, state tray/model dùng chung, close-to-tray,
fallback không có tray, controller/process independence và Qt thread-affinity.
Stress case có 10.000 latest-state update, library churn và model 1.000
Library. Suite không được báo “everything synced” từ scheduling acknowledgement.

Linux CI cài development package Qt 6.4 hoặc mới hơn và chạy cùng hard-fail
script. Job Windows native pin Qt 6.8.3 MSVC thật trên `windows-latest` và
hard-fail:

```text
cargo test -p synveil-desktop --locked
cargo build -p synveil-desktop --locked
cargo build -p synveil-desktop --release --locked
```

Script smoke-load cả binary debug và release để kiểm tra embedded QML resource
path không phụ thuộc source tree ở cả hai build mode.

Workspace check/test cross-platform thông thường exclude Qt binary
platform-specific; Windows native job sở hữu Qt build thật. Việc tách này
không làm yếu named-pipe controller check trên Windows. Máy Linux không được
claim runtime/tray native Windows; evidence đó chỉ ghi nhận khi Windows job
authoritative chạy.

PostgreSQL chỉ cần cho acceptance live Sync Now/process-controller, không cần
cho QML model hoặc shell offscreen. Production-process target hiện có exercise
controller-to-process Sync Now path thật và chỉ chạy với PostgreSQL 17
disposable mới:

```text
cargo test -p synveil-api --test desktop_process_postgres --locked -- \
  --ignored --test-threads=1 --nocapture live_pg17_process
```

Prompt 98C bổ sung việc quan sát end-to-end `LIVE-UI5` còn thiếu vào target
QML Linux ignore `crates/api/tests/desktop_qml_postgres.rs`. Target chạy
`synveil-client` production thật và Qt shell thật ở hai process riêng, detach
rồi restore một managed root, và bắt buộc chuỗi
`Available -> Unavailable -> Recovering -> Available`. Marker `Recovering` chỉ
được QML phát sau khi quan sát `selected_root_label` canonical an toàn từ
bridge (`Checking folder changes`), với status `Fresh` và cùng controller
generation; sau đó test mới release
`DesktopRootRecoveryGate` hiện có. File handshake bên ngoài của gate chỉ được
compile trong client acceptance bật feature tường minh, không thêm production
delay, retry hay UI state.

Chỉ chạy target sau khi build client test-support và Qt shell, với PostgreSQL
17 mới và native Secret Service đã unlock:

```text
cargo build -p synveil-client --features test-support --locked
cargo build -p synveil-desktop --locked
SYNVEIL_TEST_DATABASE_URL=postgresql://postgres@127.0.0.1:5432/fresh_p98c \
  QT_QPA_PLATFORM=offscreen \
  cargo test -p synveil-api --test desktop_qml_postgres --locked -- \
  --ignored --test-threads=1 --nocapture live_pg17_qml_production_shell_acceptance
```

Target này báo riêng các state `Available`, `Unavailable` và
`Checking folder changes` mà QML nhìn thấy, tách khỏi acceptance controller/
process. Target còn kiểm tra một lần vào recovery gate, zero delete/trash
intent tổng hợp qua root/process regression hiện có, không duplicate runtime
hay controller generation và state cuối `Available`. Nếu QML target bị ignore
vì thiếu PostgreSQL, Secret Service, Qt hoặc client acceptance bật feature thì
đó là unverified, không phải pass.

Khi `SYNVEIL_TEST_DATABASE_URL` unset, PostgreSQL target bị ignore là
unverified. Không được in marker đầy đủ
`SYNVEIL_NATIVE_DESKTOP_SHELL_READY` nếu focused UI, toàn bộ gate
Rust/dependency/web/OpenAPI/deployment, evidence Qt native Linux/Windows,
regression live controller/Sync Now, ADR-040 và tài liệu EN/VI synchronized
chưa thật sự pass. Riêng focused local gate không đủ cho claim đó.

### Đóng gate build Qt Windows thật Prompt 98D

Blocker cuối của Prompt 98 được đóng vào 2026-09-16 bằng Path B, cross-build từ
Linux. Đây là bằng chứng Qt target cho Windows thật, không phải build Qt Linux:

- SDK Qt lấy từ official Qt online repository qua `aqtinstall` 3.3.0. Gói đã
  chọn là Qt 6.8.3 `win64_llvm_mingw` (Windows x86_64 GNU/UCRT), cài ngoài
  repository tại `/tmp/synveil-p98d-qt/6.8.3/llvm-mingw_64`. Base URL của gói
  official là
  `https://download.qt.io/online/qtsdkrepository/windows_x86/desktop/qt6_683/qt6_683/qt.qt6.683.win64_llvm_mingw/`;
  file được tải qua Qt mirror đã cấu hình. `Qt6Core.dll`, `Qt6Gui.dll`,
  `Qt6Widgets.dll`, `Qt6Qml.dll` và `Qt6QuickControls2.dll` đều được inspect
  là PE32+ Windows x86-64. Export của QtCore target có
  `qResourceFeatureZlib`, không phải shared library Linux.
- Compiler target tương thích ABI là official LLVM-MinGW release
  `llvm-mingw-20260602-ucrt-ubuntu-22.04-x86_64`, cài ngoài repository tại
  `/tmp/synveil-p98d-llvm/llvm-mingw-20260602-ucrt-ubuntu-22.04-x86_64`.
  Compiler là `x86_64-w64-mingw32-clang++`, Clang 22.1.7, target
  `x86_64-w64-windows-gnu`; SHA-256 archive là
  `9d191203f9768ead60662d3ae53cdf28e0a28b1e6d44b7f329b9202cb2add337`.
- Rust official cô lập là Rust 1.98.1 / Cargo 1.98.1, đã cài target
  `x86_64-pc-windows-gnu` dưới `RUSTUP_HOME=/tmp/synveil-p98d-rustup`.
  CMake 4.4.3 và Ninja 1.13.2 sẵn có. CXX-Qt/cxx-qt-build là 0.10.0. Build
  dùng qmake adapter bên ngoài, adapter báo spec Windows `win32-clang-g++`,
  Windows Qt prefix ở trên, cùng các generator `moc`, `rcc`, `qmlcachegen` Qt
  6.8.3 chạy trên Linux host. `CXX_QT_AUTORCC_OPTIONS=--no-zstd` chỉ là cài
  đặt cross-build bên ngoài, khớp feature của Qt target; resource generated
  dùng feature zlib mà target export. Linker/runtime compatibility wrapper
  cũng ở ngoài repository; không thêm workaround vào production source.

Các stage bắt buộc đã chạy riêng và đều pass:

| Stage | Bằng chứng |
| --- | --- |
| Lower stack Windows | `cargo check -p synveil-client-sync --all-targets --locked --target x86_64-pc-windows-gnu`, sau đó cùng command cho `synveil-platform` và `synveil-client`: tất cả pass. |
| CXX-Qt generation | Đã generate `cxxqtgen/src/bridge.cxx.cpp`, `bridge.cxxqt.cpp`, QML type registration, initializer, QML cache và RCC source trong Cargo target tree bên ngoài. |
| C++ generated | Bridge generated, source moc/QML-generated và `src/native/tray.cpp` thật compile bằng Clang target; `cd12d4f3968eceae-tray.o` được tạo ở cả hai profile. |
| Qt link | Debug/release link import library của Qt Core, Gui, Qml, QuickControls2 và Widgets target. `Qt6QuickControls2.dll` import `Qt6Quick.dll`; `Qt6Qml.dll` import `Qt6Network.dll`, nên Quick và Network dependency cần thiết vẫn ở trong Windows graph. |
| QML resources | Debug/release đều generate và link RCC của QML module. Output có `Main.qml` và `com/synveil/desktop`; executable có `qrc:/qt/qml/com/synveil/desktop/qml/Main.qml`, không phụ thuộc source-tree path khi runtime. Cả hai RCC output dùng `qResourceFeatureZlib` và không có call `qResourceFeatureZstd`. |
| Debug PE | PASS: `/mnt/Projects/synveil-p98d-target/x86_64-pc-windows-gnu/debug/synveil-desktop.exe`, `file` báo PE32+ x86-64; `llvm-readobj` báo `IMAGE_FILE_MACHINE_AMD64`. |
| Release PE | PASS: `/mnt/Projects/synveil-p98d-target/x86_64-pc-windows-gnu/release/synveil-desktop.exe`, `file` báo PE32+ x86-64; `llvm-readobj` báo `IMAGE_FILE_MACHINE_AMD64`. |

PE import audit thấy `Qt6Core.dll`, `Qt6Gui.dll`, `Qt6Widgets.dll`,
`Qt6Qml.dll`, `Qt6QuickControls2.dll`, `libc++.dll`, `libunwind.dll` và
Windows system/API-set DLL. Không executable hay captured link log nào có
`/usr/lib/libQt6*`, Linux Qt `.so` hoặc Linux host Qt prefix. Strings của
executable vẫn có symbol production thật `QSystemTrayIcon`, `QMenu` và
`QAction`. Windows dependency graph có `\\.\pipe\synveil-` và implementation
named-pipe của Tokio trên Windows, chứng minh đây không phải build chỉ có UDS
transport của Prompt 96.

Không cần sửa source Rust, C++, CXX-Qt hay QML của Synveil. Windows native
offscreen startup và native Windows tray interaction là **NOT RUN** vì Linux
host này không có Windows runner và không có Wine. Compile/link tray thật và
compile/link named-pipe vẫn PASS; không claim native runtime hay tray
interaction.

Bằng chứng Linux được chấp nhận của Prompt 98C vẫn còn hiệu lực vì không có
shared production source thay đổi: dedicated Linux desktop suite, Qt QML lint,
debug/release build và offscreen smoke, full Rust/dependency/web/OpenAPI/
deployment gate, PostgreSQL migration 36/36, client schema 6 và ADR-040
(`Accepted — LOCKED`) vẫn như đã ghi. Prompt 98D không thêm server migration,
client migration, route, OpenAPI operation, web, autostart, service, Windows
Service hay installer. Repository có zero SDK/compiler/build artifact Qt
Windows; toàn bộ toolchain tạm và PE file nằm ngoài repository.


## Gate launch desktop production Prompt 99

Prompt 99 validate orchestration launch như process-management boundary trên
controller Prompt 97 hiện có. Topology bắt buộc là
`synveil-desktop -> BackgroundClientManager -> DesktopController/Prompt 96 ->
synveil-client`; Qt process không sở hữu `DesktopSyncHost`, `SyncRuntime`,
SQLite, credential, root hay writer lock Prompt 95. Launch manager là layer duy
nhất được request start packaged client, và public result là category typed hữu
hạn thay vì OS diagnostic.

Focused suite trong `crates/client/src/launch.rs` cover LAUNCH1–12: construction
không side effect, reuse client đang chạy, absence chỉ một start request,
coalescing đồng thời, boundedness 1,000 request, security/protocol fail-closed,
launch failure typed, không stop do GUI, sibling canonical resolution, reject
PATH spoof và profile identity không thành shell argument. Suite cũng cover
autostart reversible và Windows task-definition policy deterministic.
`max_active_attempts` của manager phải luôn một và test không được start
Synveil client thật.

Platform policy gate cover AUTO1–12 và CRASH1–6:

- Linux unit là user unit tại `/usr/lib/systemd/user`, dùng đúng invocation
  `/usr/bin/synveil-client` được source hỗ trợ, `Type=simple`,
  `Restart=on-failure`, `RestartSec`/`StartLimit*` bounded và
  `RestartPreventExitStatus=78` derive từ source. Không có dependency
  network-online và không có system-unit path.
- Enable, disable, start, stop, status Linux dùng argv cố định
  `systemctl --user`. Package hook và GUI startup không tự enable. Test user
  manager disposable link unit tạm, verify load/start/active/stop/disable rồi
  cleanup link.
- Windows task definition chứng minh current-user `InteractiveToken`,
  `LeastPrivilege`, logon trigger, đúng sibling `synveil-client.exe`, restart
  finite, `IgnoreNew`, profile identity sanitized và không password/credential/
  SYSTEM/admin. Native registration là gate trên Windows runner, không phải
  claim của Linux.
- Crash policy bounded theo supervisor. Endpoint security, protocol mismatch,
  control malformed, writer conflict và terminal state không request launch
  replacement. GUI absent không disable user supervisor đã cấu hình.

Package gate cover PKG1–12. Linux build DEB thật và RPM hiện có từ cùng payload
stage `deploy/install/MANIFEST`, rồi audit archive content, path, mode, executable
byte parity, unit/metadata parity, secret absence và development-path absence.
DEB phải có client, desktop, user unit, desktop entry/icon, license/notice và
maintenance file hiện có; không được có
`/usr/lib/systemd/system/synveil-client.service`. Windows packager tạo ZIP
reproducible có hai EXE, `qt.conf`, Qt DLL/QML/plugin closure target,
`platforms/qwindows.dll`, C++ runtime khi cần và license/notice. PE import
closure reject non-system DLL thiếu, Linux library, SDK/development file và
repository/build path.

Các check repository-owned là:

```text
cargo test -p synveil-client --lib --locked -- --nocapture
cargo test -p synveil-metadata --test linux_desktop_launch_units --locked -- --nocapture
cargo test -p synveil-metadata --test windows_desktop_packaging_units --locked -- --nocapture
cargo test -p synveil-metadata --test linux_install_lifecycle --locked -- --nocapture
cargo test -p synveil-metadata --test linux_deployment_adversarial_units --locked -- --nocapture
cargo test -p synveil-metadata --test linux_native_packaging_units --locked -- --nocapture
deploy/packages/build.sh --format=all
deploy/packages/build-windows.sh --desktop-binary=... --client-binary=... --qt-prefix=...
systemd-analyze verify <disposable-user-unit>
```

`LIVE-LAUNCH1`–`LIVE-LAUNCH10` vẫn là acceptance test cần environment thật:
GUI start client, reuse client đang chạy, GUI quit độc lập, reopen, crash/
reconnect có GUI mở và đóng, start race, security/protocol fail-closed và
chạy từ package layout ngoài source tree. Chỉ báo pass khi có evidence profile/
state disposable thật. Native Windows Task Scheduler registration, startup ZIP
portable và native tray interaction là **NOT RUN** trên Linux host không có
Windows/Wine; không được suy ra từ cross-build hoặc static XML test.

Prompt 99 thêm zero migration, route, OpenAPI operation, web change hay sync
correctness record. PostgreSQL không phải dependency của launch/package
boundary. PostgreSQL test bị skip vì `SYNVEIL_TEST_DATABASE_URL` unset vẫn là
unverified, không được đếm ngầm là pass. Marker đầy đủ Prompt 99 là
`SYNVEIL_PRODUCTION_DESKTOP_LAUNCH_READY`, chỉ hợp lệ sau khi toàn bộ gate
repository, regression lịch sử, Windows, deployment, package và live bắt buộc
đều pass thật.

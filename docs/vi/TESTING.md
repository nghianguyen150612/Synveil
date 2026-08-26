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
8. Content dựa trên stale version bảo toàn byte incoming và current trong
   conflict representation đã định nghĩa. Không mất byte last-writer-wins.
9. Metadata conflict trả current state/rebase instruction; ancestry directory
   vẫn acyclic và sibling name vẫn unique theo portable profile.
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

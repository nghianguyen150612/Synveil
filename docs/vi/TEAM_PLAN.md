# Kế hoạch thực thi của team và agent Synveil

Trạng thái: **Contract lập kế hoạch quy chuẩn**

Tài liệu này chuyển [ROADMAP.md](ROADMAP.md) thành công việc mà developer con
người và implementation agent có thể thực thi mà không tự định nghĩa lại hệ
thống. Tài liệu bao phủ mọi phase nhưng không cấp quyền thực hiện công việc có
prerequisite gate chưa đạt. Capability giữ trạng thái `PLANNED` hoặc
`EXPERIMENTAL` đã document và không bao giờ thành `IMPLEMENTED` cho tới khi
implementation, validation, hướng dẫn vận hành và tài liệu song ngữ đều qua
promotion gate.

## Mô hình vận hành

Synveil áp dụng bàn giao contract-first, được kiểm soát bằng evidence gate:

```text
task quyết định/spec
    ↓ contract + fixture đã review
task implementation có giới hạn
    ↓ bằng chứng component
task integration/recovery/security
    ↓ bundle bằng chứng phase
gate owner promote
```

Một implementation agent sở hữu một kết quả quan sát được và một tập path có
giới hạn. Agent không tự mở rộng scope để “hoàn tất tính năng”, âm thầm giải
quyết một `OPEN DECISION`, thay đổi ADR đã chấp thuận hay sửa migration đã phát
hành. Nếu contract không đủ, task implementation dừng lại tại một khoảng trống
contract được viết rõ và chuyển sang một task kiến trúc riêng.

Một người hoặc agent được nêu tên **chịu trách nhiệm cuối cùng** cho mỗi work
package. Reviewer chịu trách nhiệm cung cấp bằng chứng độc lập, nhưng accountable
owner vẫn chịu trách nhiệm giải quyết phát hiện và báo cáo residual risk.

## Workstream và trách nhiệm thường trực

| Workstream | Sở hữu | Bắt buộc review |
|---|---|---|
| Architecture / Contracts + Platform / Distribution | Quy trình ADR, bất biến domain/protocol, platform/runtime boundary, deployment mode, capability contract, tương thích contract | Mọi thay đổi ID, event, cursor, object format, public API, service-manager, installer, update, migration hoặc platform boundary |
| Rust Backend | Axum transport, application command/query, composition domain, stable error | Hành vi API, streaming có giới hạn, điều phối transaction |
| Database / Jobs | Schema PostgreSQL, SQLx repository, isolation/lock, migration, outbox/job | Mọi bất biến transaction, backfill, retention query và upgrade |
| Storage / Uploads + Storage Platform | `ObjectStore`, adapter local/S3, filesystem capability probe, storage picker, staging, checksum, vòng đời object, GC | Mọi path content-write, range-read, compression, dedup, storage capability hoặc storage migration |
| Sync | Journal, cursor, conflict, initial scan, client conformance | Mọi mutation namespace live và thay đổi client protocol |
| Backup / Restore | Set, snapshot, manifest, retention, operation restore | Mọi tính năng tuyên bố bảo vệ lịch sử hoặc xóa reference được giữ lại |
| Auth / Security | Credential, authorization policy, share, threat model, audit, abuse limit | Mọi boundary bên ngoài, secret, parser, public link, integration, AI egress |
| Web + UX / Accessibility | App React/Vite, API type được sinh, UX progressive disclosure/accessibility, transfer/recovery UX | Trạng thái user nhìn thấy, terminology, xác nhận phá hủy, onboarding, health, recovery và conflict |
| DevOps / Release + Installer / Updater + Networking / Connectivity | CI, image, Compose/Caddy, native packaging, service adapter, configuration, observability, signed release/update, remote-access connectivity | Deployment, health/readiness, runbook backup, provenance artifact, service privilege, pairing, proxy/TLS, relay và migration behavior |
| QA / Reliability | Kiến trúc test, fixture, model/property/fuzz test, E2E, fault injection | Bằng chứng phase và chấp nhận regression; không chỉ chạy lại happy path |
| Documentation | Parity tiếng Anh/tiếng Việt, tài liệu operator/developer/user, taxonomy trạng thái | Mọi public contract, feature promotion, migration và công bố rủi ro |
| Clients + Desktop / Mobile | Reference client, adapter Windows/macOS/Linux desktop, Android/iPhone/iPad tương lai, local state, negotiation capability | Protocol fixture, ngữ nghĩa name/filesystem của OS, background limit, lưu credential, pairing, sync/backup parity |
| Photos | Photo metadata, derivative, timeline/album, contract PhotoKit | Isolation parser, original, EXIF/location, ngữ nghĩa xóa nguồn |
| AI / Privacy | Runtime Python, provenance OCR/embedding/index, mode và provider policy | Mọi vòng đời derived-data, remote request, model/license và ACL AI search |
| Integrations | Interface connector, Forgejo, webhook/polling, repository backup | Policy SSRF/credential, staleness, tương thích external và restore |
| Product / Project owner | Product scope, ưu tiên gate, mô hình ownership, quyết định license | Lời hứa với user, non-goal, phân tách paid/open, policy trade-off chưa giải quyết |

Team nhỏ có thể giao nhiều workstream cho một người. Các boundary trách nhiệm
vẫn áp dụng, và gate data-safety hoặc authorization nhận một reviewer độc lập ở
mọi nơi khả thi.

## Quy tắc thẩm quyền và single-writer

Thứ tự ưu tiên trong
[CONTRIBUTING_ARCHITECTURE.md](CONTRIBUTING_ARCHITECTURE.md) là bắt buộc: ADR đã
chấp thuận, spec domain/protocol, OpenAPI, migration/format, implementation, rồi
ví dụ không quy chuẩn.

- ADR và mỗi protocol spec có một contract owner trong thời gian thay đổi.
- `api/openapi.yaml` có một integrator cho mỗi change set public-contract.
- Việc cấp số thứ tự migration được serialize. Nội dung migration đã phát hành
  là bất biến; correction là migration mới.
- Client/type được sinh từ source đã ghi nhận và không bao giờ sửa bằng tay.
- Shared test fixture có version và chỉ thay đổi khi mọi consumer nằm trong cùng
  integration plan.
- Task chạm shared authority file phải liệt kê mọi downstream consumer cùng merge
  order trước khi bắt đầu.
- UUIDv7 định danh entity; không bao giờ dùng nó thay cho change sequence có thứ
  tự theo `Library`.
- `ChangeEvent`, internal outbox/job và `AuditEvent` là các contract riêng và
  không được task implementation gộp lại.

## Contract task tiêu chuẩn cho agent

Mọi task coding, documentation, review hoặc operations tương lai phải dùng prompt
đầy đủ sau. Hướng dẫn trong ngoặc vuông được thay bằng giá trị cụ thể; không để
agent tự suy luận.

```markdown
# Task ID and title
[Identifier phase/workstream ổn định và một kết quả]

## Role
[Trách nhiệm domain, accountable owner, reviewer bắt buộc]

## Context
[ADR/heading spec đã chấp thuận chính xác, bằng chứng repository, báo cáo task
trước liên quan và trạng thái feature hiện tại]

## Prerequisite gate
[Gate token được nêu tên cùng artifact/test chính xác đã đạt]

## Objective
[Một kết quả quan sát được từ bên ngoài]

## Exact scope
[State transition, input, error, retry, edge/failure case được bao gồm]

## Expected files/components
[Path thuộc quyền sở hữu được phép; xác định shared file cùng integrator]

## Implementation constraints
[Bất biến domain, boundary transaction/durability, giới hạn auth/privacy,
compatibility, resource bound, dependency được phép]

## Tests
[Unit, integration, conformance, property/fuzz, crash, security, performance,
E2E và upgrade case được task này yêu cầu, có tên cụ thể]

## Validation
[Command, environment, fixture, kiểm tra manual/visual và format bằng chứng mong
đợi chính xác]

## Forbidden changes
[Path không thuộc quyền sở hữu, thay đổi ADR/API/schema/format/status, refactor,
dependency hoặc mở rộng scope mà task không được thực hiện]

## Exit gate
[Kết quả đo được và bằng chứng reviewer cần để hoàn tất]

## Required report
[Kết quả; file chính xác; test cùng pass/fail/skip; migration/format; giả định;
tác động security/privacy; bằng chứng performance; residual risk; follow-up]
```

### Checklist dispatch

Coordinator chỉ dispatch task khi:

1. prerequisite gate được ghi nhận đã đạt;
2. mọi `OPEN DECISION` đang chặn đã đóng hoặc task được giới hạn tường minh vào
   một experiment có thể bỏ;
3. path dự kiến và bị cấm không xung đột với task đang active khác;
4. revision input contract và fixture được pin;
5. failure case và abuse case là một phần scope, không bị hoãn thành QA chung;
6. reviewer/integration owner và merge order được nêu tên;
7. task đủ nhỏ để rollback mà không loại bỏ work không liên quan.

### Báo cáo hoàn tất bắt buộc

“Done” không phải báo cáo hợp lệ. Agent báo cáo:

- `COMPLETE`, `PARTIAL` hoặc `BLOCKED` theo exit gate đã nêu;
- chính xác file được thêm/sửa/xóa;
- tác động tới migration, public API, event/schema, stored-format và configuration;
- command đã chạy và số lượng pass/fail/skip chính xác khi tool cung cấp;
- giả định chưa test và giới hạn môi trường;
- phát hiện security, privacy, durability, performance và compatibility;
- mọi `OPEN DECISION` mới, cùng owner và needed-by gate;
- một task follow-up duy nhất được khuyến nghị, không phải implementation tự ý.

Reviewer tái lập bằng chứng quan trọng hoặc ghi rõ vì sao không thể. Task có test
bắt buộc bị skip vẫn là partial.

## Thực thi song song và tích hợp

### Công việc song song an toàn

- UI component và API implementation chỉ được làm song song sau khi fixture
  OpenAPI/error/idempotency đã review.
- Adapter object-store có thể làm song song theo một conformance suite; chỉ một
  task sửa contract trait/capability.
- Handler API và worker có thể làm song song sau khi schema database/job cùng ngữ
  nghĩa lease được đóng băng.
- Công việc tài liệu tiếng Anh sang tiếng Việt đi theo semantic revision đã đóng
  băng; cả hai bản tham gia cùng release gate.
- Công việc AI, Photos và Forgejo tùy chọn có thể chạy độc lập sau khi contract
  input/event chuẩn và boundary privacy được cố định.

### Công việc phải serialize

- đánh số migration và thay đổi quan hệ domain;
- phân bổ change-journal, ngữ nghĩa cursor và conflict;
- commit/finalization object và quy tắc đủ điều kiện GC;
- format capability auth/session/share;
- release manifest và promote version;
- giải quyết ADR hoặc `OPEN DECISION` đang chặn dùng chung.

### Trình tự tích hợp

1. Contract owner công bố revision và fixture đã review.
2. Component owner triển khai trên branch/worktree cô lập hoặc path không chồng
   lấn.
3. Component test đạt theo fixture được pin.
4. Một integration owner kết hợp thay đổi theo thứ tự dependency, sinh lại
   artifact và chạy test cross-domain.
5. Security/QA độc lập thử negative path và failure path.
6. Documentation và operations owner đối chiếu trạng thái cùng runbook.
7. Gate owner ghi bằng chứng và promote—hoặc từ chối—phase token.

## Kế hoạch thực thi theo phase

Mỗi phase bên dưới nêu mọi field lập kế hoạch bắt buộc. Ma trận scenario chi tiết
nằm trong [TESTING.md](TESTING.md); threat control nằm trong
[SECURITY.md](SECURITY.md); bằng chứng deployment nằm trong
[DEPLOYMENT.md](DEPLOYMENT.md).

Phần phase bên dưới dùng nhãn `SV-G*` cho expanded roadmap internal evidence
checkpoint. Khi `Prerequisite gate` hoặc `Next-phase gate` chứa giá trị `SV-G*`,
phải đọc cùng mapping canonical master-gate ở phần sau; giá trị `SV-G*` không
tự nó cấp quyền promotion.

### Phase 0 — Nền tảng repository và kiến trúc

**Objective:** Tạo monorepo và runtime skeleton chưa có feature, an toàn, quan
sát được, cưỡng chế contract mà không biến Compose thành định nghĩa sản phẩm.

**Prerequisite gate:** Hoàn tất review Blueprint/ADR; repository audit được chấp
thuận; đã gán owner cho `OPEN DECISION` Phase 0.

**Responsible workstreams:** Architecture / Contracts + Platform / Distribution
chịu trách nhiệm cuối; Rust Backend, Database, Web + UX / Accessibility, DevOps
+ Installer / Updater + Networking / Connectivity, Security, QA, Documentation,
Storage Platform và project owner chịu trách nhiệm cho package có giới hạn.

**Tasks:**

1. Đóng băng quy ước naming, error, UUIDv7, timestamp, keyset, idempotency,
   migration, configuration và feature-status.
2. Tạo Rust workspace và composition root API/worker cùng kiểm tra hướng
   dependency; chỉ thêm crate có nội dung Phase 0 thực.
3. Tạo workspace React/TypeScript/Vite nghiêm ngặt và pipeline API type được
   sinh/validate.
4. Tạo migration runner PostgreSQL cùng schema skeleton jobs/outbox và quy tắc
   advisory-lock/checksum.
5. Tạo configuration validation, liveness/readiness, tracing/che log và skeleton
   bootstrap một lần.
6. Tạo profile developer và Advanced / Server cho Compose/Caddy với private
   network và khai báo persistent-volume; không gắn nhãn đây là Personal / Home
   onboarding.
7. Đóng băng Personal / Home với Advanced / Server, platform-runtime và
   service-lifecycle port, filesystem capability probe, terminology progressive
   disclosure, pairing, health layer, remote access, update, uninstall và
   migration/recovery contract trong tài liệu cặp đôi và ADR.
8. Thêm CI cho formatting, lint, unit/integration, migration, link docs,
   dependency/license, secret và kiểm tra image.
9. Ghi lại quy trình quyết định của owner cho MIT so với phân tách license tương
   lai; không thay đổi license khi chưa được authorize.

**Allowed parallel work:** Rust, web, docs và deployment skeleton có thể chạy
song song sau khi cố định path owner cùng quy ước configuration/API. Một
integrator sở hữu OpenAPI; một integrator sở hữu migration.

**Dependencies:** Không có product code; lựa chọn phiên bản toolchain/runtime,
ADR, quyết định tên portable, kế hoạch test durability filesystem và quy trình
quyết định contributor license.

**Expected files/components:** file workspace/toolchain root; `bins/`; các
`crates/` ban đầu thuộc sở hữu; `apps/web/`; `api/openapi.yaml`; `migrations/`;
`deploy/`; configuration CI; script; test; tài liệu cặp đôi. Không tạo tree
client tương lai rỗng.

**Tests:** Clean build; kiểm tra hướng dependency; unit test error/ID/config;
test apply/reapply/concurrency migration; integration PostgreSQL; race bootstrap
một lần; health/che log; Compose smoke; type/lint/build web; link docs; scan
secret/dependency/license.

**Definition of done:** Clean checkout đạt CI; Compose đạt readiness; bootstrap
có thể hoàn tất an toàn một lần; không cần secret/manual DB edit; trạng thái vẫn
trung thực.

**Security gate:** Boundary threat, nguồn secret, quy tắc token/log, least
privilege, hành vi đóng/race bootstrap, dependency policy và không có phát hiện
critical chưa chấp nhận đều đã review.

**Performance gate:** Measurement baseline build/start/health và no-op request
được ghi cùng môi trường; có instrumentation cho event-loop và DB pool.

**Documentation gate:** Architecture, quy tắc contributing, deployment skeleton,
command test, configuration và parity contract Anh/Việt được liên kết và chính xác.

**Next-phase gate:** Ghi `SV-G0-FOUNDATION` sau khi các subgate đa nền tảng được
ghi nhận; chỉ sau đó mới dispatch task schema/storage/auth Phase 1.

### Phase 0A — Nền tảng đa nền tảng

**Objective:** Đóng băng protocol/data-model/platform boundary dùng chung trước
khi task implementation chọn service hoặc storage behavior theo host.

**Prerequisite gate:** Review Blueprint/ADR và `SYNVEIL_CONTRACTS_READY`.

**Responsible workstreams:** Platform / Distribution và Architecture / Contracts
chịu trách nhiệm cuối; Storage Platform, Clients + Desktop / Mobile, Web + UX /
Accessibility, Security, QA, Documentation và project owner review.

**Tasks:**

1. Duy trì platform contract Anh/Việt cho Windows, macOS, Linux Desktop, Linux
   Server và client Android/iPhone/iPad tương lai.
2. Đóng băng `PlatformRuntime`, `ServiceLifecycle`, `SecretStore`, storage
   discovery, update, diagnostics và network-discovery port; giữ Windows
   Service, launchd, systemd và Compose trong adapter.
3. Đóng băng `StorageBackend → StorageCapabilities`, portable fallback behavior,
   filesystem/NAS/object-store capability fixture và Btrfs/WinBtrfs accelerator
   tùy chọn.
4. Đóng băng onboarding Personal / Home và Advanced / Server, pairing, lớp
   remote access, diagnostics user/admin/developer và core flow không terminal.
5. Thêm evidence contract/error/accessibility/recovery cho
   `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` mà không tạo implementation
   directory cho client tương lai.

**Definition of done:** Hai mode dùng chung API, domain model, storage
correctness rule và stable error contract; gate không tuyên bố native installer
hay client đã tồn tại.

**Next-phase gate:** `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY`.

### Phase 0B — Nền tảng distribution và recovery

**Objective:** Biến platform contract thành kế hoạch evidence-ready cho
installation, service, update, uninstall, migration và recovery.

**Prerequisite gate:** `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY`.

**Responsible workstreams:** Installer / Updater + Networking / Connectivity
và Platform / Distribution chịu trách nhiệm cuối; Database / Jobs, Storage
Platform, Backup / Restore, Security, QA, Clients + Desktop / Mobile, Web + UX /
Accessibility, DevOps, Documentation và project owner review.

**Tasks:**

1. Định nghĩa native/guided install preflight và storage picker cho từng
   first-class host profile, gồm behavior bảo toàn data khi thất bại.
2. Định nghĩa provision/discover/start/backup/restore/upgrade PostgreSQL managed
   mà không đưa SQLite vào như product model thứ hai.
3. Định nghĩa verify signed release/update, service recovery khi crash/reboot/
   sleep, health translation và automatic maintenance có giới hạn.
4. Định nghĩa retention choice uninstall/reinstall và fixture machine
   migration/recovery theo inspect/plan/validate/execute/verify.
5. Xây support matrix release-lab và documentation-only gate bundle; để mở
   package format và relay decision cho tới khi có evidence.

**Definition of done:** Mọi first-class host có evidence path được nêu cho
installation, service lifecycle, health, update, uninstall, migration và
recovery; environment chưa hỗ trợ được gắn nhãn, không suy diễn.

**Next-phase gate:** Đóng góp cho `SYNVEIL_FOUNDATION_READY`; không cấp quyền
gọi native package là supported trước khi platform lab đạt.

### Phase 1 — Nền tảng storage

**Objective:** Cung cấp file/folder logic đã xác thực và content I/O streaming
có giới hạn trên adapter local production.

**Prerequisite gate:** `SYNVEIL_FOUNDATION_READY` sau
`SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` và `SV-G0-FOUNDATION`; các quyết định
về tên portable và durability cục bộ đã đóng.

**Responsible workstreams:** Storage / Uploads và Rust Backend chịu trách nhiệm
cuối; Database, Auth / Security, Web, QA, DevOps, Documentation là reviewer bắt
buộc.

**Tasks:**

1. Triển khai user, bootstrap admin, credential Argon2id, session mờ đục được
   hash, phòng vệ CSRF, cấp recovery-code và authorization tập trung.
2. Triển khai `Library`, `Node`, `FileVersion`, `Object`, `ObjectReplica`,
   `StorageBackend` và metadata lease cùng constraint và hành vi revision/ETag.
3. Triển khai `ObjectStore` local an toàn: key được sinh, streaming có giới hạn,
   staging, promotion bền vững, head/read/range, abort và delete có kiểm soát.
4. Triển khai folder/list/rename/move và commit content streamed cơ bản với
   journal/audit/outbox được gắn transaction.
5. Triển khai range download/content-disposition, quota precheck, báo capacity
   disk và mapping stable error.
6. Xây web file manager accessible tối thiểu trên API type được sinh.

**Allowed parallel work:** Auth, logical metadata, local adapter, web UI và
adapter conformance test sau khi contract schema/API đóng băng. Điều phối content
commit có một owner và merge sau primitive adapter cùng DB.

**Dependencies:** PostgreSQL primary, identity local root đã chọn, contract
ObjectStore và logical-object được chấp thuận, interface policy tập trung, cấu
hình streaming Caddy và closure OD-SYNC-004 trước schema/API freeze của Node nếu
representation đó mutate state ancestor hoặc subtree.

**Expected files/components:** crate domain/application/auth/API; migration/
repository metadata-Postgres; object-store/local adapter; API contract; view Web
Files; tài liệu storage/deployment/runbook; integration test.

**Tests:** Authorization matrix/IDOR; path traversal/symlink/TOCTOU; name
normalization/collision/cycle; SQL binding; CSRF/XSS filename; streaming/range;
file zero/large; quota/disk-full; durability; object-written/DB-failed;
DB-committed/response-lost; Compose E2E.

**Definition of done:** User đã authorize có thể create/list/move/rename, upload
và range-download file đã verify; file không bao giờ được buffer toàn bộ; không
metadata hiển thị nào có thể tham chiếu byte partial/missing.

**Security gate:** Control auth/session/recovery, scope lookup object, path
safety, output safety filename, giới hạn body/rate/concurrency, che secret/log
và negative authorization test đều đạt.

**Performance gate:** Memory có giới hạn độc lập kích thước object; công bố
baseline có thể tái lập cho listing, concurrent streaming, range, SHA-256 và DB
pool cùng regression budget được chấp thuận.

**Documentation gate:** Tài liệu trạng thái API, storage, auth/security, capacity/
disk-full cho operator và web được cập nhật ở cả hai ngôn ngữ.

**Next-phase gate:** Ghi `SV-G1-STORAGE`; đóng băng contract basic object commit
và precondition recursive-subtree OD-SYNC-004 trước công việc completion có thể
tiếp tục hoặc Trash recursive.

### Phase 2 — Upload đáng tin cậy và an toàn dữ liệu

**Objective:** Làm upload, version, trash, integrity, reconciliation và GC an
toàn qua gián đoạn, retry và crash.

**Prerequisite gate:** `SV-G1-STORAGE`; conformance adapter local xanh.

**Responsible workstreams:** Storage / Uploads chịu trách nhiệm cuối; Database /
Jobs, Backup, Security, QA, Rust Backend, Web, DevOps và Documentation chịu trách
nhiệm.

**Tasks:** Triển khai state machine `UploadSession`/`UploadPart` bền vững; API
part/status/retry; assembly/verification/completion được serialize; terminal
outcome bền vững; duyệt/restore version; retention trash/purge; đối soát orphan
và staging; scan/quarantine toàn vẹn; lease; mark/sweep GC nhận biết reference;
accounting logic/physical và tool audit cho operator.

**Allowed parallel work:** Transport part, UI version/trash, invariant auditor
và harness fault-injection sau khi schema lifecycle đóng băng. Completion và
điều kiện đủ GC là vùng contract serialize với một integrator.

**Dependencies:** Content commit ổn định, quy tắc checksum SHA-256/representation,
lease job/outbox, hành vi durability backend, mọi kiểu reference được bảo vệ và
subtree precondition OD-SYNC-004 đã chấp thuận từ `SV-G1-STORAGE`.

**Expected files/components:** domain/API uploads, storage, jobs, version/trash;
migration; worker reconciliation/integrity; view web transfer/version/trash;
failure harness và runbook.

**Tests:** Conflict part duplicate/different-payload; gap/overlap/overflow; race
expiry; completion đồng thời; mất response completion; crash tại mọi boundary
object/DB/outbox; cạn disk/quota; object hỏng; active lease; orphan grace; GC
dry/live; giới hạn purge đệ quy; hash version restore.

**Definition of done:** Một logical outcome cho mỗi idempotency key; object đã
commit được verify; rò rỉ orphan/staging có thể quan sát và reclaim an toàn;
restore version/trash hoạt động; GC không thể xóa protected reference.

**Security gate:** Ownership session/part, resource limit, độ tin cậy checksum,
authorization cleanup, quyền truy cập quarantine và audit đạt adversarial review.

**Performance gate:** Concurrency assembly/checksum có giới hạn; benchmark part
size, completion, integrity scan và GC trên dataset công bố; API latency không
bị công việc worker tùy chọn chặn.

**Documentation gate:** Trạng thái/lỗi/retry upload, retention version/trash,
integrity/quarantine, accounting, GC và crash runbook thống nhất song ngữ.

**Next-phase gate:** Chỉ ghi `SV-G2-DATA-SAFETY` sau bằng chứng invariant audit và
restore; bật branch prototype sharing/device và tùy chọn.

### Phase 3 — Nền tảng sharing và thiết bị

**Objective:** Thêm delegated access và identity thiết bị có audit, có thể revoke.

**Prerequisite gate:** `SV-G2-DATA-SAFETY`; quyết định ownership/isolation đã đóng.

**Responsible workstreams:** Auth / Security chịu trách nhiệm cuối; Rust Backend,
Database, Devices/Clients, Web, QA, DevOps và Documentation chịu trách nhiệm.

**Tasks:** Share riêng tư; capability public link; expiry/password/permission;
revoke nguyên tử; audit share; đăng ký thiết bị; credential có scope; rotation/
revocation; last-seen/status/pause; recovery; rate limiting và abuse budget; UI
Security/Devices/Shared.

**Allowed parallel work:** Domain Device và share sau khi contract policy/audit
chung đóng băng. Edge handling public-share và UI share đã xác thực có thể chạy
song song. Một policy owner tích hợp quy tắc effective permission.

**Dependencies:** Authorization tập trung, boundary ownership, mô hình opaque
token/hash, schema audit, ngữ nghĩa authorization node đệ quy, cấu hình trust
proxy/client IP.

**Expected files/components:** domain/application/API auth/sharing/devices;
migration; component rate-limit/audit; view web Shared/Devices/Security; tài
liệu security/operations và abuse test.

**Tests:** IDOR chéo user; grant kế thừa/đệ quy; writable/read-only; expiry/time
skew; brute force password tùy chọn; hash/non-enumeration token; revoke trong
download/mutation; replay rotation; revoke thiết bị; session epoch; chiếm đoạt
recovery; các chiều rate; che audit.

**Definition of done:** Mọi grant tường minh và có thể revoke; sở hữu ID thông
thường không cấp quyền; credential bị revoke thất bại trong giới hạn được ghi;
UI và audit báo trạng thái trung thực.

**Security gate:** Ma trận authorization độc lập cùng review public-abuse, test
threat credential/recovery, không lưu/log raw token và hành vi revoke ổn định.

**Performance gate:** Kiểm tra authorization và rate có query giới hạn, không
load subtree vô hạn; concurrency/byte download công khai được cấp budget.

**Documentation gate:** Permission/limit sharing, giới hạn revoke thiết bị,
recovery và activity data được ghi ở cả hai ngôn ngữ.

**Next-phase gate:** Ghi `SV-G3-TRUSTED-ACCESS`; identity thiết bị và grant là
input đóng băng cho sync.

### Phase 4 — Protocol sync

**Objective:** Cung cấp ordered change feed đúng reference conformance và
protocol mutation offline an toàn.

**Prerequisite gate:** `SV-G3-TRUSTED-ACCESS`; mọi namespace mutation phát journal
fact đã review.

**Responsible workstreams:** Sync chịu trách nhiệm cuối; Database, Rust Backend,
Clients, Storage, Security, QA, DevOps và Documentation chịu trách nhiệm.

**Tasks:** Clock/epoch theo library; codec cursor có version; page thay đổi có thứ
tự; initial snapshot/rebaseline; retention cursor; checkpoint; mutation ID;
ETag/base version; conflict copy content xác định; rebase metadata; quy tắc
delete/restore/move/cycle/name; client state-machine tham chiếu; fixture có
version và simulator.

**Allowed parallel work:** Server journal, reference client và fixture model-based
sau khi protocol đóng băng. Presentation UI conflict có thể dùng fixture. Chỉnh
contract clock allocation/cursor/conflict là single-writer.

**Dependencies:** Credential thiết bị, ngữ nghĩa logical metadata/version/trash,
idempotency store, tên portable, bất biến journal trong transaction, cấu hình
retention vận hành.

**Expected files/components:** domain/application/API sync, migration, client/
reference core, protocol fixture, surface web Activity/conflict, metric/runbook,
spec `SYNC` và test harness.

**Tests:** Mọi scenario và invariant trong phần sync của
[TESTING.md](TESTING.md), gồm offline/offline, delete/edit, response trùng/mất,
write trong pagination, cursor stale/foreign/old-epoch, apply page local nguyên
tử, race directory, backlog, restart server, disk full và thiết bị bị revoke.

**Definition of done:** Client được hỗ trợ không thể âm thầm bỏ sót event đã
commit hay ghi đè byte conflict; lịch sử hết hạn có rebaseline hoàn chỉnh do
server chỉ đạo; hành vi fixture khớp server và reference client.

**Security gate:** Integrity/scope cursor, authorization mutation, thiết bị đã
revoke, replay/idempotency, rò rỉ metadata và giới hạn abuse/backlog đã review.

**Performance gate:** Ghi contention mutation theo library, latency page/list,
lag change-feed, chi phí client apply và catch-up backlog dưới concurrency được
công bố; memory cùng page size vẫn có giới hạn.

**Documentation gate:** Protocol sync quy chuẩn, nghĩa vụ client, retention, UX
conflict, error và recovery thống nhất trong Anh/Việt và OpenAPI.

**Next-phase gate:** Ghi `SV-G4-SYNC-CONFORMANT`; đóng băng conformance fixture
cho consumer backup/desktop.

### Phase 5 — Backup và restore

**Objective:** Cung cấp snapshot thiết bị được giữ độc lập, có thể verify và
recovery không phá hủy.

**Prerequisite gate:** `SV-G4-SYNC-CONFORMANT`; contract `BackupSet`/snapshot
được review tách biệt với live sync.

**Responsible workstreams:** Backup / Restore chịu trách nhiệm cuối; Storage,
Database, Clients, Rust Backend, Security, QA, Web, DevOps và Documentation chịu
trách nhiệm.

**Tasks:** Định nghĩa backup set/policy/source; trạng thái manifest building/
verifying/committed; tái sử dụng unchanged tăng dần; consistency label; retention
và hold; trạng thái thiết bị; operation restore có thể restart; collision policy;
recovery file/folder/snapshot; workflow clean-device; metric recovery operator.

**Allowed parallel work:** Client scanner, server snapshot builder, restore
planner và web UI sau khi format/API manifest đóng băng. Tích hợp retention và GC
chỉ merge sau khi mô hình hóa mọi lớp reference.

**Dependencies:** Object đã verify, lease/GC, identity thiết bị, durable job,
quota/accounting, manifest có version, policy restore destination.

**Expected files/components:** domain/application/API backup; migration; manifest
codec; worker retention/restore; adapter backup client; view web Backups/Restore;
fixture, runbook và docs.

**Tests:** Mọi backup scenario trong [TESTING.md](TESTING.md): xóa local, bỏ sót,
scan bị gián đoạn/lặp lại, file lớn unchanged/changed, thay đổi đồng thời, tính
nguyên tử commit snapshot, corruption, quota, thiết bị bị loại, an toàn reference/
retention, partial restore/retry, recovery clean-destination.

**Definition of done:** Chỉ snapshot hoàn chỉnh đã verify mới restore được;
nguồn thiếu không bao giờ lan truyền live deletion; retention bảo toàn mọi
snapshot được chọn; clean restore độc lập verify byte mong đợi và báo skip.

**Security gate:** Ownership backup set, xử lý manifest/path, authorization
restore target, policy symlink, xóa protected-history, mất thiết bị và audit đã
review.

**Performance gate:** Giới hạn memory/page manifest, hiệu quả unchanged-file,
commit snapshot, retention scan và throughput restore được đo bằng profile file
nhỏ và file lớn.

**Documentation gate:** Backup so với sync, consistency label, schedule,
retention, collision/error/report restore và runbook mất thiết bị thống nhất.

**Next-phase gate:** Ghi `SV-G5-RESTORABLE`; reviewer riêng ký bằng chứng clean
restore.

### Phase 6 — Tối ưu storage

**Objective:** Thêm compression đã đo và whole-object dedup có giới hạn mà không
thay đổi dữ liệu logic hoặc bảo đảm vòng đời.

**Prerequisite gate:** `SV-G5-RESTORABLE`; GC biết mọi protected reference.

**Responsible workstreams:** Storage chịu trách nhiệm cuối; Database, Backup,
Security, QA, DevOps, Rust Backend và Documentation chịu trách nhiệm.

**Tasks:** Classifier/policy compression; metadata representation Zstandard;
checksum plaintext/stored; decode path; lookup/verification dedup trong domain;
accounting logical/physical; job re-encoding/copy-switch; audit GC/reference;
feature flag và compatibility reader.

**Allowed parallel work:** Benchmark/classifier, codec adapter và UI accounting
sau khi contract representation đóng băng. Aliasing dedup và GC vẫn được tích
hợp tuần tự.

**Dependencies:** Mô hình object bất biến, SHA-256, representation có version,
reference backup, restore vững chắc, capability storage migration.

**Expected files/components:** Codec/policy object-store, metadata/migration,
worker, accounting query/UI, adapter conformance, corpus benchmark/fuzz, tài
liệu storage/upgrade.

**Tests:** Round trip byte-exact; type đủ/không đủ điều kiện; frame truncated/
corrupt; decompression bomb; phiên bản codec; path quarantine hash collision;
owner cùng/khác; concurrent dedup; retention/GC; đọc representation cũ;
migration bị gián đoạn và rollback window.

**Definition of done:** Policy được bật tiết kiệm storage có thể đo trên workload
được công bố; disable chúng vẫn bảo toàn read/restore; không tồn tại rò rỉ sự tồn
tại chéo owner hay xóa sớm.

**Security gate:** Giới hạn decompression, side channel chéo domain,
authorization/accounting, thư viện parser/supply-chain và tác động secret/key
của migration đã review.

**Performance gate:** Công bố compression ratio, CPU, throughput, memory,
amplification range-read và overhead lookup/GC dedup theo workload; chỉ quy tắc
có lợi mới bật mặc định.

**Documentation gate:** Ngữ nghĩa codec/hash/accounting, format bị skip,
migration/rollback và boundary privacy thống nhất.

**Next-phase gate:** Ghi `SV-G6-OPTIMIZED-SAFELY`; chunk dedup nâng cao
`PLANNED` ở sau ADR mới được chấp thuận và gate promotion đo lường về sau.

### Phase 7 — Desktop reference client

**Objective:** Chứng minh selected-folder sync và backup trên ngữ nghĩa filesystem
desktop thực tế.

**Prerequisite gate:** `SV-G4-SYNC-CONFORMANT`; tuyên bố backup còn đòi hỏi
`SV-G5-RESTORABLE`.

**Responsible workstreams:** Clients chịu trách nhiệm cuối; Sync, Backup,
Security, QA, Release, Documentation và platform owner chịu trách nhiệm.

**Tasks:** Quyết định boundary shared Rust core; local state bền vững; onboarding;
credential thiết bị; sync initial/rebaseline/incremental; filesystem watcher
cùng rescan; UX conflict/status; backup được chọn; bandwidth/retry; policy
packaging/update và diagnostic.

**Allowed parallel work:** Protocol core và từng platform adapter sau khi đóng
băng fixture. Một local-state schema owner và một packaging owner tích hợp.

**Dependencies:** Sync fixture, tên portable, credential thiết bị, endpoint
range/resumable, contract backup, key store OS và supported-platform matrix.

**Expected files/components:** `clients/sync-core` thật và package nền tảng chỉ
khi được triển khai; test platform adapter; asset packaging/release; docs client
và support matrix.

**Tests:** Reference fixture cộng case/Unicode/reserved name, symlink/loop, file
bị khóa, permission, watcher overflow, clock skew, atomic replace, reboot/crash,
local DB corruption, low disk, thay đổi network, conflict, stale cursor, revoke
credential, full rebuild và ngữ nghĩa uninstall/cache.

**Definition of done:** Mỗi nền tảng được tuyên bố hoàn tất initial và
incremental sync, bảo toàn offline conflict, revoke thiết bị và restore/rebaseline
trong giới hạn đã công bố.

**Security gate:** Dùng key-store, integrity update/package, permission local DB/
path, che token/path trong log, xử lý symlink và consent diagnostic.

**Performance gate:** Initial scan, watcher/reconcile, kích thước local DB,
CPU/memory, network concurrency và catch-up backlog được đo trên cấu trúc
directory đã công bố.

**Documentation gate:** Installation, policy, conflict, status, filesystem/nền
tảng được hỗ trợ, limit và troubleshooting thống nhất.

**Next-phase gate:** Ghi `SV-G7-DESKTOP-REFERENCE`; dùng bằng chứng client cho
quyết định Apple và smart-storage.

### Phase 8 — Ảnh

**Objective:** Xây thư viện ảnh bảo toàn original với derivative có thể bỏ và
được kiểm soát privacy.

**Prerequisite gate:** `SV-G5-RESTORABLE`; chọn policy deletion/backup ảnh trước
khi promote.

**Responsible workstreams:** Photos chịu trách nhiệm cuối; Storage, Backup,
Security, Web, QA, Clients, DevOps và Documentation chịu trách nhiệm.

**Tasks:** Mô hình photo asset/group; idempotency upload/import original;
metadata timeline, album/favorite; trích EXIF; worker thumbnail/rendition; phân
loại image/video/screenshot; exact duplicate và provenance suggestion; search
metadata; purge/rebuild derivative; trạng thái backup.

**Allowed parallel work:** UI/timeline, parser sandbox, pipeline derivative và
contract PhotoKit sau khi schema/event photo đóng băng. Vòng đời original có
một integrator storage/backup.

**Dependencies:** Upload/version/backup chuẩn, job lease, isolation parser,
privacy policy, inventory codec/runtime được hỗ trợ.

**Expected files/components:** Domain/API/job photos, migration, view web Photos,
policy service/container parser/rendition, corpus/fixture và docs.

**Tests:** Media malformed/truncated/huge-dimension; giới hạn image/video;
preview HEIC/HEVC không hỗ trợ; ACL EXIF location/strip khi share; hash original;
derivative stale/rebuild/delete; gợi ý duplicate; resource được nhóm; xóa nguồn;
retention/restore; worker vắng/crash.

**Definition of done:** Original vẫn nguyên vẹn/tải xuống/restore được khi thiếu
photo worker; derivative gắn version/có thể rebuild; metadata nhạy cảm được
authorize; hành vi xóa nguồn tường minh.

**Security gate:** Sandbox/egress/resource limit parser, privacy EXIF/location,
SVG độc hại/content sniffing, sharing và authorization cache derivative.

**Performance gate:** Pagination timeline, tuổi queue thumbnail, budget pixel/
CPU/memory, upload video và amplification storage derivative được đo.

**Documentation gate:** Ngữ nghĩa original/derivative, privacy, codec, hành vi
delete/backup và degradation worker thống nhất.

**Next-phase gate:** Ghi `SV-G8-PHOTOS-SAFE`; đóng băng API hướng PhotoKit và ngữ
nghĩa group.

### Phase 9 — Prototype contract Apple client

**Objective:** Xác thực boundary platform Apple và tạo evidence contract/fixture
có thể bỏ. Đây là bước prototype sớm trong master plan, không phải promotion
Apple client cuối; workflow Apple được hỗ trợ vẫn chờ gate sau tích hợp
Forgejo/code.

**Prerequisite gate:** `SV-G7-DESKTOP-REFERENCE` và `SV-G8-PHOTOS-SAFE`; quyết
định prototype shared core đã ghi.

**Responsible workstreams:** Apple Clients chịu trách nhiệm cuối; Sync, Photos,
Backup, Security, QA, Release và Documentation chịu trách nhiệm.

**Tasks:** Onboarding/status Swift/SwiftUI; auth Keychain; upload background
URLSession; import tăng dần/quyền giới hạn PhotoKit; enumeration FileProvider,
change anchor, hydrate/evict; policy thiết bị; UI conflict/error; packaging và
privacy declaration.

**Allowed parallel work:** Adapter PhotoKit và FileProvider sau khi contract
server đóng băng; auth/onboarding có thể chạy độc lập. Một Apple release
integrator sở hữu entitlement và package.

**Dependencies:** Negotiation capability server, API cursor/resumable/range,
photo group/import ID, policy backup, thiết bị thật/OS matrix, review license
App Store.

**Expected files/components:** Swift package/app chỉ khi công việc thật bắt đầu;
API layer được sinh; platform fixture; kế hoạch CI/device test; docs privacy/
support.

**Tests:** Quyền PhotoKit limited/full/no; revoke quyền; callback duplicate;
asset đã edit/delete; background termination/resume; low power/data/disk;
FileProvider stale anchor và trạng thái placeholder; conflict; Keychain/revoke
thiết bị; tương thích server.

**Definition of done:** Prototype và platform fixture exercise boundary đã claim,
tôn trọng permission scope và mô tả trung thực giới hạn background/backup mà
không claim supported product client.

**Security gate:** Keychain, trust ATS/TLS, xử lý URL/open redirect, entitlement,
bảo vệ local cache, nhãn diagnostic/privacy và wording xóa remote cache đã review.

**Performance gate:** Transfer background, enumeration page/apply, hydration,
battery/network và hành vi local-cache được đo trên thiết bị thật được hỗ trợ.

**Documentation gate:** Onboarding, permission, backup so với sync, trạng thái
FileProvider, troubleshooting và non-goal thống nhất.

**Next-phase evidence:** Ghi `SV-G9-APPLE-CONTRACT-PROTOTYPE` chỉ như internal
prototype evidence bundle. Gate chuẩn `SYNVEIL_APPLE_CLIENT_READY` không thể
promote trước bước Apple trong master sequence sau
`SYNVEIL_CODE_INTEGRATION_READY`.

### Phase 10 — Nền tảng AI

**Objective:** Thêm intelligence dẫn xuất tùy chọn mà không egress dữ liệu remote
âm thầm và không tạo dependency availability cho core.

**Prerequisite gate:** `SV-G2-DATA-SAFETY`; durable job/outbox, authorization và
contract vòng đời derived-record ổn định.

**Responsible workstreams:** AI / Privacy chịu trách nhiệm cuối; Security, Rust
Backend, Database, DevOps, Search/Web, QA, Documentation và model-license owner
chịu trách nhiệm.

**Tasks:** Policy mode/config; boundary runtime Python; input job có scope; OCR/
text extraction; record embedding/vector; pha trộn metadata/semantic search;
provenance AI-tag; độ fresh index; retry/dead-letter; exclusion theo item; công
bố/credential remote-provider; purge/rebuild và migration model.

**Allowed parallel work:** Benchmark model local, Python sandbox, schema dẫn
xuất và UI search sau khi data classification cùng job contract đóng băng.
Remote mode là phần riêng và không thể ngầm đi kèm.

**Dependencies:** Version ID/event chuẩn, query ACL, deployment pgvector tùy
chọn, parser sandbox, license provider/model/asset, resource profile.

**Expected files/components:** `services/ai`, domain/job/API AI, profile
extension/migration tùy chọn, web Search/AI settings, metadata model, docs
privacy/operations và adversarial corpus.

**Tests:** Core khi AI absent/offline; no-egress disabled/local; consent và
revocation remote; stale version; delete/revoke/purge; thay đổi ACL; prompt
injection; document độc hại; timeout/OOM/retry; dead-letter; reindex; provider
failure/rate limit; che log/content.

**Definition of done:** Core feature và deterministic search hoạt động không có
AI; mode và freshness nhìn thấy được; category dữ liệu remote đòi hỏi policy có
hiệu lực; record dẫn xuất có thể rebuild và purge.

**Security gate:** Review data-flow/privacy, exact provider/origin allowlist,
content capability least-privilege, network/parser/resource sandbox, lưu secret,
ACL-at-query và lag deletion đều đạt.

**Performance gate:** Công bố profile model/hardware, tuổi queue, throughput,
memory/CPU/GPU, kích thước index/query latency và backpressure; AI không được
làm đói công việc critical API/database/storage.

**Documentation gate:** Hành vi disabled/local/remote, category dữ liệu,
retention provider, model/license, exclusion, freshness và deletion thống nhất.

**Next-phase gate:** Ghi `SV-G10-AI-OPTIONAL`; remote mode có thể vẫn disabled dù
local mode được promote.

### Phase 11 — Tích hợp Forgejo

**Objective:** Khám phá và restore repository Forgejo mà không triển khai một
forge hay biến Forgejo thành core dependency.

**Prerequisite gate:** `SV-G5-RESTORABLE`; quyết định về credential/SSRF connector
và representation backup đã review.

**Responsible workstreams:** Integrations chịu trách nhiệm cuối; Backup,
Security, Rust Worker, Database, Web, QA, DevOps và Documentation chịu trách
nhiệm.

**Tasks:** Interface connector; adapter tương thích Forgejo; lưu/rotation secret
đã encrypt; policy exact-origin; polling và webhook hint; inventory repository/
ref/commit; staleness/health; liên kết Project; manifest backup Git/LFS/artifact;
verification và restore không phá hủy.

**Allowed parallel work:** UI inventory, connector adapter và công việc fixture
backup-format sau khi contract đóng băng. Layer credential/SSRF và thực thi
restore có owner riêng duy nhất.

**Dependencies:** Durable job, object/manifest backup, recovery master-secret,
ma trận test Forgejo, sandbox tooling/process Git, quy tắc project/ownership.

**Expected files/components:** Module integrations connector/Forgejo, job/API,
migration, web Code/Projects/Settings, fixture compatibility lab và runbook.

**Tests:** Opt-in origin private/local, DNS rebinding/redirect, che/rotation
credential, webhook HMAC/replay/duplicate/size, đối soát polling, outage/stale
state, phạm vi repository/LFS/artifact, `git fsck`, backup bị gián đoạn, clean
restore, không tương thích version, từ chối overwrite.

**Definition of done:** Lỗi Forgejo chỉ degrade surface tích hợp; phạm vi backup
được tuyên bố tường minh trong manifest và restore độc lập được; Synveil không
phơi custom Git smart HTTP/SSH.

**Security gate:** Credential least-scope đã encrypt, egress/SSRF, webhook, thực
thi process, repository path/content, restore target và audit đã review.

**Performance gate:** Gộp poll/webhook, rate-limit backoff, memory/disk/throughput
repo/LFS lớn và công bằng queue được đo; core worker job không bị làm đói.

**Documentation gate:** Compatibility, scope, phạm vi/omission backup, staleness,
an toàn restore và hành vi outage thống nhất.

**Next-phase gate:** Ghi `SV-G11-FORGEJO-RESTORABLE`; repository intelligence chờ
cả gate AI và connector.

### Phase 12 — AI cộng code

**Objective:** Cung cấp search và question answering repository đúng ACL, mang
provenance trên source có version.

**Prerequisite gate:** `SV-G10-AI-OPTIONAL` và
`SV-G11-FORGEJO-RESTORABLE`.

**Responsible workstreams:** AI / Privacy và Integrations cùng chịu trách nhiệm
cuối; Security, Web, QA, DevOps và Documentation là reviewer bắt buộc.

**Tasks:** Extraction gắn commit/ref; policy file/language/generated/binary;
index metadata README/docs/commit; semantic code search; retrieval project;
citation/provenance câu trả lời; refresh/delete; adapter issue/PR tùy chọn có
mapping permission; loại prompt-injection và secret.

**Allowed parallel work:** Indexer và query UI theo fixture ACL/provenance cố
định. Một owner retrieval/authorization tích hợp filter kết quả cuối.

**Dependencies:** Inventory connector hiện tại và quan hệ truy cập, AI record
gắn version, deletion, giới hạn context/size model, liên kết project.

**Expected files/components:** Code indexer/adapter AI, API/UI search, job,
fixture/corpus, docs privacy/security/operations.

**Tests:** ACL chéo user/project/repo; quyền bị revoke; branch/ref stale; xóa
repository; exclusion secret/generated/binary; prompt/tool injection; repository
độc hại/rất lớn; consent remote; correctness citation; provider failure và
reindex hoàn chỉnh.

**Definition of done:** Kết quả trích dẫn version đã index, không bao giờ vượt
ACL hiện tại, gắn nhãn staleness và biến mất/rebuild theo lifecycle policy; không
AI output nào được coi là sự thật repository.

**Security gate:** Review authorization và prompt-injection độc lập, scan/loại
secret, data classification provider và không thực thi URL/command dẫn xuất từ
content.

**Performance gate:** Chi phí index incremental, context limit, công bằng queue,
latency vector/filter và kích thước index được đo trên hình dạng repository đã
công bố.

**Documentation gate:** Source được index/bỏ qua, permission, freshness, mode
provider, limitation và câu trả lời không có thẩm quyền thống nhất.

**Next-phase gate:** Ghi `SV-G12-CODE-INTELLIGENCE`; không mở rộng tới protocol
forge hay mutation code tự trị.

### Phase 12B — Promotion Apple client theo master sequence

**Objective:** Chỉ promote Apple client sau khi contract client, Photos, AI và
Forgejo/code trước đó trong master plan đã ổn định.

**Prerequisite gate:** `SYNVEIL_DESKTOP_SYNC_READY`,
`SYNVEIL_PHOTOS_FOUNDATION_READY`, `SYNVEIL_CODE_INTEGRATION_READY` và
`SYNVEIL_CLIENT_CONTRACT_READY`. Prototype Phase 9 có thể cung cấp fixture,
nhưng không thay thế các gate này.

**Responsible workstreams:** Apple Clients chịu trách nhiệm cuối; Sync, Uploads,
Photos, Backup, Security, QA, Release và Documentation chịu trách nhiệm.

**Tasks:** Đối chiếu prototype với server contract đã version cuối; hoàn thiện
device auth/revocation, import và permission PhotoKit,
enumeration/change-anchor/hydration FileProvider ở nơi hỗ trợ, resume
background URLSession, UX conflict/error, review package/entitlement và ma
trận support cho từng OS/device được claim.

**Tests:** Chạy matrix platform đầy đủ cho PhotoKit limited/full/revoked,
callback trùng, asset edit/delete, background termination/resume, low power/
network/storage, stale anchor FileProvider, conflict, Keychain/revoke thiết bị,
tương thích upgrade server, clean restore và scheduling best-effort đã document.
Capability không hỗ trợ phải là error/state tường minh, không giả lập âm thầm.

**Definition of done:** Workflow Apple được hỗ trợ recovery sau interruption,
tôn trọng permission/authorization scope, interoperate với server/client fixture
đã freeze và mô tả đúng giới hạn backup/background.

**Security and operations gate:** Keychain/ATS/TLS, entitlement, local cache,
privacy declaration, diagnostic, release provenance, support runbook và
rollback/update behavior được review độc lập.

**Next-phase gate:** Promote token chuẩn chính xác
`SYNVEIL_APPLE_CLIENT_READY`; không tuyên bố full-device backup hay background
liên tục được bảo đảm.

### Phase 13 — Smart storage

**Objective:** Thêm files-on-demand, tiering xác định và bảo vệ bất thường có thể
recovery mà không mất bản sao đã verify.

**Prerequisite gate:** `SYNVEIL_STORAGE_OPTIMIZED` (evidence mở rộng
`SV-G6-OPTIMIZED-SAFELY`), `SYNVEIL_APPLE_CLIENT_READY`, một client đủ capability
và đúng conformance, cùng contract placement/state được chấp thuận. Prototype
cô lập có thể chạy sớm hơn với scope disposable nhưng không được promote phase.

**Responsible workstreams:** Storage và Clients cùng chịu trách nhiệm cuối;
Sync, Backup, Security, QA, DevOps, Web và Documentation chịu trách nhiệm.

**Tasks:** Mô hình placeholder/local state; hydrate/range/resume; eviction cache
đã verify; policy pin/exclude; quy tắc tier replica; job copy/verify/switch/retire;
recommendation capacity; heuristic burst phá hủy, pause/review/override; UX và
metric recovery.

**Allowed parallel work:** Placement engine server và từng placeholder adapter
client sau khi state-machine fixture đóng băng. Retire replica có một integrator
storage/backup.

**Dependencies:** Replica/lease object, storage migration, trạng thái sync,
capability client được hỗ trợ, restore, health/capacity backend, audit policy.

**Expected files/components:** Policy/domain/job placement, adapter client,
trạng thái API/UI, migration nếu cần, fixture state, docs operations và recovery.

**Tests:** Race hydrate/evict/pin; range partial; low disk; offline/backend
failure; move replica bị gián đoạn; target corruption; rollback window; client
stale; delete so với eviction; disable rule; false positive/override bất thường;
restore từ mỗi tier.

**Definition of done:** Ít nhất một bản sao đã verify và có thể truy cập còn lại;
cache state không bao giờ giả dạng server deletion; quyết định rule có thể giải
thích và đảo ngược; xử lý bất thường không thể vĩnh viễn chặn operation hợp lệ.

**Security gate:** Permission placeholder/cache local, authorization policy,
credential backend, archive restore, abuse/lockout bất thường và audit đã review.

**Performance gate:** Latency/throughput hydration, amplification range, cache/
accounting local, chi phí copy placement, queue backend và đánh giá rule được
đo; core operation giữ ưu tiên.

**Documentation gate:** Ý nghĩa state, hỗ trợ nền tảng, giải thích policy,
recovery/override và giới hạn bất thường thống nhất.

**Next-phase gate:** Ghi `SV-G13-SMART-STORAGE`; công việc scale nâng cao vẫn
đòi hỏi trigger được đo độc lập.

### Phase 14 — Scale nâng cao

**Objective:** Xử lý một nút thắt đã đo hoặc mục tiêu availability bằng thay đổi
topology tương thích nhỏ nhất.

**Prerequisite gate:** `SV-G13-SMART-STORAGE` cùng
`SYNVEIL_APPLE_CLIENT_READY` cho promotion sản phẩm theo master sequence, hoặc
trigger đã đo riêng theo capability cùng ADR được chấp thuận cho công việc scale
S3/worker cô lập sớm hơn.

**Responsible workstreams:** Architecture và DevOps / Release chịu trách nhiệm
cuối; các workstream Database, Storage, Sync, Security, QA, Operations và
Documentation bị ảnh hưởng chịu trách nhiệm.

**Tasks:** Đo và xác định trigger; test tuning đơn giản hơn; viết ADR; chỉ triển
khai thay đổi adapter/replica/broker/read-replica/Kubernetes có căn cứ; kế hoạch
mixed-version/rollback; test partition/load/recovery/security; runbook hỗ trợ và
mô hình cost/capacity.

**Allowed parallel work:** Prototype adapter hoặc load harness có thể chạy theo
contract đóng băng. Không cho phép nhiều writer consistency/topology.

**Dependencies:** Measurement production, ownership hỗ trợ, ngữ nghĩa shared
storage, điều phối job/journal, consistency rate/session, backup/restore cho
topology mới.

**Expected files/components:** ADR thay thế, code adapter/topology, profile/
manifest deploy, test lab, migration/rollback, dashboard vận hành và docs.
Artifact Kubernetes không tồn tại trừ khi gate này phê duyệt cụ thể.

**Tests:** ObjectStore conformance; race replica API/worker; thứ tự journal;
generation job lease; shared rate limit/session revocation; network partition;
lag read replica; duplicate/loss broker; mất node; upgrade/rollback; backup/
restore phối hợp và load envelope được công bố.

**Definition of done:** Mục tiêu đã đo được cải thiện mà không làm yếu bất biến
trước đó, và operator có thể deploy, quan sát, upgrade, restore và rollback
topology trong giới hạn được ghi tài liệu.

**Security gate:** Trust boundary mới, network identity/TLS, secret/RBAC,
authorization/cache lag multi-node, supply chain và incident response đã review.

**Performance gate:** Kết quả trước/sau theo phương pháp giống hệt chứng minh
trigger đã được xử lý; trade-off cost và tail-latency/regression được chấp nhận.

**Documentation gate:** ADR, topology, capacity, health, failure, upgrade,
rollback, support và docs kiến trúc song ngữ thống nhất.

**Next-phase gate:** Ghi `SV-G14-SCALE-PROVEN` riêng theo capability; không tuyên
bố “distributed readiness” chung vượt topology đã test.

## Bundle bằng chứng gate

Phase integration owner công bố một bundle có thể review gồm:

- revision contract/ADR đã chấp thuận và quyết định đang chặn đã đóng;
- version source/config/migration/format chính xác;
- kết quả CI và environment matrix;
- báo cáo failure/recovery, security và performance;
- defect đã biết cùng severity và quyết định xử lý khi phát hành;
- bằng chứng deployment/upgrade/rollback/restore;
- review parity tài liệu tiếng Anh/tiếng Việt;
- thay đổi trạng thái được yêu cầu và tập task tiếp theo được phép.

Bằng chứng có version cùng bản phát hành hoặc được link bất biến. Screenshot và
tuyên bố không có command/fixture/configuration là phần bổ sung, không phải bằng
chứng.

## Xử lý rủi ro chương trình

Mỗi rủi ro có một accountable workstream, trigger, mitigation, verification và
hệ quả phát hành. Rủi ro thường trực tối thiểu gồm sync bỏ sót/ghi đè, phân kỳ
metadata/object, nhầm backup/sync, GC quá sớm, migration thất bại, authorization
chéo user, thiết bị bị compromise, parser bị compromise, remote AI egress, Git
credential/SSRF, giới hạn background mobile, license mơ hồ và mở rộng scope.

Nếu trigger cho thấy có thể mất dữ liệu hoặc disclosure không được authorize:

1. dừng feature promotion và destructive automation;
2. bảo toàn log/audit, reference object/snapshot bị ảnh hưởng và bằng chứng tái
   hiện mà không sao chép content nhạy cảm khi không cần;
3. phân loại blast radius và liệu dữ liệu chuẩn còn được verify hay không;
4. tạo corrective task có giới hạn và review recovery/security độc lập;
5. cập nhật runbook và regression fixture trước khi tiếp tục gate.

## Cách agent tương lai dùng kế hoạch này

Coordinator chọn chính xác một work package chưa đạt dưới phase hiện tại, sao
chép contract task tiêu chuẩn, điền path và command cụ thể từ repository hiện
tại, pin revision prerequisite và nêu tên reviewer. Agent trả về báo cáo bắt
buộc. Coordinator không phát task phụ thuộc tiếp theo cho tới khi bằng chứng
được tích hợp và gate đã nêu—không chỉ sự tự tin của agent—đạt.

## Bản đồ workstream của master plan và các gate bắt buộc

Master execution plan nêu mười workstream chức năng và Team Q là vai trò
reliability chạy xuyên suốt. Tài liệu này chia nhỏ thêm một số team thành các
workstream chuyên môn để ownership có thể thực thi; việc chia nhỏ không tạo
thẩm quyền thứ hai và không cho phép specialist tự định nghĩa lại contract do
master team sở hữu.

| Master team | Workstream chi tiết trong kế hoạch này | Trách nhiệm chuẩn | Gate master hoặc gate release |
|---|---|---|---|
| Team A — Architecture & Contracts | Architecture / Contracts, Documentation | ADR, `docs/`, `api/openapi.yaml`, domain/protocol contract, error, versioning, quyết định liên team | `SYNVEIL_CONTRACTS_READY` |
| Team B — Core Backend & API | Rust Backend, Database / Jobs | Rust workspace, Axum API, application service, PostgreSQL, migration, job, health | `SYNVEIL_API_FOUNDATION_READY` |
| Team C — Storage & Data Integrity | Storage / Uploads | ObjectStore, staging, upload, version, integrity, compression, dedup, GC | `SYNVEIL_STORAGE_FOUNDATION_READY`, `SYNVEIL_UPLOADS_RELIABLE`, `SYNVEIL_DATA_INTEGRITY_READY` |
| Team D — Sync Engine | Sync | Change journal, cursor, checkpoint thiết bị, conflict, sync conformance | `SYNVEIL_SYNC_PROTOCOL_STABLE` |
| Team E — Backup & Recovery | Backup / Restore | Backup set, manifest, snapshot, retention, restore, disaster-recovery evidence | `SYNVEIL_BACKUP_RESTORE_READY` |
| Team F — Web Application | Web | Ứng dụng React/TypeScript/Vite, generated client, UX transfer/recovery/conflict | `SYNVEIL_WEB_CORE_READY` |
| Team G — Security & Identity | Auth / Security | Password, session, credential thiết bị, authorization, share, abuse control, threat model | `SECURITY_REVIEW_PASS` |
| Team H — Infrastructure & DevOps | DevOps / Release | Compose/Caddy, image, CI, config, observability, release và upgrade runbook | `SYNVEIL_DEPLOYMENT_READY` |
| Team I — AI & Search | AI / Privacy | Python worker tùy chọn, OCR, extraction, embedding, search, provenance, privacy mode | `SYNVEIL_AI_FOUNDATION_READY` |
| Team J — Integrations & Clients | Integrations, Clients, Photos | Boundary Forgejo, desktop/Apple client, photo subsystem và platform contract | `SYNVEIL_CLIENT_CONTRACT_READY` |
| Team Q — QA & Reliability | QA / Reliability | Integration, protocol, E2E, property/fuzz, fault injection, recovery, benchmark | Review độc lập cho mọi gate |

### Team A — Architecture & Contracts

Team A sở hữu `docs/`, `api/openapi.yaml`, ADR, domain contract, cross-module
interface, error model, versioning rule và việc giải quyết dependency giữa các
team. Output critical gồm `ARCHITECTURE.md`, `DOMAIN_MODEL.md`,
`API_ARCHITECTURE.md`, `STORAGE.md`, `SYNC.md`, `BACKUP.md`, `SECURITY.md`,
`PLATFORM.md`, OpenAPI artifact đã review và ADR index. Platform / Distribution
own hai deployment mode, `PlatformRuntime`/service lifecycle, storage
capability, pairing, lớp remote access, health translation và progressive
disclosure. Team không tự cấp quyền cho native package hay hosted relay chỉ bằng
tài liệu. Team duy trì architecture, review thay đổi ID, object format, sync
semantic, API convention, error, authentication và database ownership. Team có
thể implement shared type, generated contract, contract validation hoặc
scaffolding, nhưng không implement toàn bộ backend.
`SYNVEIL_CONTRACTS_READY` đòi hỏi contract và fixture đã review; không cấp
quyền để team downstream phát minh protocol song song.

### Team B — Core Backend & API

Stack nền tảng là Rust, Axum, Tokio, Tower, Serde, SQLx và `tracing`. Team B sở
hữu `crates/api`, `crates/core`, `crates/metadata`, server bootstrap, HTTP
middleware, routing, application service, transaction orchestration,
pagination, configuration, versioning và health endpoint. Handler đi theo:

```text
HTTP → validation → application service → domain/storage/database
     → typed result → HTTP mapping
```

Handler phải thin, typed, testable và observable. AI inference, chi tiết
filesystem implementation, sync conflict policy và backup retention policy
không được nhét vào handler. `SYNVEIL_API_FOUNDATION_READY` đòi hỏi typed error
mapping, authorization hook, transaction boundary, health behavior và contract
test.

### Team C — Storage & Data Integrity

Team C sở hữu `crates/storage`, `crates/object-store`, `crates/uploads`,
`crates/versions`, `crates/compression` và `crates/dedup`, gồm port `ObjectStore`,
adapter local và S3-compatible về sau, object ID và key, safe write, integrity
verification, resumable upload, checksum, immutable version, restore, staging
cleanup, accounting, compression, whole-object dedup, GC, storage picker
integration, filesystem capability probe và portable fallback behavior. Golden
rule là:

```text
không publish metadata cho byte chưa durable và verify;
không để orphan hoặc reference không chắc chắn mà không có đường repair.
```

Evidence failure tối thiểu bao phủ disk full, process crash, checksum mismatch,
duplicate completion, mất HTTP response, temporary-file còn lại, DB transaction
failure, object-write failure và filesystem permission failure. Mọi maintenance
destructive hỗ trợ inspection và dry-run khi thực tế cho phép. Ba gate Team C
là các quyết định evidence riêng: `SYNVEIL_STORAGE_FOUNDATION_READY` cho
adapter và durable-object boundary, `SYNVEIL_UPLOADS_RELIABLE` cho resumable
completion, và `SYNVEIL_DATA_INTEGRITY_READY` cho reconciliation, verification
và protected reference.

### Team D — Sync Engine

Team D sở hữu `crates/sync`, change journal, cursor, conflict engine, protocol
và device sync state. Implementation đầy đủ chờ `SYNVEIL_DATA_INTEGRITY_READY`. Mọi create,
update, rename, move, delete và restore liên quan sync đều tạo committed fact
bền vững. Pull phải recover từ stale cursor; push phải conditional và
idempotent; mất response không được tạo mutation trùng. Conformance matrix bao
phủ create, rename, move, delete, restore, edit đồng thời, stale cursor,
duplicate request, server restart, reconnect và ít nhất 10.000 change xếp hàng.
Promotion đòi hỏi `SYNVEIL_SYNC_PROTOCOL_STABLE`.

### Team E — Backup & Recovery

Team E sở hữu `crates/backup`, backup set, source, snapshot, manifest, retention,
restore workflow, backup verification và backup health. Team bảo vệ bất biến backup và
sync là hai sản phẩm khác nhau:

```text
sync deletion có thể lan truyền deletion của current state;
backup deletion giữ snapshot lịch sử cho tới khi retention cho phép hết hạn.
```

Evidence bắt buộc bao phủ full/incremental backup, file unchanged/modified,
input bị xóa, device biến mất, restore file đơn và folder, snapshot restore và
clean instance-disaster recovery drill. Không được tuyên bố backup production
trước khi restore được verify độc lập. Promotion đòi hỏi
`SYNVEIL_BACKUP_RESTORE_READY`.

### Team F — Web Application

Web stack là React, TypeScript và Vite trong `apps/web`. Màn hình ban đầu gồm
login, dashboard, files, uploads, trash, versions, sharing, devices, backups và
settings; Photos, Code, Projects và AI Search theo gate tương ứng. Frontend dùng
generated/typed API contract và không sở hữu business logic chuẩn, object key,
conflict resolution hay backup semantic. Frontend render state do server định
nghĩa gồm `loading`, `empty`, `success`, `partial`, `offline`, `error`,
`conflict`, `uploading`, `paused` và `retrying`. UX / Accessibility sở hữu
plain-language user error, keyboard/screen-reader, progressive disclosure, core
flow không terminal, storage selection, pairing, health, update, uninstall,
migration và recovery surface. `SYNVEIL_WEB_CORE_READY` đòi hỏi UX
recovery/conflict accessible, API typed và test failure state.

### Team G — Security & Identity

Team G chạy từ phase đầu, sở hữu password hashing, login, session/token
lifecycle, credential thiết bị, revocation, authorization, public-share
security, rate limit, audit log và threat model. Team review authorization của
upload/download, object access, share, Git credential, AI-provider credential,
webhook, filesystem path và container secret. Attack class bắt buộc gồm IDOR,
path traversal, XSS, CSRF khi áp dụng, SQL injection, SSRF, oversized upload,
decompression bomb, malicious filename, token replay và brute force. Mỗi major
release cần `SECURITY_REVIEW_PASS`; feature không được bỏ qua vì là “internal”
hay chạy asynchronous.

### Team H — Infrastructure & DevOps

Team H sở hữu `deploy/`, Dockerfile, Compose profile, Caddy, CI, release
pipeline, observability plumbing, native installer/update architecture, service
adapter và developer environment. Compose/Caddy là topology tham chiếu
Advanced / Server; Personal / Home cần native/guided path có evidence riêng.
Installer / Updater sở hữu signed artifact verification, preflight, service
recovery, phối hợp PostgreSQL managed, uninstall bảo toàn data, reinstall,
migration và release channel. Networking / Connectivity sở hữu pairing
transport, proxy/TLS, lớp LAN/remote, relay boundary tùy chọn, air-gapped
behavior và connectivity diagnostic. Developer vẫn có local path có giới hạn,
thường là `docker compose up -d` khi implementation tồn tại, nhưng đây không
phải user support claim. CI phải bao phủ Rust format/lint/test, frontend
lint/typecheck/test, migration validation, integration test, docs link, secret,
dependency, image, installer fixture, service lifecycle và release-lab OS thật.
Production evidence gồm graceful shutdown, health/readiness, DB/storage backup,
mount validation, log, upgrade, signed artifact, secret, service privilege,
uninstall/migration preservation và container permission. Promotion đòi hỏi
`SYNVEIL_DEPLOYMENT_READY` cùng platform evidence của
`SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY`.

### Team I — AI & Search

Team I sở hữu `services/ai`, Python runtime tùy chọn, OCR, text extraction,
embedding, semantic search, AI tagging và AI indexing. AI luôn asynchronous:

```text
canonical file commit → durable event/job → AI worker → derived data có thể thay thế
```

Trình tự feature ban đầu là OCR/text extraction/embedding/document search, sau
đó photo embedding/tag/photo search, rồi code indexing và project-aware search.
Mode là `DISABLED`, `LOCAL` và `REMOTE` tường minh; remote không bao giờ tự bật.
File, sync, backup, download và restore cốt lõi vẫn hoạt động khi AI vắng mặt.
`SYNVEIL_AI_FOUNDATION_READY` đòi hỏi evidence no-egress ở mode
disabled/local, derived data đúng ACL, delete/rebuild, giới hạn tài nguyên và
review provider/privacy.

### Team J — Integrations & Clients

Team J — Stage 1 sở hữu boundary `crates/integrations/forgejo`: contract bên ngoài
core server. Tích hợp Forgejo kết nối Forgejo, inventory repository, báo
activity/health, backup repository/Git LFS và liên kết project; không viết lại
Git hoặc biến outage Forgejo thành outage storage core. Stage 2 là desktop target
Linux, Windows và macOS, nên dùng shared Rust sync core; Linux Server phục vụ
evidence host/service chứ không giả vờ là desktop shell. Stage 3 là mobile tương
lai Android, iPhone và iPad với boundary native cho background, filesystem,
notification, credential store và photo library; Apple work dùng Swift,
SwiftUI, FileProvider, PhotoKit, URLSession và Keychain trong giới hạn OS.
Desktop / Mobile sở hữu pairing, device lifecycle, sync/backup parity,
capability negotiation và unsupported state trung thực. Implementation client
lớn chờ `SYNVEIL_CLIENT_CONTRACT_READY`, gate đóng băng credential, capability,
sync/upload/backup behavior và platform fixture.

### Team Q — QA & Reliability

Team Q sở hữu `tests/integration`, `tests/e2e`, integration/protocol test,
reliability scenario, fault injection và performance benchmark, nhưng domain owner vẫn accountable cho behavior được
test. Test pyramid là:

```text
unit → integration → protocol → E2E → failure/recovery
```

Mất dữ liệu, corruption im lặng, conflict resolution sai, restore thất bại,
delete propagation sai và permission bypass ưu tiên hơn UI polish. QA còn sở hữu
real-OS matrix cho Windows, macOS, Linux Desktop và Linux Server; fixture
install/storage picker, service crash/reboot/sleep, signed update,
uninstall-bảo toàn-data, database managed, pairing/remote connectivity,
accessibility và machine migration. Bỏ qua scenario bắt buộc khiến task là
`PARTIAL`, không phải complete.

## Các gate bắt buộc chuẩn

Các token dưới đây là promotion gate chính xác của master plan. Đây là các token
duy nhất được dùng để cho phép phase lớn tiếp theo hoặc promote capability. Gate
là quyết định evidence do owner và reviewer ghi nhận, không phải tuyên bố tự tin
của agent. Trong repository hiện tại tất cả vẫn là gate tương lai; blueprint
không đánh dấu gate nào đã đạt.

```text
SYNVEIL_BLUEPRINT_COMPLETE
SYNVEIL_CONTRACTS_READY
SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY
SYNVEIL_FOUNDATION_READY
SYNVEIL_FILE_STORAGE_READY
SYNVEIL_UPLOADS_RELIABLE
SYNVEIL_DATA_LIFECYCLE_READY
SYNVEIL_SYNC_PROTOCOL_STABLE
SYNVEIL_BACKUP_RESTORE_READY
SYNVEIL_STORAGE_OPTIMIZED
SYNVEIL_DESKTOP_SYNC_READY
SYNVEIL_PHOTOS_FOUNDATION_READY
SYNVEIL_AI_SEARCH_READY
SYNVEIL_CODE_INTEGRATION_READY
SYNVEIL_APPLE_CLIENT_READY
```

Các workstream gate `SYNVEIL_API_FOUNDATION_READY`,
`SYNVEIL_STORAGE_FOUNDATION_READY`, `SYNVEIL_DATA_INTEGRITY_READY`,
`SYNVEIL_WEB_CORE_READY`, `SECURITY_REVIEW_PASS`,
`SYNVEIL_DEPLOYMENT_READY`, `SYNVEIL_AI_FOUNDATION_READY` và
`SYNVEIL_CLIENT_CONTRACT_READY` là evidence bắt buộc ở nơi workstream map hoặc
phase plan nêu. Chuỗi `SV-G*` chi tiết trong roadmap mở rộng chỉ là tham chiếu
nội bộ tới evidence bundle; không bao giờ thay thế các token master chính xác.

`SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` là gate tiếp theo sau documentation
blueprint hiện tại. Nó ghi nhận evidence đã review cho platform runtime/service
lifecycle dùng chung, filesystem capability boundary, hai deployment mode,
pairing/remote-access layer, progressive disclosure và first-class host matrix.
Nó không tuyên bố native installer, managed PostgreSQL package, desktop client,
mobile client hay hosted relay đã tồn tại.

Để bàn giao không mơ hồ, các checkpoint mở rộng được map như sau:

| Tham chiếu evidence mở rộng | Ý nghĩa promotion trong master plan |
|---|---|
| `SYNVEIL_CROSS_PLATFORM_FOUNDATION_READY` | Ghi nhận boundary contract/evidence Phase 0A và là prerequisite của `SYNVEIL_FOUNDATION_READY`. |
| `SV-G0-FOUNDATION` | Đóng góp cho `SYNVEIL_FOUNDATION_READY`; `SYNVEIL_BLUEPRINT_COMPLETE`, `SYNVEIL_CONTRACTS_READY` và cross-platform gate vẫn là prerequisite riêng. |
| `SV-G1-STORAGE` | Đóng góp cho `SYNVEIL_FILE_STORAGE_READY` sau `SYNVEIL_STORAGE_FOUNDATION_READY`. |
| `SV-G2-DATA-SAFETY` | Đóng góp cho `SYNVEIL_UPLOADS_RELIABLE` và `SYNVEIL_DATA_INTEGRITY_READY`. |
| `SV-G3-TRUSTED-ACCESS` | Đóng góp cho `SYNVEIL_DATA_LIFECYCLE_READY` khi version, Trash, sharing và device authorization đã tích hợp. |
| `SV-G4-SYNC-CONFORMANT` | Đóng góp cho `SYNVEIL_SYNC_PROTOCOL_STABLE`. |
| `SV-G5-RESTORABLE` | Đóng góp cho `SYNVEIL_BACKUP_RESTORE_READY`. |
| `SV-G6-OPTIMIZED-SAFELY` | Đóng góp cho `SYNVEIL_STORAGE_OPTIMIZED`. |
| `SV-G7-DESKTOP-REFERENCE` | Đóng góp cho `SYNVEIL_DESKTOP_SYNC_READY`. |
| `SV-G8-PHOTOS-SAFE` | Đóng góp cho `SYNVEIL_PHOTOS_FOUNDATION_READY`. |
| `SV-G9-APPLE-CONTRACT-PROTOTYPE` | Chỉ là prototype evidence; không promote `SYNVEIL_APPLE_CLIENT_READY`. |
| `SV-G10-AI-OPTIONAL` và AI evidence cuối | Đóng góp cho `SYNVEIL_AI_SEARCH_READY`; `SYNVEIL_AI_FOUNDATION_READY` là workstream gate. |
| `SV-G11-FORGEJO-RESTORABLE` và `SV-G12-CODE-INTELLIGENCE` | Cùng đóng góp cho `SYNVEIL_CODE_INTEGRATION_READY`. |
| Evidence Apple Phase 12B | Promote `SYNVEIL_APPLE_CLIENT_READY` chỉ sau các prerequisite master đã nêu. |

Mapping này cho phép coordinator giữ vocabulary evidence chi tiết mà không
nhầm checkpoint nội bộ thành authorization của master plan.

## Quy tắc branch, commit, pull request và database

### Chiến lược branch

`main` luôn buildable và là integration branch dài hạn duy nhất. Feature dùng
branch ngắn hạn như `feat/storage-object-store`, `feat/resumable-upload`,
`feat/sync-change-journal` hoặc `feat/web-file-browser` (thêm prefix mà hosting
workflow yêu cầu). Không duy trì branch team vĩnh viễn. Dùng worktree cô lập
khi hai agent cần path không liên quan và nêu rõ integration owner cho file
contract dùng chung.

### Chính sách commit

Commit phải có scope và review được, ví dụ:

```text
storage: add atomic object writer
sync: add monotonic change sequence
web: implement upload queue
docs: define backup retention semantics
```

Không trộn thay đổi AI, database, UI và sync không liên quan vào một commit.
Thay đổi contract tiếng Anh/tiếng Việt đi cùng một documentation change set;
ghi artifact generated và revision nguồn sinh ra nó. Không rewrite migration
hoặc stored format đã release chỉ để lịch sử trông sạch hơn.

### Yêu cầu pull request

Mọi PR có ý nghĩa phải nêu:

```text
Objective
Scope
Architecture impact
API impact
Database impact
Security impact
Tests
Migration impact
Rollback considerations
```

PR storage, sync và backup phải nêu thêm data-loss risk, crash behavior và retry
behavior. PR xác định ADR/spec bị ảnh hưởng, gate evidence, status change,
documentation parity và limitation đã biết. Reviewer reproduce critical
evidence hoặc ghi rõ vì sao không thể chạy.

### Quy tắc ownership database

Chỉ có một migration sequence chuẩn và một migration integrator được chỉ định.
Không team nào tự tạo migration number cạnh tranh hoặc tự đổi cùng một
relationship. Thứ tự bắt buộc là:

```text
domain proposal → architecture review → migration → domain/OpenAPI update
→ implementation → compatibility/recovery tests
```

Migration đã release là bất biến và forward-only. Backfill có giới hạn, quan
sát được, resumable và có rollback awareness; không dùng SQL production thủ
công thay cho migration đã review. Database ownership không cho phép team đổi
domain semantic do Architecture hoặc protocol team liên quan sở hữu.

### Quy tắc đổi protocol

Mọi thay đổi breaking tới sync, upload, authentication, object addressing hoặc
backup format cần ADR, compatibility analysis, migration strategy, fixture và
test. Client có thể offline nhiều tuần hoặc tháng, nên server phải định nghĩa
mixed-version behavior, capability negotiation, stale-cursor recovery và
reader-before-writer rollout. Task không được âm thầm đổi wire enum, cursor
meaning, error code, credential scope hoặc object format.

## Definition of Done và thứ tự ưu tiên khi xung đột

“Works on my machine” không phải tiêu chí hoàn tất. Feature hướng production
chỉ done khi implementation, unit test, integration test, failure/recovery
case liên quan, API docs, docs tiếng Anh, docs tiếng Việt, security review,
log/metrics, migration verification, backward-compatibility analysis và review
critical TODO đều đạt. Required test bị skip hoặc critical risk chưa xử lý
khiến công việc là `PARTIAL` hoặc `BLOCKED`.

Khi feature scope xung đột với safety, chọn theo thứ tự:

```text
1. ngăn mất dữ liệu
2. ngăn vi phạm security
3. giữ data integrity
4. giữ protocol compatibility
5. giữ sync/backup behavior đúng
6. reliability
7. performance
8. UX
9. advanced features
```

AI và UI đẹp không bao giờ xếp trên integrity, authorization hoặc khả năng
recover.

## Contract giao tiếp của team

Mọi completion report dùng cấu trúc này; chỉ ghi “Done” là không hợp lệ:

```text
Verdict
Workstream
Prerequisite gate
Files changed
Architecture decisions
API changes
Database changes
Security impact
Tests executed
Failures / limitations
Open decisions
Next recommended gate
```

Báo cáo mở rộng có thể thêm performance, privacy, durability, compatibility,
environment, migration và rollback evidence, nhưng phải giữ các trường trên.
Báo cáo phân biệt `COMPLETE`, `PARTIAL` và `BLOCKED`; không claim phase gate
trước khi integration owner và reviewer ghi nhận evidence.

## Phân bổ khuyến nghị và trình tự implementation đầu tiên

Master plan thu nhỏ mà không bỏ boundary ownership:

| Quy mô | Phân bổ |
|---|---|
| 2 developer | Developer 1: Architecture + Platform + Rust + Storage + Sync. Developer 2: Web/Accessibility + DevOps/Installer + QA + Security. AI và integrations về sau. |
| 3–4 developer | Platform/Architecture; Backend/API; Storage + Sync + Backup; Web/UX; DevOps/Installer + Security + QA. |
| 5–7 developer | Platform/Architecture/API; Storage; Sync/Backup; Web/Accessibility; DevOps/Installer/Connectivity; Security/QA; AI/Integrations. |
| Multi-agent | Agent Platform architect, Backend, Storage Platform, Sync, Web/Accessibility, Installer/Connectivity, Security và QA với scope cô lập; platform contract đi trước. |

Sau `SYNVEIL_BLUEPRINT_COMPLETE` và các contract/foundation gate tương ứng,
phát từng task implementation có giới hạn theo dependency sequence:

1. platform/runtime/storage-capability contract và fixture đa nền tảng;
2. repository foundation và Rust workspace;
3. distribution/service/update/uninstall/migration/recovery evidence;
4. PostgreSQL và migration foundation;
5. authentication foundation;
6. ObjectStore abstraction và local adapter;
7. file/directory metadata model;
8. streaming upload/download;
9. resumable upload protocol;
10. integrity và atomic completion;
11. versioning và Trash;
12. sharing;
13. device model và pairing;
14. change journal;
15. sync protocol;
16. conflict resolution;
17. backup model;
18. snapshots và restore;
19. compression;
20. whole-object deduplication;
21. web core accessible và progressive-disclosure recovery UX;
22. desktop, mobile, Photos, AI và Code integrations theo gate.

Có thể chia sequence thành task nhỏ hơn, nhưng task sau không được coi contract
chưa ổn định là đã freeze. Prototype phase sau phải dùng dữ liệu synthetic hoặc
format disposable rõ ràng và không được tự động promote.

## Quy tắc không bao giờ được phá

Không subsystem nào được thực hiện mutation không thể đảo ngược chỉ vì tự quyết
định cục bộ rằng mutation có vẻ đúng. Với GC, dedup cleanup, retention
deletion, storage migration, repair và thao tác tương tự, dùng:

```text
inspect → plan → validate → execute → verify
```

Trước destructive execution phải có dry-run khi thực tế cho phép. Plan phải
nêu rõ reference, lease, retention/legal hold, authorization, giới hạn
rollback, audit record và verification evidence. Nếu inspection hoặc
validation không chắc chắn, dừng lại và giữ dữ liệu.

## Mục tiêu cuối cùng của team

Mục tiêu không phải tối đa số feature hay tốc độ code. Mục tiêu là xây Synveil
thành self-hosted system mà user có thể an tâm giao dữ liệu cá nhân không thể
thay thế. Một Synveil nhỏ hơn nhưng có storage, sync, backup và restore đáng tin
cậy có giá trị hơn hệ thống nhiều tính năng nhưng có thể âm thầm mất hoặc làm
corrupt dữ liệu.

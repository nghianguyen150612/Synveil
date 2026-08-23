# Danh mục tính năng Synveil

Trạng thái: **Blueprint sản phẩm PLANNED**

Danh mục này xác định phạm vi sản phẩm dự kiến; đây không phải tuyên bố rằng
bất kỳ khả năng nào đã dùng được. Repository có foundation scaffolding nhưng
chưa có implementation tính năng sản phẩm. Một tính năng vẫn giữ trạng thái `PLANNED` hoặc
`EXPERIMENTAL` cho đến khi gate bằng chứng trong
[Đóng góp vào kiến trúc Synveil](CONTRIBUTING_ARCHITECTURE.md) cho phép thay đổi
trạng thái sau review.

## Cách đọc danh mục

Các từ chỉ trạng thái có tính quy chuẩn:

| Label | Ý nghĩa tại đây |
|---|---|
| `PLANNED` | Đã được dự kiến và giới hạn bằng hợp đồng, nhưng chưa triển khai. |
| `EXPERIMENTAL` | Nghiên cứu hoặc bề mặt opt-in chưa ổn định, không cam kết tương thích. |
| `NON-GOAL` | Cố ý nằm ngoài phạm vi thời gian đã nêu. |

`Core`, `Near-term`, `Advanced` và `Experimental/Future` mô tả dependency và
độ ưu tiên sản phẩm, không phải mức độ sẵn có hiện tại. Các phase roadmap,
prerequisite và exit gate nằm trong [ROADMAP.md](ROADMAP.md); owner triển khai
sử dụng [TEAM_PLAN.md](TEAM_PLAN.md).

Các quy tắc sau áp dụng cho mọi tính năng:

1. Tính đúng đắn của file, sync, backup, version và restore lõi không phụ thuộc
   vào AI, tạo thumbnail, OCR, làm giàu search, polling repository hay worker
   tùy chọn khác.
2. PostgreSQL là nguồn có thẩm quyền cho metadata và trạng thái giao dịch. Byte
   file chuẩn là các giá trị `Object` bất biến được lưu bởi object-store adapter.
3. Identity người dùng nhìn thấy tuân theo
   `Library → Node → FileVersion → Object` như quy định trong
   [DOMAIN_MODEL.md](DOMAIN_MODEL.md).
4. Mọi mutation client nhìn thấy đều được authorize từ principal đã xác thực và
   quan hệ resource, được áp dụng có điều kiện, retry an toàn theo hợp đồng
   idempotency đã ghi và được đưa vào journal khi làm thay đổi một `Library`.
5. Backup snapshot là manifest lịch sử chịu sự kiểm soát của retention. Input
   backup bị thiếu không bao giờ có nghĩa là “xóa live cloud node”.
6. Dữ liệu dẫn xuất tùy chọn có thể thay thế. Việc mất dữ liệu này có thể làm
   suy giảm một tính năng nhưng không được làm mất, sửa đổi hoặc khiến byte
   chuẩn không thể truy cập.

## Khả năng lõi

Các khả năng lõi thiết lập một nền tảng dữ liệu self-hosted an toàn. Mọi hạng
mục dưới đây đều là `PLANNED`.

### Identity, xác thực và tài khoản

- Bootstrap chính xác một administrator ban đầu qua flow setup một lần, cục bộ
  theo deployment; đóng bootstrap hoặc re-arm rõ ràng sau khi thành công.
- Tạo và quản trị tài khoản người dùng mà không tự phát minh mật mã. Password
  dùng cơ chế password hashing memory-hard đã được review với tham số có
  version.
- Đăng nhập và đăng xuất; cấp, rotate, refresh, revoke và đặt hết hạn session mà
  không lưu bearer credential dạng plaintext.
- Đăng ký credential riêng cho thiết bị, hiển thị hoạt động last-seen/security
  và revoke từ xa credential Synveil của thiết bị.
- Cung cấp truy cập API đã xác thực, rate limit theo principal, account recovery
  với policy rõ ràng cho operator self-hosted và security audit event.
- Chỉ thêm MFA sau khi hành vi recovery, enrollment, revocation và anti-lockout
  đã được thiết kế và threat-review.

Hệ thống không tuyên bố có thể xóa sạch hệ điều hành. Revocation thiết bị chỉ
có thể vô hiệu hóa credential và yêu cầu xóa cache do Synveil quản lý. Hợp đồng
security và trust boundary thuộc [SECURITY.md](SECURITY.md).

### File, folder và metadata

- Upload và stream-download file, gồm HTTP byte range, mà không buffer toàn bộ
  object lớn trong memory của API hoặc worker.
- Tạo folder; liệt kê child bằng keyset pagination ổn định; đồng thời filter,
  sort, search, select hoặc thao tác trên nhiều node trong giới hạn hữu hạn.
- Rename, move và copy logical node. Rename và move cập nhật metadata thay vì di
  chuyển nội dung bất biến nhiều gigabyte.
- Giữ MIME type dưới dạng metadata không đáng tin cậy, byte size, timestamp tạo
  và sửa, filename do client cung cấp, trạng thái toàn vẹn và version identity.
- Soft-delete vào trash, restore và cuối cùng purge theo quy tắc retention và
  storage accounting rõ ràng. Thao tác đệ quy chạy bất đồng bộ hoặc được giới
  hạn theo cách khác, thay vì một giao dịch không giới hạn.
- Hỗ trợ drag-and-drop trên browser như một hành vi client sử dụng cùng upload
  protocol, không phải storage path riêng.

Tên là display data không đáng tin cậy. Tên không bao giờ trở thành object-store
key hay filesystem path trên server nếu không qua encoding và validation do
adapter sở hữu. Hành vi storage chính xác được quy định trong
[STORAGE.md](STORAGE.md).

### Logical object, tính toàn vẹn và upload đáng tin cậy

- Giữ `Node`, `FileVersion` bất biến và `Object` bất biến tách biệt.
- Hỗ trợ local filesystem, filesystem gắn qua NAS, MinIO và backend
  S3-compatible khác phía sau một hợp đồng object-store; chỉ thêm adapter tương
  lai khi có conformance test và recovery test.
- Khởi tạo `UploadSession` có thể resume, chấp nhận các part có thể retry độc
  lập, verify length và checksum đã khai báo, assemble hoặc finalize bền vững
  rồi expose một version mới theo cách nguyên tử.
- Làm hết hạn và thu hồi staging data bị bỏ dở mà không chạm vào object đã
  commit.
- Dùng SHA-256 làm hash toàn vẹn plaintext chuẩn ban đầu và hash deduplication
  whole-object. Storage key vẫn mờ đục; không để lộ hash và user path qua key.
- Xử lý disk-full, hết quota, checksum mismatch, process crash, complete trùng,
  mất success response và trường hợp object write thành công rồi database
  commit thất bại.

Chỉ object bền vững đã verify mới được một content commit thành công tham chiếu.
Giao dịch database đó cũng append library change, audit fact và durable outbox
work. Chi tiết protocol nằm trong [UPLOADS.md](UPLOADS.md).

### Version, trash và recovery

- Tạo `FileVersion` bất biến mới khi thay nội dung; không bao giờ rewrite version
  cũ tại chỗ.
- Duyệt version trước, restore version trước bằng cách tạo head mới, đồng thời
  giữ source attribution và timestamp.
- Chỉ dùng chung byte vật lý giữa các version trong cùng deduplication domain
  và không làm yếu authorization.
- Áp dụng quy tắc retention độc lập cho version và trash. Purge logical
  reference không xóa byte vật lý dùng chung cho đến khi không còn reference
  live, historical, backup, derivative hoặc staging nào và safety window đã
  qua.
- Quarantine object hỏng, hiển thị các version bị ảnh hưởng và hỗ trợ restore
  có verify thay vì âm thầm phục vụ byte đã biết là hỏng.

### Đồng bộ nhiều thiết bị

- Cấp cho mỗi `Library` một sequence thay đổi có thứ tự, tăng theo giao dịch và
  một `SyncCursor` mờ đục, có version.
- Đồng bộ các fact create, content modify, rename, move, trash, restore và purge
  qua `GET /api/v1/libraries/{library_id}/changes?cursor=...`.
- Yêu cầu precondition base version hoặc ETag cho write. Response bị mất có thể
  được replay bằng idempotency key và trả về outcome ban đầu.
- Giữ cả hai kết quả của chỉnh sửa nội dung offline đồng thời. Write hợp lệ đầu
  tiên đẩy head tiến lên; content write stale trở thành conflict copy xác định
  theo ADR-006, không bao giờ là ghi đè im lặng.
- Phát hiện cursor hết hạn, malformed, sai Library hoặc sai epoch. Khi đó client
  thực hiện authoritative rescan có pagination và tiếp tục từ server checkpoint
  mà không suy ra thứ tự từ UUID hoặc timestamp.
- Xác định hành vi tất định cho directory move đồng thời, ngăn cycle, name
  collision, delete so với edit, restore so với purge và retry sau khi client áp
  dụng một phần.

Các policy dự kiến là two-way sync, upload-only backup, download-only mirror,
cloud-only, pinned locally và excluded. “Upload-only backup” gọi ngữ nghĩa
backup; đây không phải sync mode lan truyền deletion. Quy tắc participation và
local materialization có thể được scope theo device, `Library`/directory
subtree, file type và lựa chọn rõ ràng của user, trong khi protocol vẫn giao
metadata và tombstone cần thiết để hội tụ đúng khi filter thay đổi.

### Backup, snapshot và restore

- Xác định các giá trị `BackupSet` theo thiết bị cho source được chọn, capture
  theo lịch hoặc liên tục, exclusion và retention.
- Xây manifest `BackupSnapshot` bất biến và chỉ expose snapshot đã đạt
  `COMMITTED` sau khi toàn bộ object được tham chiếu cùng tính toàn vẹn manifest
  đã được verify.
- Giữ entry lịch sử khi input biến mất theo policy. Sự vắng mặt được ghi là một
  backup observation, không phát thành deletion của `Node` live.
- Restore một file, directory subtree, file version trước hoặc snapshot hoàn
  chỉnh đến destination rõ ràng; không overwrite dữ liệu mới hơn khi chưa có
  precondition được review và lựa chọn của người dùng.
- Recovery sau khi mất hoặc revoke source device, đồng thời verify byte count đã
  restore, canonical hash, membership trong manifest và entry bị skip/conflict.
- Giới hạn scan bị gián đoạn, submission lặp, tái sử dụng file không đổi, xử lý
  file lớn đã đổi, quota failure, xử lý object hỏng và retention cleanup.

[BACKUP.md](BACKUP.md) sở hữu snapshot và restore protocol. Backup data có thể
tái sử dụng giá trị `Object` trong deduplication domain được phép, nhưng trạng
thái sync và trạng thái snapshot vẫn tách biệt.

### Quản lý thiết bị

- Đăng ký thiết bị với user đã xác thực, ID mờ đục ổn định, display label, khai
  báo platform/capability và credential có thể revoke độc lập.
- Hiển thị lần contact thành công gần nhất, trạng thái credential, library
  policy được gán, sync checkpoint, trạng thái backup và trạng thái
  storage/cache mà không coi client claim là security fact đáng tin cậy.
- Pause sync hoặc backup theo policy, resume an toàn, hiển thị lag và error, giữ
  lịch sử hoạt động có thể audit.
- Revoke ngay mọi server credential của một device và tùy chọn queue request
  best-effort chỉ để xóa local cache do Synveil quản lý. Request có thể không bao
  giờ đến được client offline/bị compromise và không phải tuyên bố remote wipe
  hệ điều hành.
- Negotiate protocol và capability version để client thiếu native placeholder,
  background execution hoặc tính năng khác nhận policy tương thích.

### Search tất định và vận hành

- Search filename và typed metadata không cần AI; thêm full-text search cho text
  đã extract hoặc index rõ ràng khi parser và access control qua gate.
- Giữ semantic search, photo search và code search thành các lớp tùy chọn riêng,
  đồng thời ghi label provenance của kết quả.
- Expose structured log, request ID, trace, metric, liveness, readiness,
  database health, storage health và worker lag trong khi loại secret và nội
  dung file.
- Hỗ trợ Docker Compose như một production topology, với Caddy là edge proxy tùy
  chọn thay vì dependency nội bộ.
- Backup và restore metadata cùng object storage của Synveil tại compatibility
  point đã ghi và đã test trước khi upgrade.

### Installation, onboarding, health và recovery

Sản phẩm lõi phải dễ tiếp cận mà không làm nhỏ hơn correctness boundary:

- Cung cấp guided Personal / Home installation cho Windows, macOS, Linux
  Desktop và Linux Server khi từng platform đạt release gate của nó.
- Cho phép user không chuyên chọn storage location, xem capacity/health, tạo
  account đầu tiên, pair device và hiểu nghĩa vụ backup/recovery mà không phải
  tự cấu hình PostgreSQL, SQL, Compose, reverse proxy hoặc secret dạng
  environment variable.
- Giữ Advanced / Server path với Compose, PostgreSQL external, proxy/TLS tùy
  chỉnh, NAS/S3, CLI và operator diagnostics nhưng dùng chung API, data model và
  storage correctness rule.
- Dịch stable error thành next action cho user thông thường, đồng thời giữ
  administrator/developer diagnostics sau progressive disclosure.
- Cung cấp workflow update, uninstall, reinstall, machine migration và recovery
  bảo toàn data. Xóa application và xóa vĩnh viễn data không bao giờ là cùng một
  action.
- Dùng một pairing flow authenticated, sống ngắn cho browser, desktop và mobile
  tương lai. Remote access là tùy chọn và self-hosted-first; hosted relay không
  phải core dependency.
- Coi automatic maintenance, service recovery, signed update, storage
  capability discovery và health check là product behavior, không phải folklore
  operator không được ghi lại.

## Khả năng sản phẩm near-term

Các hạng mục này là `PLANNED` sau nền tảng storage.

### Sharing

- Share riêng tư với user khác dưới dạng read-only hoặc writable, tùy thuộc
  policy của resource owner và effective permission của recipient.
- Tạo public link có thể revoke, có scope, expiration, password tùy chọn và
  abuse limit. Lưu password verifier, không bao giờ lưu plaintext password.
- Audit việc create, access, đổi permission, failed password attempt và
  revocation mà không log share secret.
- Từ chối nỗ lực insecure direct object reference: chỉ sở hữu ID `Node`,
  `Object` hoặc `Share` không trao quyền truy cập.
- Revoke quyền truy cập ngay tại thời điểm authorization. Không thể thu hồi bằng
  mật mã byte đã cache hoặc download; UI không được ngụ ý điều ngược lại.

### Tối ưu storage whole-object

- Chỉ áp dụng nén Zstandard theo policy cho nội dung đã đo và đủ điều kiện như
  text, JSON, CSV, log, database dump và source code.
- Bỏ qua format vốn đã nén, mã hóa hoặc không được hỗ trợ, trừ khi benchmark
  chứng minh lợi ích hữu hạn.
- Ghi codec, codec version/parameter, encoded length, plaintext length và
  plaintext hash để decompression minh bạch và có thể verify.
- Chỉ deduplicate canonical plaintext giống nhau trong một ownership/
  deduplication domain. Không để lộ tính bằng nhau giữa user qua timing, quota,
  error hoặc object ID.
- Hạch toán logical byte, retained byte và physical byte riêng; không bao giờ
  hứa “saved space” của user dựa trên private data của user khác.

Compression đứng trước server-side application encryption khi dùng cả hai.
Không đồng thời giả định zero-knowledge encryption, cross-user deduplication,
server-side preview và server-side AI.

### Photos

- Ingest resource image và video original qua durable upload path thông thường,
  sau đó extract metadata an toàn và tạo derivative bất đồng bộ.
- Cung cấp timeline, album, favorite, video, phân loại screenshot, search,
  suggestion duplicate exact-byte và trạng thái backup theo thiết bị.
- Coi suggestion perceptual near-duplicate là `EXPERIMENTAL`, derived data có
  consent riêng không thể xóa, merge hay đổi retention.
- Giữ EXIF làm original metadata, đồng thời coi location, face và capture
  context là sensitive derived field.
- Nhóm resource kiểu Live Photo mà không giả vờ mọi client hoặc format có cùng
  representation.
- Chuẩn bị hợp đồng PhotoKit và background transfer cho Apple client tương lai,
  gồm limited-library access, source identifier theo thiết bị, retry và giới
  hạn scheduling của iOS.

Không photo processor nào được rewrite canonical original. Chi tiết và privacy
control nằm trong [PHOTOS.md](PHOTOS.md).

## Khả năng nâng cao

Các hạng mục nâng cao là `PLANNED`, nhưng không được chặn release sớm.

### Files on demand và smart sync

- Biểu diễn trạng thái logic phía server mà client cần để ánh xạ `Local`,
  `Cloud-only`, `Pinned`, `Downloading`, `Uploading`, `Conflict` và
  `Unavailable`.
- Cho phép desktop client có năng lực hoặc Apple FileProvider client hydrate
  theo version bất biến và chỉ evict local cache đã verify một cách an toàn.
- Làm rõ policy pin và exclusion theo thiết bị. Không bao giờ suy ra local cache
  bị evict là server deletion.
- Negotiate capability vì Linux, Windows, macOS, iOS và web không expose
  placeholder API giống hệt nhau.

### Deduplication cấp chunk

- Khảo sát content-defined chunking, immutable chunk được index bằng hash và
  manifest có version cho VM image, dataset, game asset và backup lớn lặp lại.
- Giữ hợp đồng whole-object chuẩn hợp lệ để client và restore không phụ thuộc
  một chunker cụ thể.
- Giới hạn chunk count, manifest size, collision verification, memory,
  random-read amplification, garbage collection và format migration.
- Chỉ đưa stored chunk format vào qua ADR đã chấp thuận và kế hoạch tương thích
  readers-before-writers.

### Desktop client và Apple client

- Tái sử dụng Rust sync core đã được review trên desktop client nếu thí nghiệm
  platform boundary và FFI chứng minh hợp lý; giữ native placeholder và
  credential integration riêng theo platform.
- Lập kế hoạch selected-folder sync, selected-folder backup, tray/status UX,
  bandwidth control và recovery cho Linux, Windows và macOS.
- Lập kế hoạch Swift, SwiftUI, FileProvider, PhotoKit, URLSession, Keychain và
  Swift Concurrency cho Apple client.
- Để dành client Android, iPhone và iPad tương lai cho cùng protocol và device/
  pairing model, với boundary riêng theo OS cho background, filesystem,
  notification, credential store và photo library. Mobile chưa phải yêu cầu của
  cross-platform foundation gate đầu tiên.
- Xác thực, đăng ký thiết bị, chọn policy, thực hiện initial snapshot rồi consume
  ordered change mà không thiết kế lại server.

### Tích hợp code và project

- Kết nối Forgejo trước tiên; liệt kê repository và metadata, tóm tắt
  branch/tag/recent commit, báo cáo health cùng storage use và liên kết
  repository với workspace `Project` tùy chọn.
- Backup và verify repository Git data, Git LFS object và release artifact được
  hỗ trợ rõ ràng; restore đến destination an toàn mà mặc định không overwrite
  repository live.
- Giữ Git smart HTTP/SSH, packfile, ref, permission, pull request và issue thuộc
  quyền sở hữu của Forgejo.
- Cho phép `Project` liên kết repository, document, asset, backup, device và
  metadata liên quan mà không thay ownership hoặc deletion lifecycle gốc.

Ranh giới integration và hành vi khi Forgejo không sẵn có được xác định trong
[CODE_INTEGRATION.md](CODE_INTEGRATION.md).

### Nền tảng AI tùy chọn

- OCR PDF, screenshot, scan và photo phù hợp; lưu extracted text tách khỏi byte
  chuẩn.
- Tạo embedding gắn version cho chunk dẫn xuất từ text/code/OCR đã authorize,
  gồm OCR text từ photo, repository/code và scope project, đồng thời cung cấp
  semantic retrieval như lớp tùy chọn. Vision/image embedding vẫn experimental.
- Đề xuất automatic tag có thể chỉnh sửa từ text/code/OCR và deterministic
  metadata đã review với provenance `USER`, `SYSTEM` hoặc `AI`. Tag
  scene/object/person do vision tạo vẫn `EXPERIMENTAL`.
- Hỗ trợ inference mode `DISABLED`, local/self-hosted và remote-provider có
  consent rõ ràng. Nội dung nhạy cảm không bao giờ âm thầm rời server.
- Expose source/model provenance, freshness, reindex, consent withdrawal và xóa
  derived data mà không đổi canonical retention.

Các khả năng Phase 10 này là tính năng nâng cao `PLANNED`. AI gặp lỗi, bị tắt,
reindex hoặc xóa index vẫn để upload, download, sync, backup, restore và search
tất định hoạt động. Hợp đồng đầy đủ nằm trong [AI.md](AI.md).

### Smart storage tiering

- Bắt đầu bằng rule policy `HOT`, `WARM`, `COLD` và `ARCHIVE` có thể giải thích,
  dựa trên lựa chọn rõ ràng của user, access time, size, device free space,
  network state và file type.
- Coi placement là replica transition bất đồng bộ. Không đánh dấu source replica
  có thể xóa cho đến khi target bền vững và đã verify.
- Giữ một copy có thể tiếp cận và restore path khi backend outage, migration một
  phần hoặc policy thay đổi.

## Khả năng thử nghiệm và tương lai

Các bề mặt này là `EXPERIMENTAL` trừ khi được nâng cấp qua ADR và phase gate.

### Thí nghiệm AI và repository intelligence

- Đánh giá generated summary repository có citation, repository Q&A và câu trả
  lời natural-language nhận biết project chỉ trên dữ liệu caller hiện có quyền
  truy cập. Semantic code indexing/retrieval gắn version tự nó là nền tảng nâng
  cao `PLANNED` ở trên.
- Đánh giá image caption, hiểu face/scene và recommendation dựa trên model như
  các derived feature có consent riêng.
- Chỉ thêm provider/model adapter sau khi gate về license, privacy, deletion,
  resource, quality và prompt injection đã qua.
- Giữ generated answer ở trạng thái read-only và không có tool; mọi agent gây
  side effect cần ADR và authorization design riêng.

### Phát hiện bất thường

- Phát hiện pattern phá hủy bất thường như một đợt rewrite hoặc delete; giữ bằng
  chứng, đề xuất pause propagation và yêu cầu review.
- Dùng heuristic hữu hạn trước machine learning và expose observation kích hoạt.
- Không bao giờ tuyên bố phát hiện ransomware hoàn hảo hay chặn vĩnh viễn công
  việc hợp lệ khi không có recovery path cho operator.

### Scale-out và provider tương lai

- Chỉ thêm GitHub, GitLab, Gitea, remote AI provider, object store bổ sung hoặc
  distributed worker phía sau hợp đồng hiện có.
- Chỉ cân nhắc API replica ngang, message broker, Kubernetes và vận hành
  multi-node sau khi load đo được hoặc yêu cầu availability chứng minh chi phí
  vận hành là hợp lý.

## Kiến trúc thông tin web dự kiến

Ứng dụng React/TypeScript/Vite đã xác thực là `PLANNED` để expose:

```text
Login
Dashboard
Files
  My Drive | Shared | Recent | Favorites | Trash
Photos
  Timeline | Albums | Search
Backups
  Devices | Snapshots | Restore | Policies
Devices
Code
  Repositories | Projects | Git Servers
Search
Activity
Settings
  Account | Storage | Security | Integrations | AI
```

Files → Favorites dùng relation `NodeFavorite` theo từng user. Đây không phải
metadata node dùng chung, không bao giờ cấp access hay giữ content, và ẩn node
mà caller không còn quyền đọc.

Files → Shared lấy dữ liệu từ collection received share đã lọc authorization
cho caller; user không cần biết trước share ID hoặc node ID, và grant
revoked/expired biến mất mà không làm lộ ancestor không truy cập được.

Files → Recent là list có snapshot bound của node hiện đọc được, ordered theo
thời gian mutation server đã commit. Nó không ghi preview/download thành lịch
sử xem cá nhân ẩn và bỏ ngay content bị trash hoặc revoke.

Các page phải hiển thị trung thực trạng thái capability và degradation. Một
navigation shell hoặc mock không nâng tính năng thành `IMPLEMENTED`.

## Hợp đồng lỗi xuyên tính năng

| Lỗi | Hành vi sản phẩm bắt buộc |
|---|---|
| Object store không sẵn có | Từ chối content commit mới bằng stable retryable error; không tạo version nhìn thấy được mà tham chiếu byte bị thiếu. Metadata read không cần byte có thể tiếp tục nếu an toàn. |
| Object bền vững nhưng DB commit thất bại | Giữ object không được tham chiếu và đủ điều kiện cho delayed orphan reconciliation; trả về failure. Không bao giờ expose version `Node` từ giao dịch chưa commit. |
| DB commit thành công nhưng response bị mất | Retry với cùng idempotency key trả về outcome đã commit và không được tạo thêm version hoặc journal entry. |
| Disk đầy trong staging | Fail hoặc pause upload bị ảnh hưởng, giữ object đã commit, báo quota/disk health và chỉ thu hồi staging data đã validation. |
| Optional worker offline | Core write thành công sau khi ghi durable outbox; derived state thành `PENDING` hoặc `STALE` và worker lag nhìn thấy được. |
| PostgreSQL không sẵn có | Từ chối mutation thay vì ghi canonical object không được theo dõi như user data thành công. |
| Sync cursor stale | Trả stable cursor error cùng rescan path do server chỉ định; không bao giờ đoán từ client timestamp. |
| Backup source bỏ qua path trước đây | Áp dụng snapshot/retention policy; không phát live deletion. |
| Forgejo không sẵn có | Đánh dấu integration data là stale, retry có giới hạn và giữ core storage cùng repository backup hiện có đã verify dùng được. |
| AI bị tắt hoặc remote consent bị rút | Dừng dispatch tới mode đó, revoke queued remote work khi có thể và cho phép xóa/rebuild derived record; canonical data vẫn dùng được. |

## Non-goal rõ ràng cho giai đoạn sớm

Các hạng mục sau là `NON-GOAL` cho các version đầu:

- backup toàn bộ iPhone hoặc hệ điều hành;
- đồng bộ iMessage;
- giải pháp thay thế GitHub/Forgejo hoặc custom Git transport;
- office suite hoặc chat platform;
- nền tảng media transcoding đầy đủ;
- enterprise IAM suite;
- distributed filesystem Kubernetes-native;
- zero-knowledge encryption gắn thêm vào server-side deduplication, preview,
  OCR và AI khi chưa có kiến trúc riêng;
- remote AI bắt buộc hoặc proprietary hosted control plane.

## Gate nâng cấp tính năng

Một tính năng chỉ có thể chuyển khỏi `PLANNED` hoặc `EXPERIMENTAL` khi:

1. hợp đồng domain và protocol được chấp thuận, OpenAPI được review;
2. hành vi authorization, privacy, abuse, secret và audit được threat-review;
3. test correctness, retry, crash, recovery và compatibility đều qua;
4. benchmark cùng resource bound liên quan phase được ghi lại, không có target
   bịa đặt;
5. health, metric, backup/restore, upgrade và giới hạn rollback được ghi tài liệu
   vận hành;
6. tài liệu tiếng Anh và tiếng Việt khớp nhau; và
7. không còn defect data loss hoặc authorization severity cao chưa giải quyết.

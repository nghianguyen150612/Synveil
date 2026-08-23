# Đóng góp vào kiến trúc Synveil

Trạng thái: **Quy trình quy chuẩn**

Tài liệu này ngăn contributor và coding agent song song tạo ra các hợp đồng
storage, sync, API và dữ liệu hợp lý khi đứng riêng nhưng không tương thích khi
ghép lại.

## Thẩm quyền và thứ tự ưu tiên

Khi artifact mâu thuẫn, dừng triển khai và giải quyết theo thứ tự:

```text
Architecture Decision Record đã chấp thuận
    ↓
Đặc tả domain và giao thức
    ↓
Hợp đồng OpenAPI đã review
    ↓
Database migration và phiên bản storage format
    ↓
Implementation và client sinh tự động
    ↓
Ví dụ/văn bản không được đánh dấu quy chuẩn
```

Artifact phía dưới được thêm chi tiết nhưng không được trái artifact phía trên.
Migration và format on-disk đã phát hành còn tạo nghĩa vụ tương thích; ADR không
thể khiến dữ liệu đã triển khai biến mất.

Nếu ADR và đặc tả giao thức khác nhau, mở thay đổi kiến trúc trước khi viết mã.
Nếu implementation khác mọi đặc tả thì đó là defect, trừ khi migration đã phê
duyệt quy định khác.

## Thứ tự đọc baseline

Với quyết định product hoặc platform mới, hãy đọc [PRODUCT.md](PRODUCT.md),
[PLATFORM.md](PLATFORM.md), [ARCHITECTURE.md](ARCHITECTURE.md) và đặc tả
domain/giao thức liên quan trước khi chọn implementation. `PLATFORM.md` định
nghĩa ranh giới user-facing Personal/Home và Advanced/Server, host support,
service lifecycle port, filesystem capability, pairing, remote access và
recovery; tài liệu này không đưa OS concern vào domain logic.

## Baseline đã đóng băng

Blueprint cố định các mặc định sau cho Phase 0 và Phase 1:

1. Khối Rust mô-đun cung cấp API và dùng chung domain crate với Rust worker.
   Python là runtime AI tùy chọn riêng.
2. PostgreSQL là nguồn sự thật cho metadata và trạng thái giao dịch. Nội dung
   file là object trong ObjectStore, không phải BLOB database thông thường.
3. Domain ID công khai là chuỗi UUIDv7 mờ đục. Client không suy ra thời gian,
   owner, storage path hay quyền từ ID.
4. API bắt đầu bằng `/api/v1`; JSON dùng thời gian UTC RFC 3339, error code ổn
   định, keyset cursor và mutation có điều kiện.
5. Mỗi `Library` có sequence thay đổi tăng trong giao dịch. Cursor mờ đục, có
   phiên bản; client không tự tạo.
6. Content commit thành công chỉ tham chiếu Object bền vững đã verify, đồng thời
   ghi metadata change, audit và outbox trong một giao dịch.
7. SHA-256 là hash plaintext chuẩn ban đầu cho toàn vẹn và whole-object dedup.
   Storage key mờ đục, không lộ hash/path người dùng. Mặc định không dedup qua
   ranh giới owner/dedup domain.
8. Backup snapshot và trạng thái sync là domain khác nhau. Thiếu input backup
   không phát live deletion.
9. Worker tùy chọn, AI, thumbnail, OCR, search enrichment và Forgejo polling
   không phải điều kiện đồng bộ cho core storage.
10. Synveil có hai deployment profile dùng chung protocol và data model:
    Personal/Home là profile guided/native dự kiến cho user thông thường, còn
    Advanced/Server là profile do operator kiểm soát, nơi Docker Compose hoặc
    topology khác đã review được hỗ trợ. Kubernetes, message broker, Redis và
    service mesh không phải dependency Phase 0.
11. Windows, macOS, Linux Desktop và Linux Server là host target first-class của
    platform contract. Android, iPhone và iPad vẫn là client target tương lai;
    OS service manager, shell hoặc desktop assumption không được lọt vào domain
    core.
12. Storage được chọn qua capability-driven adapter contract. NTFS, ReFS, APFS,
    Btrfs, ext4, XFS, ZFS tương lai, NAS và object storage được đánh giá bằng
    capability đã discover; Btrfs/WinBtrfs chỉ là accelerator tùy chọn, không
    phải correctness requirement.
13. Mọi tính năng giữ trạng thái `PLANNED` hoặc `EXPERIMENTAL` đã document và
    không bao giờ thành `IMPLEMENTED` cho đến khi code, test bắt buộc, hướng dẫn
    vận hành và status docs đều qua promotion gate.
14. MIT License hiện tại tiếp tục có hiệu lực cho đến khi owner phê duyệt và
    thực hiện quy trình đổi giấy phép tương thích quyền.

Muốn đổi lựa chọn đóng băng phải có ADR kèm tác động migration/tương thích. Có
thể ghi thí nghiệm có scope mà không đổi mặc định.

## Thuật ngữ chung

Technical identifier giữ nguyên giữa tài liệu tiếng Anh và tiếng Việt.

| Thuật ngữ | Nghĩa quy chuẩn |
|---|---|
| `Node` | Identity của file hoặc directory người dùng nhìn thấy trong một `Library`. |
| `FileVersion` | Bản ghi metadata bất biến gắn một revision file với một `Object` chuẩn. |
| `Object` | Identity nội dung plaintext chuẩn bất biến cùng bản ghi vòng đời logic trong một dedup domain. |
| `ObjectReplica` | Một representation lưu trữ bất biến theo backend/key của `Object`, có codec, stored checksum và trạng thái replica. |
| `Library` | Namespace sync và ranh giới ownership/policy có một journal có thứ tự. |
| `ChangeEvent` | Sự kiện committed bền vững để client tiến `SyncCursor`. |
| `SyncCursor` | Token server mờ đục biểu diễn vị trí và epoch trong journal của Library. |
| `UploadSession` | Workflow staging nối tiếp/idempotent; chưa phải file nhìn thấy trước commit. |
| `BackupSet` | Định nghĩa nguồn được bảo vệ và retention theo thiết bị. |
| `BackupSnapshot` | View manifest committed bất biến; snapshot `BUILDING` không restore được. |
| `dedup domain` | Ranh giới trong đó Object plaintext giống nhau có thể dùng chung byte vật lý. |
| `outbox` | Công việc ghi cùng giao dịch mutation lõi và được giao ít nhất một lần. |
| `Personal/Home` | Deployment profile self-hosted có guided flow, ẩn lựa chọn hạ tầng thường ngày nhưng giữ nguyên server protocol và data model. |
| `Advanced/Server` | Deployment profile do operator kiểm soát cho Compose, PostgreSQL ngoài, proxy/TLS tùy chỉnh, NAS/S3, CLI và lựa chọn hạ tầng tường minh khác. |
| `PlatformRuntime` | Port cho process, service, secret, discovery, update, diagnostic và storage-host capability; không phải authorization layer của domain. |
| `ServiceLifecycle` | Contract start, stop, restart, readiness, upgrade và shutdown do Windows Service, launchd, systemd hoặc adapter khác thực hiện. |
| `StorageCapabilities` | Bằng chứng về operation mà storage backend đã chọn có thể cung cấp an toàn; acceleration thiếu thì fallback về behavior portable. |
| `pairing` | Trao đổi enroll device rõ ràng, ngắn hạn, có scope, revoke được và chống replay; tách khỏi lưu credential dài hạn. |

Domain model sở hữu định nghĩa trường chính xác. Văn bản không được tạo nghĩa
khác, ví dụ coi `Object` là file người dùng nhìn thấy.

## Taxonomy trạng thái

- `IMPLEMENTED`: implementation truy cập được, validation bắt buộc, xử lý vận
  hành và docs đều qua gate.
- `IN PROGRESS`: đã có work có scope nhưng chưa qua exit gate.
- `PLANNED`: khả năng dự kiến đã review nhưng implementation chưa hoàn tất.
- `EXPERIMENTAL`: nghiên cứu chưa ổn định, chưa hứa API/data format.
- `NON-GOAL`: bị loại rõ ràng khỏi giai đoạn liên quan.

Đổi label là thay đổi cần review. Mock, UI shell, một migration hay endpoint chỉ
có happy path không đủ bằng chứng để ghi `IMPLEMENTED`.

## Quy trình thay đổi kiến trúc

1. Nêu bất biến hoặc giới hạn cần đổi.
2. Xác định ADR, spec, API operation, migration, stored representation, client,
   test, threat control và docs song ngữ bị ảnh hưởng.
3. Viết/sửa ADR; bao gồm alternative, compatibility, migration, giới hạn
   rollback, observability và tác động security.
4. Nhận review từ architecture owner và domain owner liên quan.
5. Cập nhật đặc tả domain/giao thức ở cả hai ngôn ngữ.
6. Cập nhật OpenAPI trước hoặc cùng implementation; chỉ sinh client từ hợp đồng
   đã review.
7. Thêm test migration/tương thích trước khi deploy writer mới.
8. Triển khai reader trước writer khi nhiều version có thể chạy chung.
9. Ghi quyết định và xóa ví dụ mâu thuẫn.

Hotfix production được đi trước văn bản chỉ khi bảo vệ dữ liệu hoặc security.
Vẫn phải bổ sung ADR và đồng bộ hợp đồng trước feature release kế tiếp.

## Quy trình `OPEN DECISION`

Dùng đúng marker này khi cố ý để lựa chọn chưa giải quyết:

```text
OPEN DECISION OD-NNN: tiêu đề ngắn
Owner: workstream hoặc role
Needed by: phase gate
Options: các phương án hữu hạn
Recommendation: phương án hiện ưu tiên và lý do
Decision evidence: test, benchmark, threat review hoặc product input cần có
```

Open decision không được chặn công việc sớm hơn trừ khi đã đến `Needed by` gate.
Implementer không được âm thầm chọn phương án làm stored/public contract thoát
khỏi ranh giới experimental.

## Hợp đồng task cho implementation agent

Mỗi coding task được giao độc lập với đầy đủ trường sau. Task không điền được
prerequisite hoặc exit gate thì chưa sẵn sàng dispatch.

```markdown
# Role
Trách nhiệm domain và nghĩa vụ review.

## Context
ADR/spec section đã chấp thuận và bằng chứng repo hiện tại.

## Prerequisite gate
Artifact/test cụ thể phải qua trước.

## Objective
Một kết quả quan sát được.

## Exact scope
Hành vi và edge case nằm trong task.

## Expected files/components
Đường dẫn sở hữu; đường dẫn dùng chung phải nêu cơ chế phối hợp.

## Implementation constraints
Bất biến, compatibility, security, performance và giới hạn dependency.

## Tests
Unit, integration, recovery, protocol, security hoặc benchmark bắt buộc.

## Validation
Lệnh và bằng chứng thủ công chính xác.

## Forbidden changes
Module, public contract, migration, format hoặc mở rộng scope không thuộc quyền.

## Exit gate
Bằng chứng chuyển task từ in progress sang hoàn thành.

## Required report
Tệp đổi, test/kết quả, giả định, rủi ro, migration và follow-up.
```

Task phải đủ nhỏ để một owner chịu trách nhiệm. Tách task khi nó thay đổi hợp
đồng không liên quan; xếp tuần tự khi một task dùng output chưa ổn định của task
khác.

## Quy tắc làm song song

Làm song song an toàn khi contributor sở hữu module khác nhau và dùng hợp đồng
đã review. Không an toàn khi tự thiết kế ID, error, cursor, storage format, quan
hệ DB hoặc authorization riêng lẻ.

Trước khi bắt đầu:

- chỉ định một contract owner;
- liệt kê quyền sở hữu path và tệp dùng chung;
- đóng băng revision ADR/spec liên quan;
- định nghĩa fixture/contract test mọi phía cùng dùng;
- xác định task tích hợp và điểm rollback.

Database migration phải tuần tự và bất biến sau phát hành. OpenAPI chỉ có một
integrator. Không sửa file generated bằng tay hoặc song song.

## Gate bắt buộc cho mọi phase

- **Architecture:** quyết định đã chấp thuận, không có câu hỏi blocking chưa xử
  lý.
- **Security:** đã review threat, authorization, secret, abuse limit và audit.
- **Correctness:** bất biến cùng hành vi failure/retry được test.
- **Performance:** có phương pháp và giới hạn regression liên quan phase; không
  dùng con số marketing tự nghĩ.
- **Operations:** health, log, metric, backup/restore và upgrade đã có tài liệu.
- **Documentation:** ý nghĩa Anh/Việt tương đương, public status chính xác.
- **Next phase:** integration suite xanh, không còn lỗi data loss hoặc
  authorization severity cao.

## Tương đương tài liệu

Tạm thời tệp tiếng Anh trong `docs/en` là chuẩn phân xử cho đến khi team có thể
review đồng thời hai bản. Tệp `docs/vi` phải truyền đạt cùng quyết định, cảnh
báo, state name, ID, error code và gate; không phải bản tóm tắt. ADR để hai ngôn
ngữ trong cùng tệp để không sinh hai trạng thái.

Mọi thay đổi đặc tả lõi tiếng Anh phải kèm tệp tiếng Việt tương ứng trong cùng
documentation gate. Nếu translation tạm trễ, đánh dấu rõ cả hai tệp và chặn
release—không nhất thiết chặn nháp local—đến khi parity được phục hồi.

## Checklist review

- Thay đổi có giữ bất biến storage, sync, backup, version và restore khi crash/
  retry không?
- Mất response có replay an toàn không?
- Authorization có dựa trên principal và quan hệ resource thay vì chỉ có ID?
- Service tùy chọn có nằm ngoài đường găng không?
- Kích thước, RAM, quota và disk-full có giới hạn không?
- Lệch DB/ObjectStore có được sửa hoặc cách ly không?
- Client cũ có phát hiện không tương thích và phục hồi không mất dữ liệu?
- Status công khai có bằng chứng hỗ trợ không?
- Thay đổi có giữ cùng protocol/data model giữa Personal/Home và Advanced/Server
  không?
- Mỗi first-class host đã có path installer/service/update, uninstall, migration
  và recovery được review chưa, hay capability đã ghi rõ là `PLANNED`?
- Filesystem accelerator có nằm sau `StorageCapabilities`, với portable
  correctness path khi accelerator không có không?
- User không kỹ thuật có hoàn thành core flow mà không cần terminal không, trong
  khi operator nâng cao vẫn tới được explicit infrastructure control không?
- Hai ngôn ngữ và liên kết có hợp lệ không?

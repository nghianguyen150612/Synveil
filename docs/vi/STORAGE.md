# Lưu trữ, vòng đời object, phiên bản và Thùng rác

Trạng thái: **Hợp đồng ObjectStore VALIDATED; adapter in-memory VALIDATED; adapter
filesystem cục bộ IMPLEMENTED/VALIDATED; subset persisted upload-session và
application-service IMPLEMENTED/VALIDATED; exact-offset HTTP upload transport
IMPLEMENTED; lifecycle cấp cao PLANNED**

Tài liệu này đặc tả hợp đồng lưu trữ byte chuẩn của Synveil và vòng đời logic
nằm bên trên hợp đồng đó. Tài liệu tuân theo các ADR đã được chấp thuận và sử
dụng ý nghĩa entity trong [DOMAIN_MODEL.md](DOMAIN_MODEL.md). Chi tiết giao thức
upload nằm trong [UPLOADS.md](UPLOADS.md), đồng bộ client nằm trong
[SYNC.md](SYNC.md), còn lịch sử backup được bảo vệ nằm trong
[BACKUP.md](BACKUP.md).

Bảng trạng thái dưới đây là ranh giới implementation của repository hiện tại.
Các phần lifecycle, upload, retention, reconciliation và backend còn lại vẫn là
tài liệu quy chuẩn kế hoạch trừ khi được đánh dấu khác.

| Capability | Trạng thái hiện tại của repository |
|---|---|
| Hợp đồng `ObjectStore` | `VALIDATED` |
| adapter in-memory | `VALIDATED` |
| adapter filesystem cục bộ | `IMPLEMENTED/VALIDATED` |
| persisted upload-session state | `IMPLEMENTED/VALIDATED` |
| resumable upload service trung lập transport | `IMPLEMENTED/VALIDATED` |
| exact-offset HTTP byte upload transport | `IMPLEMENTED` |
| production download path | `PLANNED` |
| GC | `PLANNED` |
| compression | `PLANNED` |
| filesystem optimization | `PLANNED` |
| sync | `PLANNED` |
| backup | `PLANNED` |

## Phạm vi và quyền sở hữu

Workstream storage sở hữu:

- port `ObjectStore` và bộ kiểm thử tương thích adapter;
- chỉ tiếp nhận adapter filesystem cục bộ, NAS-mounted, tương thích S3/MinIO và
  storage adapter tương lai thông qua capability đã khai báo cùng bộ kiểm thử
  tương thích đó;
- việc sinh physical key mờ đục và metadata representation;
- promote object bền vững, đọc/range-read, verify, quarantine, quản lý replica,
  lease, đối soát và garbage collection;
- vòng đời `Node -> FileVersion -> Object`, bao gồm restore phiên bản;
- ngữ nghĩa metadata của Trash/Recently Deleted và điều phối purge;
- việc hạch toán storage logic, được giữ lại, đang staging và vật lý.

Module upload sở hữu transport có thể tiếp tục và gọi port storage. Module sync
sở hữu các fact trong journal và conflict policy. Module backup sở hữu manifest
snapshot và retention. Không module nào được gọi trực tiếp filesystem cục bộ
hoặc S3 SDK; chúng dùng application service của storage và port `ObjectStore`.

## Độc lập filesystem và tăng tốc theo capability

Synveil không yêu cầu Btrfs, WinBtrfs, một filesystem local cụ thể hay native
filesystem snapshot. Correctness model hướng tới NTFS, ReFS, APFS, Btrfs, ext4,
XFS, ZFS được hỗ trợ về sau, filesystem local generic, NAS-mounted filesystem
và S3-compatible/object backend theo adapter conformance gate.

Ranh giới local/backend dựa rõ trên capability:

```text
StorageBackend
    ↓ khai báo và chứng minh
StorageCapabilities
    ↓ cung cấp thông tin cho
portable correctness path và accelerator tùy chọn
```

`StorageCapabilities` có thể gồm:

```text
reflink                 block_clone
copy_on_write_clone     native_snapshot
compression             checksumming
sparse_files            atomic_rename
durable_fsync           range_reads
filesystem_health
```

Vocabulary capability và version evidence là contract của adapter. Application
logic phải branch theo evidence đã khai báo, không theo tên OS hoặc filesystem.
Thiếu capability thì dùng portable path đúng đắn, hiện unsupported rõ ràng hoặc
tắt tối ưu. Không capability nào được làm yếu guarantee commit, integrity,
retention, sync, backup, restore hay delete.

Btrfs có thể tăng tốc Linux qua reflink, compression, native snapshot, scrub
hoặc send/receive. NTFS/ReFS có thể có clone, integrity hoặc sparse-file native;
APFS có thể có file clone và snapshot phù hợp. Đây chỉ là tối ưu. Synveil
versioning không phải Btrfs snapshot, Synveil backup không phải APFS snapshot,
filesystem replication không phải Synveil sync. Identity, journal, object
reference, conflict, retention, integrity metadata, dedup và restore của
Synveil vẫn trung lập với backend.

Storage picker có thể discovery filesystem/capability, capacity, writable,
removable, layout nguy hiểm và health signal an toàn. UI Personal / Home Mode
chỉ cần hiện “Internal drive”, “External SSD”, capacity và health; `Settings →
Advanced` mới hiện NTFS/APFS/Btrfs, tên capability, backend hay durability
profile. Chọn storage không được âm thầm initialize path rộng hoặc bất ngờ.

## Các bất biến không thể thương lượng

1. Response commit content thành công tham chiếu tới một object bất biến có
   length chuẩn và SHA-256 đã được verify, đồng thời replica được chọn bền vững
   và đọc được theo hợp đồng backend của nó.
2. `FileVersion` chỉ tham chiếu tới `Object` ở trạng thái `VERIFIED`; object tạm,
   chưa hoàn chỉnh, `QUARANTINED` hoặc `DELETING` không bao giờ là file hiện tại.
3. Physical key do server sinh, mờ đục, có phiên bản và không liên quan tới tên,
   path, object ID hay plaintext hash của người dùng.
4. Byte của object là bất biến. Thay content tạo `FileVersion` mới và tạo object
   mới hoặc tái sử dụng object đã verify trong cùng dedup domain.
5. Rename hoặc move một `Node` không bao giờ copy hay rename byte object.
6. PostgreSQL là nguồn có thẩm quyền cho identity của object, reference logic,
   trạng thái vòng đời, lease và work. Listing backend là bằng chứng đối soát,
   không bao giờ là nguồn sự thật duy nhất.
7. Không operation nào commit database reference trước khi byte được tham chiếu
   trở nên bền vững. Ghi bền vững rồi database rollback tạo ra một orphan
   candidate an toàn, không tạo file nhìn thấy được.
8. Xóa object gồm hai phase, có kiểm tra generation, bị trì hoãn bởi safety
   window và chỉ được phép sau khi đã chứng minh không còn bất kỳ authoritative
   reference, hold hay lease chưa hết hạn nào.
9. Cột reference count có thể thay đổi có thể là một tối ưu, nhưng không bao giờ
   là bằng chứng đủ để xóa.
10. Authorization bắt đầu từ một logical resource đã xác thực. Object ID,
    storage key, hash, backend ETag hoặc việc sở hữu range URL không phải là
    authorization grant.
11. Logical quota độc lập với phần tiết kiệm từ dedup. Content giống nhau không
    được làm thay đổi hành vi API, tiết lộ timing hay kết quả quota của owner
    khác.
12. Corruption phải fail closed: Synveil quarantine replica hỏng và không bao
    giờ cố ý phục vụ byte hỏng như content hợp lệ.

## Mô hình logic và vật lý

```text
Library
  -> Node (file hoặc directory ổn định mà người dùng nhìn thấy)
      -> current FileVersion (metadata revision bất biến)
          -> Object (identity content chuẩn bất biến)
              -> ObjectReplica (backend + key mờ đục + representation)
```

Một `Object` là metadata về tính bằng nhau và vòng đời của byte plaintext chuẩn.
Một `ObjectReplica` là một representation được lưu của các byte đó. Deployment
cục bộ ban đầu thường có một replica, nhưng việc tách riêng hai khái niệm cho
phép migration storage có verify, repair, phân tầng và redundancy tương lai mà
không thay đổi `FileVersion.object_id`.

Tuple so sánh bằng nhau ban đầu là:

```text
(dedup_domain_id, hash_algorithm = SHA-256, plaintext_length, plaintext_hash)
```

Identity của representation còn bao gồm codec, tham số/phiên bản codec, scheme
encryption/key version, stored length và checksum byte đã lưu. Backend ETag là
bằng chứng adapter mờ đục; không bao giờ được coi là SHA-256 hay identity
content. Nghi ngờ hash collision, hash bằng nhau nhưng length khác nhau hoặc kết
quả verify bất đồng sẽ tạo một candidate riêng bị quarantine và một integrity
incident mà operator nhìn thấy. Trường hợp này không bao giờ ghi đè object hiện
có. Vì vậy constraint equality thông thường chỉ áp dụng một phần cho các row
`VERIFIED` chuẩn thông thường; collision candidate bị quarantine mang một
incident/discriminator và không thể được tham chiếu. Collision SHA-256 cùng
length đã xác nhận đòi hỏi migration identity/format thứ cấp đã review và vô
hiệu hóa aliasing cho các byte đó thay vì ép một row đi qua uniqueness rule
thông thường.

### Hình dạng physical key được khuyến nghị

Key generator phát ra layout version đã đăng ký cùng các component ngẫu
nhiên/mờ đục, ví dụ:

```text
objects/v1/7f/2a/<opaque-random-key>
staging/v1/uploads/<opaque-session-key>/<opaque-part-key>
```

Ví dụ này không phải public contract. Client không bao giờ thấy hoặc dựng key,
và migration layout không làm đổi public ID. Component được sinh dùng alphabet
nghiêm ngặt phía server; không component path không tin cậy nào đi vào key. Một
key chỉ dùng cho một representation bất biến và không bao giờ được tái sử dụng,
kể cả sau khi xóa.

## Port `ObjectStore`

Port hỗ trợ streaming và nhận biết capability. Các Rust type chính xác thuộc về
hợp đồng crate đã review, nhưng các operation về mặt ngữ nghĩa là:

| Operation | Ngữ nghĩa bắt buộc |
|---|---|
| Begin staged write | Tạo staging target độc quyền có tên do server đặt và trả về handle mờ đục. Collision không thể ghi đè byte. |
| Stream staged bytes/part | Áp dụng giới hạn length đã khai báo và quan sát được trong khi hash với memory có giới hạn. Failure không để lại object đã commit. |
| Inspect staged state | Trả về bằng chứng adapter cần cho retry an toàn; trạng thái không tồn tại phải được biểu diễn rõ. |
| Abort staging | Xóa/hủy theo cách idempotent một staging handle đã biết, bao gồm native multipart upload. |
| Finalize immutable | Tạo một final key không bao giờ bị ghi đè và durability receipt. Retry trùng lặp trả về cùng receipt hoặc chứng minh key đã finalize. |
| `HEAD` immutable | Trả về tồn tại, stored length, checksum/metadata representation và bằng chứng version backend mờ đục. |
| Read/range-read | Stream byte logic đã authorize với memory có giới hạn và hành vi range chính xác; không bao giờ trả partial success như full object. |
| Delete immutable | Chỉ xóa key được server authorize cùng generation/version replica dự kiến; không tồn tại là idempotent success. |
| Enumerate owned prefix | Hỗ trợ đối soát khi có thể. Tính nhất quán listing được khai báo và listing không bao giờ được dùng để authorize hoặc thiết lập reference mới commit. |

Một durability receipt chứa ít nhất backend ID, final opaque key, stored length,
stored-byte checksum và algorithm, format và version của representation, bằng
chứng object/version backend, thời điểm hoàn tất và những durability check đã
khai báo nào thành công. Chỉ application service mới có thể chuyển receipt đó
thành record object/replica `VERIFIED`.

### Khai báo capability

Mỗi backend được cấu hình phải khai báo và chứng minh qua conformance test:

- hỗ trợ atomic promotion, nếu có;
- hỗ trợ tạo conditional/exclusive;
- hỗ trợ native multipart cùng các quy tắc part tối thiểu/tối đa;
- các checksum algorithm đáng tin cậy và chúng có bao phủ object hoàn chỉnh hay
  không;
- hành vi read-after-write đối với key đã hoàn tất;
- tính nhất quán và hành vi phân trang của listing;
- hỗ trợ delete có điều kiện/có version;
- hành vi range-read;
- bảo đảm durability-flush mà adapter cung cấp;
- object size và metadata limit tối đa.

Application code phân nhánh theo capability tường minh. Code không được suy ra
capability từ tên adapter và không được hạ thấp bảo đảm success khi backend yếu
hơn. Adapter không chứng minh được read-after-write/durability bắt buộc phải trì
hoãn success, dùng workflow verify mạnh hơn hoặc không đủ điều kiện cho write
production.

## Adapter filesystem cục bộ

Adapter production đầu tiên được implement trong
`crates/storage/src/local.rs` và được export lại qua storage composition crate.
Hợp đồng root tường minh của nó đã được validate trên host hiện tại bằng
primitive filesystem Rust/Tokio portable:

- `LocalFilesystemObjectStore::open` chỉ nhận root absolute được truyền tường
  minh, giữ dạng canonical đã resolve, tạo marker layout Synveil cùng `objects/`
  và `staging/`, từ chối root filesystem, home/profile người dùng,
  current-directory và source-workspace tại thời điểm build, đồng thời không
  recursive-clean content không rõ chủ sở hữu. Marker chứng minh đúng version
  local adapter/layout; việc gắn nó với record `StorageBackend` bền vững vẫn
  thuộc tầng cao hơn.
- Physical path được sinh từ SHA-256 của `ObjectKey` opaque đã validate, không từ
  filename, public ID, MIME type hay caller path. Layout nội bộ là
  `objects/v1/<hash-prefix>/<key-hash>/` với content, metadata và committed
  marker; client không thấy layout này.
- Mỗi staging handle là tên opaque dựa trên UUID. Byte được stream vào file
  `.upload` exclusive, hash và kiểm tra length bằng memory bounded, sau đó được
  freeze thành `.verified` cùng metadata record bounded. Staging không xuất hiện
  trong đọc final object và staging cũ không bị tự động xóa.
- Promotion verify lại staged bytes, tạo final directory và content bằng
  hard-link create-only trong cùng root, ghi metadata rồi ghi committed marker
  sau cùng. Key committed đã tồn tại trả về conflict ổn định; không có fallback
  copy cross-device âm thầm làm yếu an toàn promotion. Directory chưa hoàn tất
  không có committed marker tiếp tục vô hình và được quarantine thay vì bị ghi
  đè hay tự động xóa.
- Full read stream qua buffer bounded và validate SHA-256 đã lưu; range read
  validate logical range và toàn bộ object trước khi stream riêng range đó.
  Metadata chỉ trả các field của contract. Delete và conditional delete chỉ
  tác động đúng opaque key committed. So sánh và xóa có điều kiện được serialize
  giữa các clone của một adapter instance; không tuyên bố atomic giữa các
  process độc lập, và các process đó không được mutate đồng thời cùng local root.
- Adapter gọi `sync_all` cho staged content, metadata có giới hạn và commit
  marker. Khi probe lúc open chứng minh directory synchronization, adapter cũng
  sync transition staging đã hoàn tất, chuỗi directory promotion và visibility
  của delete trước khi báo success. `DurableFsync` đòi file-sync probe, còn
  `DurableFlush` đòi thêm directory-sync và same-root hard-link promotion probe.
  Các capability này mô tả hành vi quan sát được, không suy ra từ tên OS.
  Compression, snapshot, reflink/block clone và filesystem-health acceleration
  được đánh dấu unsupported rõ ràng. Unit/conformance test exercise các code
  path này nhưng không phải bằng chứng crash khi mất điện.
- Managed directory/file được kiểm tra bằng metadata no-follow, bao gồm path
  nhạy cảm với Windows reparse/symlink khi standard API cung cấp bằng chứng.
  Portable API không thể loại bỏ mọi TOCTOU race giữa process, nên ownership và
  permission deployment vẫn bắt buộc.

Adapter và upload service trung lập transport đã được contract/conformance
validate. HTTP transport exact-offset đã authenticate hiện stream request frame
có giới hạn qua service đó; đây không phải download path hay protocol
part-manifest tương lai. Download cấp cao, GC, sync, backup và installer
deployment vẫn là kế hoạch. Transaction
finalization PostgreSQL của service chỉ tạo `FileVersion` nhìn thấy được đầu
tiên sau khi object đã durable và được verify. NAS và filesystem khác vẫn cần
capability/crash evidence riêng trước khi tuyên bố production support.

Startup không scan đệ quy toàn bộ content trước khi phục vụ. Nó validate
identity/configuration và lập lịch đối soát có giới hạn. Object được tham chiếu
nhưng thiếu làm readiness degraded hoặc failed tùy phạm vi; không bao giờ âm
thầm tạo lại chúng thành file rỗng.

## Adapter tương thích S3

S3/MinIO là adapter về sau, không phải dependency deployment giai đoạn đầu. Hợp
đồng của nó cố ý khác filesystem:

- Dùng final key chưa từng được dùng trước đó. Không mô phỏng rename bằng
  copy-and-delete rồi gọi đó là atomic promotion.
- Native multipart upload vẫn là staging cho đến khi complete thành công. Lưu
  upload identifier và part receipt dưới dạng dữ liệu adapter mờ đục; abort là
  idempotent và cleanup cũng liệt kê các multipart upload bị bỏ dở để phòng vệ.
- Sau completion, thực hiện `HEAD`/read verification mà capability yêu cầu trước
  khi metadata commit. Lời gọi complete bị mất response được recovery bằng cách
  inspect final key đã gán trước; không xử lý bằng cách mù quáng tạo key thứ hai.
- Không bao giờ diễn giải multipart ETag là plaintext MD5 hay SHA-256. Hỗ trợ
  checksum của các hệ tương thích S3 khác nhau; SHA-256 chuẩn được tính hoặc
  verify độc lập qua upload protocol.
- Ghi provider version ID khi bucket versioning được bật, nhưng không yêu cầu
  versioning để đảm bảo object immutability. Conditional creation và delete
  dùng precondition do adapter hỗ trợ khi có.
- Listing có thể trễ. `HEAD` trực tiếp một key đã biết và record PostgreSQL điều
  khiển recovery; listing lặp có giới hạn chỉ là bằng chứng orphan.
- Signed URL cho direct upload/download không phải shortcut ban đầu. Nếu bật về
  sau, chúng đòi hỏi extension ADR/protocol bao quát proof checksum, enforcement
  content-length, expiry, giới hạn revocation, authorization, audit và ngăn
  content-presence oracle.

## State machine của object và replica

### Trạng thái object

```mermaid
stateDiagram-v2
    [*] --> STAGING
    STAGING --> VERIFIED: canonical bytes and durable replica verified
    STAGING --> QUARANTINED: mismatch or ambiguous integrity
    STAGING --> DELETING: abandoned after leases and grace
    VERIFIED --> QUARANTINED: no trustworthy replica remains
    VERIFIED --> DELETING: no references, holds, leases, or grace
    QUARANTINED --> VERIFIED: independently repaired and verified
    QUARANTINED --> DELETING: unreferenced and incident policy permits
    DELETING --> [*]: every selected replica absent and metadata tombstoned
```

Chỉ `VERIFIED` mới có thể nhận authoritative reference mới. Chuyển sang
`DELETING` lock row object và đóng gate đó. Recovery không chuyển object
thiếu/hỏng về `VERIFIED` nếu không có bằng chứng verify mới.

### Trạng thái replica

```text
COPYING -> VERIFIED -> DELETING -> deleted/tombstoned
    |          |
    v          +-> MISSING
  CORRUPT <--------+
```

Phải có ít nhất một replica `VERIFIED` để object tiếp tục đọc được. Nếu một
replica fail nhưng replica khác verify được, read route tới replica lành và một
repair job được ghi bền vững. Nếu không replica nào verify được, object bị
quarantine và mọi logical reference bị ảnh hưởng vẫn được giữ để operator
recovery.

### Object lease

Lease bảo vệ công việc không phải reference như assembly upload, snapshot đang
build, active restore, migration copy hoặc integrity repair. Mỗi lease có
identity object/staging, purpose, owner operation ID, generation và expiry có
giới hạn. Renewal có điều kiện theo generation. Worker stale không thể gia hạn
hoặc release lease của successor. Lease hết hạn và được đối soát; lease không
thay thế `FileVersion` đã commit hay reference `BackupEntry`.

## Commit content theo thứ tự bền vững trước

Commit content là một saga nhỏ với bước kết thúc nguyên tử trong PostgreSQL:

1. Lưu intent của operation/session, destination, base condition, quota
   reservation, final key gán trước và idempotency identity.
2. Stream/assemble vào staging với resource có giới hạn.
3. Tính length/SHA-256 chuẩn, finalize một representation bất biến và nhận
   durability receipt.
4. Verify key cuối đã biết qua backend được chọn. Lưu đủ bằng chứng receipt cho
   recovery trước khi thử logical commit.
5. Trong một database transaction, lock operation và các logical row bị ảnh
   hưởng; revalidate authorization, base version/revision, name và quota; chọn
   hoặc tạo `Object` trong dedup domain; tạo hoặc attach `ObjectReplica` đã
   verify để bind receipt backend/key/representation được chọn với object đó
   (hoặc ghi representation không được chọn cho orphan disposition được grace
   bảo vệ); tạo logical reference cùng `FileVersion`/binding backup manifest
   bất biến; cập nhật current logical state; append journal, audit, job/outbox
   bắt buộc, accounting và idempotent outcome.
6. Commit, rồi trả về outcome đã lưu. Consumer tùy chọn chạy sau.

Nếu bước 3 fail, không metadata nào nhìn thấy được. Nếu bước 3-4 thành công nhưng
bước 5 rollback, final key là orphan candidate được operation lease và global
orphan grace bảo vệ. Retry cùng operation sẽ tái sử dụng/re-verify key đó. Nếu
bước 5 commit và process crash trước khi response, lookup idempotency trả về
chính xác ID, ETag và kết quả journal đã commit.

Không giữ PostgreSQL transaction mở trong khi upload, hash object lớn, copy
replica, gọi S3 hoặc chờ worker.

## Deduplication toàn object

Deduplication chỉ diễn ra sau khi server verify. Commit transaction tìm object
`VERIFIED` hiện có cùng equality tuple trong cùng `dedup_domain_id`:

- Nếu có object đáng tin cậy, tạo logical reference mới tới nó và để
  representation mới ghi đã verify trở thành delayed orphan, hoặc gắn nó làm
  replica được mong muốn tường minh. Lựa chọn không được ảnh hưởng response theo
  cách làm lộ content ngoài domain.
- Nếu các transaction concurrent race, unique constraint trên equality tuple
  chọn một canonical object. Bên thua đọc lại bên thắng và commit logical
  reference của mình; byte không được chọn của nó vẫn là orphan data được grace
  bảo vệ.
- Không bao giờ chấp nhận client hash làm bằng chứng byte đã tồn tại. Tối ưu bỏ
  qua upload cần proof-of-possession protocol đã review và phải nằm trong dedup
  domain.
- Restore version cũ hoặc tái sử dụng backup entry không đổi sẽ thêm reference
  mà không copy byte.

Deduplication cấp chunk là capability nâng cao `PLANNED`, không phải storage
profile ban đầu. Nó làm thay đổi blast radius của corruption, format manifest,
range read, encryption, GC và repair, đồng thời đòi hỏi ADR riêng được chấp
thuận, lý do đo được và kế hoạch migration storage-format trước implementation.

## Policy compression representation

Compression là tối ưu physical representation `PLANNED` cho Phase 6. Correctness
profile ban đầu lưu canonical byte theo pass-through. Bật hay đổi codec không
bao giờ làm thay đổi `Node`, `FileVersion`, canonical plaintext length/SHA-256,
retention, authorization hay sự thật logical quota.

Codec candidate đầu tiên là Zstandard với một tập profile tham số nhỏ, có
version. Việc lựa chọn có tính xác định và dựa trên policy:

1. Coi MIME type và filename extension là hint không tin cậy. Dùng sample có
   giới hạn cùng allowlist/denylist, minimum size, maximum CPU/concurrency
   budget và ngưỡng expected space-saving.
2. Text, JSON, CSV, log, database dump và source code chỉ là candidate có khả
   năng sau khi có số đo corpus. Không mù quáng recompress JPEG, WebP, AVIF,
   MP4, MKV, ZIP, 7z, representation khác đã nén hoặc encrypted data.
3. Fallback về pass-through khi sampling mơ hồ, compression làm data lớn hơn,
   codec/version không có, chạm resource limit hoặc không thể đáp ứng active
   range-read profile.
4. Ghi decision và algorithm generation để cùng object vẫn đọc được sau policy
   change. Policy mới ảnh hưởng replica mới/re-encoded, không ảnh hưởng cách
   diễn giải representation hiện có.

Mỗi `ObjectReplica` đã nén ghi ít nhất:

- format/version representation (`IDENTITY` hoặc named Zstandard profile), codec
  parameter/dictionary identity nếu dictionary từng được cho phép;
- canonical plaintext length và SHA-256 từ `Object`;
- stored representation length và independent stored-byte checksum;
- identity seek/chunk index có version tùy chọn và encoded logical range;
- backend/key, thời điểm create/verify và reader compatibility floor.

Read stream qua decoder có giới hạn và verify declared length. Full read có thể
verify canonical SHA-256; stored checksum phát hiện corruption representation
trước hoặc trong decoding. Decoder reject output vượt declared plaintext
length, expansion quá mức, malformed frame, unsupported dictionary hoặc
configured resource limit, để representation hỏng không thể trở thành
decompression bomb.

Transparent decompression không biện minh cho false range promise. Whole-object
Zstandard thường làm arbitrary logical range read tốn kém. Backend chỉ có thể
advertise logical `Accept-Ranges` khi pass-through storage hoặc format
chunk/seek có version, decodable độc lập đáp ứng requested range với bounded
work. Nếu không, compression vẫn disabled cho profile đó tới khi
OD-STORAGE-003 được đóng.

Whole-object deduplication so sánh verified canonical plaintext identity trước
khi chọn representation, nên equal content có thể tham chiếu một `Object` kể cả
khi replica dùng codec generation khác nhau. Physical saving từ dedup và
compression được đo riêng; người dùng thấy ước lượng logical/retained, staged,
physical và saved-byte mà không tối ưu nào làm đổi semantic quota hay retention.

Nếu application-managed encryption được bật về sau, readable content được nén
trước khi encryption. Ciphertext không thể nén hữu ích, và E2EE mode không thể
âm thầm giữ promise server-side compression/dedup/indexing. Re-encoding là
asynchronous replica migration: ghi, decode và verify immutable representation
mới, switch/thêm selected replica trong transaction, chờ qua lease/rollback
grace rồi retire representation cũ. Export và restore luôn tạo canonical
plaintext byte sau authorization.

## Hợp đồng versioning

Content hiện tại của file là một `FileVersion` bất biến; lịch sử của nó không
bao giờ bị viết lại.

| Operation | Tác động lên phiên bản |
|---|---|
| Create file | Tạo phiên bản đầu tiên và trỏ node mới tới đó theo cách nguyên tử. |
| Replace/sync edit | Tạo child version từ base đã chấp nhận và cập nhật node head theo cách nguyên tử. |
| Rename/move | Chỉ tăng node metadata revision; không tạo content version. |
| Copy | Tạo node mới và record `FileVersion` mới có thể tham chiếu cùng object; không dùng chung mutable node identity. |
| Restore old version | Tạo head version mới tham chiếu byte object lịch sử và ghi source `RESTORE`; không bao giờ lùi head pointer để viết lại lịch sử. |
| Conflict | Bảo toàn byte incoming trong conflict version/node theo [SYNC.md](SYNC.md); không bao giờ âm thầm thay winning head. |
| Trash | Giữ version và object reference trong suốt retention của Trash. |
| Purge/retention expiry | Xóa logical version reference bằng purge work bền vững; GC object vật lý vẫn tách riêng. |

Mỗi content mutation cung cấp base version phù hợp với operation. Node revision
bảo vệ metadata mutation. Cả `Object` ID lẫn physical key của `ObjectReplica`
đều không bao giờ làm concurrency token.

Version retention policy phải tường minh và có phiên bản. Policy có thể giữ tất
cả, giữ theo duration/count hoặc đặt hold, nhưng không thể xóa current version,
version cần cho retained snapshot, conflict vẫn được hiển thị cho người dùng
hoặc bất kỳ protected reference nào khác. Retention trước hết làm logical
reference hết hạn; object GC sau đó xác định byte có thể xóa vật lý hay không.

## Trash / Recently Deleted

Trash là soft deletion người dùng nhìn thấy trong live sync domain. Nó không
phải backup retention và không phải physical deletion.

Trạng thái implementation của metadata API hiện tại: logical trash một node
đơn lẻ đã implement, root được bảo vệ và directory không rỗng bị reject bằng
conflict ổn định. Recursive subtree trash cố ý chưa implement cho tới khi
subtree precondition và contract edit descendant concurrent trong `SYNC.md`
OD-SYNC-004 được đóng. Operation hiện tại không xóa row `FileVersion` hoặc
`Object`.

### Transaction đưa vào Trash

Đối với một node hoặc subtree directory, một PostgreSQL transaction duy nhất:

1. authorize actor và lock target cùng các row ancestry/name bắt buộc;
2. verify expected node revision và, với recursive directory action,
   precondition subtree do server cấp; đồng thời kiểm tra không ancestor nào đã
   khiến operation trở nên dư thừa;
3. chuyển subtree root sang `TRASHED` và ghi một `TrashEntry` với former
   parent/name, actor, deletion operation và purge deadline;
4. làm toàn subtree không thể truy cập qua live-tree mutation và traversal
   thông thường dựa trên effective ancestor state;
5. append recursive tombstone `ChangeEvent`, audit fact, job bắt buộc và
   idempotent outcome trong transaction sequence của library;
6. commit mà không xóa `FileVersion` hay object reference nào.

Không descendant mutation nào được lọt qua sau quyết định cho subtree. Correctness
profile ban đầu lấy library namespace-mutation guard trước mọi transaction mutate
một `Node`; Trash giữ guard đó cho tới commit. Scheme chi tiết hơn trong tương lai
có thể dùng shared ancestor/exclusive subtree lock, nhưng phải chứng minh cùng
thứ tự edit-versus-Trash trước khi thay guard.

Physical schema có thể đặt root của trashed subtree vào namespace ẩn do server
quản lý hoặc giữ ancestry cùng effective-state index. Mỗi layout phải bảo toàn
stable node ID, cho phép tái sử dụng tên active sibling, ngăn trashed descendant
bị edit và vượt qua cùng các subtree race test. Layout không được update mọi
descendant chỉ để làm directory lớn biến mất.

Client áp dụng recursive tombstone cho subtree cục bộ. Client không sở hữu toàn
bộ subtree sẽ invalidate root đã biết và đối soát qua node/snapshot API; không
suy diễn các child bị thiếu là upload mới.

### Transaction restore

Restore có điều kiện và tường minh:

- API xác định `TrashEntry`, expected trash revision, target parent và collision
  policy. Mặc định là parent/name đã ghi nếu cả hai vẫn hợp lệ.
- Nếu parent đã purge, authorization thay đổi hoặc name bị chiếm, trả về các
  fact hiện tại và yêu cầu alternate parent/name tường minh (hoặc reviewed
  non-destructive rename policy). Không bao giờ ghi đè node đang chiếm tên.
- Transaction restore subtree root sang `ACTIVE`, xóa/đánh dấu trash context đã
  restore, tăng revision và append restore change, audit, outbox cùng idempotent
  outcome. Byte và version của descendant không đổi.
- Restore directory sẽ restore retained subtree của nó. Nested independent
  trash entry, nếu sau này product cho phép, vẫn trashed; hành vi ban đầu nên từ
  chối redundant nested trash operation để tránh mơ hồ.

### State machine purge

```text
TRASHED --retention/manual confirmation--> PURGING --batched durable job--> logical tombstone
   ^                                          |
   +--------------- restore -----------------+  (only before PURGING begins)
```

Đi vào `PURGING` là điểm không thể quay lại ở lớp logic và được audit trong
transaction. Một job duyệt subtree theo các batch xác định, có giới hạn, xóa
authoritative version/reference row, ghi progress và có thể tiếp tục sau crash.
Root vẫn ẩn và immutable trong khi còn công việc một phần. Completion giữ lại
tombstone nhỏ gọn cho journal/replay policy và chỉ release object sang workflow
GC eligibility riêng. Purge fail không làm dữ liệu đã purge một phần xuất hiện
live.

Mặc định Trash tiếp tục tiêu thụ retained logical quota. UI báo riêng live byte,
historical-version byte, Trash byte, backup byte, staging-reserved byte và
physical byte để người dùng hiểu vì sao xóa chưa giải phóng capacity.

## Garbage collection và đối soát orphan

### Bằng chứng đủ điều kiện

Object chỉ có thể thành GC candidate khi tất cả điều sau đúng trong transaction
lock lifecycle row của nó:

- state là `VERIFIED` hoặc state `QUARANTINED` không được tham chiếu đã được
  incident policy chấp thuận;
- không current hay historical `FileVersion` nào được retention policy bảo vệ
  tham chiếu nó;
- không backup/repository snapshot `COMMITTED` hay retained derivative nào tham
  chiếu nó;
- không Trash, legal/administrative hold, migration/repair plan hay active
  restore nào bảo vệ nó;
- không `ObjectLease` chưa hết hạn hay staging/upload claim nào bảo vệ key;
- object và mọi transaction có thể xóa reference đều cũ hơn safety window được
  cấu hình;
- đối soát không còn điều mơ hồ chưa giải quyết cho backend/key đó.

Transaction đặt `DELETING`, tăng delete generation và insert một delete job duy
nhất. Vì reference mới chỉ được phép nhắm tới `VERIFIED`, không reference mới
nào có thể race vào sau transition này.

### Delete job

Worker claim generation, xóa từng replica được chọn với bằng chứng key/version
dự kiến, verify absence theo yêu cầu rồi ghi kết quả. Backend absence là
idempotent success. Lỗi backend tạm thời retry với backoff. Lỗi
permission/configuration trở thành trạng thái cần operator hành động; database
record vẫn `DELETING`. Sau khi mọi replica được xác nhận không tồn tại, một
transaction ngắn tombstone/finalize metadata. Nếu byte đã xóa nhưng final
transaction fail, retry quan sát absence và hoàn tất an toàn.

Không cleanup command nào nhận arbitrary user path, prefix không giới hạn hay
backend target rỗng.

### Các lớp orphan

- **Staging orphan:** trạng thái part/temp/multipart không còn được open session
  hay valid lease tham chiếu. Cleanup dùng upload expiry cộng safety grace.
- **Final-key orphan:** byte bất biến đã finalize nhưng không transaction
  object/reference nào commit. Recovery trước hết kiểm tra receipt của
  session/operation và có thể tiếp tục commit; chỉ sau đó cleanup theo grace mới
  được xóa chúng.
- **Metadata orphan:** PostgreSQL tham chiếu replica thiếu/hỏng. Đây là công việc
  data-loss/quarantine, không bao giờ là chỉ thị xóa metadata.
- **Duplicate verified representation:** dedup concurrent tạo thêm byte đã
  verify. Nó tuân theo xử lý final-key orphan thông thường.

Đối soát chạy định kỳ, có phân trang, có thể restart, có rate limit và ghi
watermark/cursor theo backend. Phải có nhiều pass cũ hơn grace trước khi hành
động phá hủy dựa trên listing eventually consistent. Finding tạo metric và
repair/cleanup job bền vững, không xóa inline ngay lập tức.

## Verify integrity và read

Mỗi upload tính plaintext length và SHA-256. Mỗi representation cũng có
stored-byte checksum. Các mức verify phải tường minh:

- **Commit verification:** bắt buộc trước reference đầu tiên; validate
  canonical length/hash và durability receipt của representation.
- **Read verification:** validate checksum backend/provider ở nơi đáng tin cậy;
  full read có thể validate plaintext SHA-256 trước khi tuyên bố verified
  delivery.
- **Scrub verification:** job định kỳ có giới hạn đọc lại stored byte, validate
  representation và canonical content rồi ghi bằng chứng/thời điểm.
- **Restore verification:** validate chính xác source object và destination
  result theo [BACKUP.md](BACKUP.md).

Range read trả về logical byte range. Stored codec không hỗ trợ arbitrary
logical range phải decode qua server path có giới hạn, lưu seek/chunk index có
version hoặc bị loại khỏi canonical object phục vụ range. API không bao giờ trả
range trên byte của compressed representation nhưng gắn nhãn chúng là file
offset.

Khi mismatch, dừng stream nếu có thể, đánh dấu replica suspect qua durable
incident command, thử replica khác đã verify sẵn và trả response ổn định
`object_corrupt`/`storage_unavailable`. Không repair bằng cách copy từ source
chưa verify. Backup manifest tham chiếu cùng sole physical replica là historical
protection, không phải corruption failure domain độc lập; tài liệu operation
phải nói rõ điều này.

## Hạch toán và capacity

Theo dõi độc lập ít nhất các dimension sau:

| Metric | Ý nghĩa |
|---|---|
| Live logical bytes | Kích thước file hiện thấy theo counting rule của policy. |
| Historical-version bytes | Kích thước logic được giữ bởi file-version policy. |
| Trash bytes | Kích thước logic vẫn restore được từ Trash. |
| Backup logical bytes | Tổng được biểu diễn bởi retained snapshot manifest theo reporting view đã chọn. |
| Staging bytes/reservation | Byte vật lý chưa hoàn tất cộng byte logic dự kiến đã reserve. |
| Physical stored bytes | Representation length thực của replica, bao gồm các lớp temporary/duplicate/orphan khi đo được. |
| Dedup/compression savings | Chỉ là ước lượng dẫn xuất; không bao giờ là sự thật cho authorization hoặc deletion. |

Bắt đầu upload sẽ reserve expected logical capacity và enforce staging limit
theo user, library, session và instance. Commit chuyển reservation theo cách
nguyên tử. Expiry/abort release nó theo cách idempotent. Dedup không xóa logical
usage. Probe disk-free chỉ mang tính gợi ý; mọi write vẫn phải xử lý
`ENOSPC`/provider quota theo cách nguyên tử và an toàn.

## Migration storage backend

Migration là copy theo replica và verified switch:

1. đăng ký và health-check destination backend mà không đổi current read;
2. tạo migration operation và lease cho một object batch có giới hạn;
3. stream từ verified source tới opaque destination key mới;
4. verify canonical content và destination representation;
5. thêm/chuyển replica `VERIFIED` trong transaction và ghi audit/progress;
6. giữ replica cũ trong rollback window;
7. chỉ retire replica cũ qua normal two-phase deletion;
8. chỉ retire backend sau khi proof query cho thấy nó không phải sole verified
   location của bất kỳ protected object nào.

Copy failure để location hiện tại giữ thẩm quyền. Mixed backend là trạng thái
migration bình thường. Chỉnh mount path hay bucket prefix trong configuration
không phải migration và phải fail validation storage-identity.

## Ranh giới change, audit, outbox và job

Các record này có audience và retention khác nhau:

- `ChangeEvent` là library fact có thứ tự cho sync client và được đặc tả trong
  [SYNC.md](SYNC.md).
- `AuditEvent` là bằng chứng accountability/security hướng append. Nó ghi actor,
  action, target, outcome, correlation và safe reason—không ghi file content,
  secret hay raw storage key.
- `OutboxEvent` là committed domain fact/bàn giao follow-up được insert trong
  cùng transaction với mutation.
- `Job` là công việc thực thi at-least-once với stable identity, payload schema
  version, attempt, availability time, lease và terminal state.

Work bắt buộc nên được insert trực tiếp thành job trong core transaction khi
không cần fan-out riêng. Nếu outbox dispatcher tạo job, unique consumer/event
identity làm conversion đó idempotent; không bao giờ có unprotected in-memory
handoff.

### Trạng thái job và lease protocol

```mermaid
stateDiagram-v2
    [*] --> AVAILABLE
    AVAILABLE --> LEASED: short claim transaction
    LEASED --> SUCCEEDED: generation-matched completion
    LEASED --> AVAILABLE: retryable failure or expired lease
    LEASED --> DEAD_LETTER: permanent/exhausted/operator-action failure
    AVAILABLE --> CANCELED: owning operation canceled
    DEAD_LETTER --> AVAILABLE: audited manual replay with new generation
```

Worker claim batch có giới hạn bằng row lock/`SKIP LOCKED`, đặt `lease_owner`,
`lease_generation`, `lease_until` và attempt count, rồi commit trước khi làm
công việc external hoặc dài. Update heartbeat, success và failure có điều kiện
theo cùng generation. Worker tiếp tục sau khi mất lease không thể complete hay
ghi đè kết quả successor. Handler dùng semantic key duy nhất như
`(job_kind, aggregate_id, aggregate_revision)` và verify current state trước khi
hành động.

Retry dùng exponential backoff đã phân loại, có jitter và cap. Failure về
integrity, authorization, invalid format và missing-secret không được retry
trong tight loop. Queue oldest age, attempt, expired lease, dead letter và job
duration là metric. Manual replay được audit và không bao giờ sửa historical
outbox payload tại chỗ.

## Hợp đồng lỗi

API tiếp xúc storage dùng code ổn định và detail an toàn:

- `not_found` cho logical resource đã authorize nhưng không tồn tại;
- `permission_denied` mà không làm lộ sự tồn tại của object khác owner;
- `version_conflict` hoặc `name_conflict` cho conditional logical mutation;
- `quota_exceeded` cho policy capacity rejection;
- `storage_unavailable` cho failure backend/disk/flush có thể retry;
- `object_corrupt` cho integrity failure đã biết;
- `invalid_range` cho logical byte range không thỏa mãn;
- `resource_in_trash` hoặc `purge_in_progress` cho lifecycle action không hợp
  lệ;
- `internal_error` cùng correlation ID, không bao giờ kèm filesystem path,
  bucket secret, SQL error hay stack trace.

Retryability phải tường minh. Không trả HTTP success cho tới khi strong boundary
liên quan được thỏa mãn.

## Ma trận crash và failure

| Điểm crash/failure | Recovery bắt buộc |
|---|---|
| Process chết trong staging stream | Không verified part/object nào được ghi; temp độc quyền bị xóa sau session lease/grace. |
| Disk đầy hoặc flush fail | Dừng stream, báo lỗi storage/capacity có thể retry, không giữ visible version, đối soát partial temp. |
| Final local rename/remote complete thành công nhưng mất update receipt | Key gán trước cùng session intent cho phép recovery `HEAD` và re-verify; không bao giờ tạo logical result thứ hai. |
| Object bền vững, PostgreSQL commit rollback | Không logical reference; retry cùng operation hoặc phân loại orphan sau lease và grace. |
| PostgreSQL commit, response bị mất | Persisted idempotency result trả cùng outcome node/version/event. |
| Transaction dedup race | Equality uniqueness chọn một canonical object; cả hai logical operation có thể tham chiếu nó; byte bên thua là delayed orphan. |
| Worker chết sau khi claim job | Lease hết hạn; worker khác retry. Stale generation không thể complete. |
| Worker chết sau backend delete nhưng trước DB update | Retry coi absence là success và finalize cùng delete generation. |
| Backend mất key được tham chiếu | Giữ metadata, quarantine/đánh dấu replica missing, thử verified replica hoặc operator restore; không bao giờ chuyển mất mát thành user deletion. |
| Listing đối soát bỏ sót key | Không đưa ra kết luận phá hủy từ một listing; bắt buộc check trực tiếp known-key và nhiều pass cách nhau bởi grace. |
| Cấu hình storage root/bucket thay đổi ngoài dự kiến | Fail readiness với storage identity error; không tự động khởi tạo replacement rỗng. |

## Test bắt buộc

### Conformance adapter

- stream object zero-byte, nhỏ, multipart-sized và maximum-policy mà không
  buffer toàn file;
- collision key độc quyền và finalization concurrent không bao giờ ghi đè;
- hành vi exact và suffix/prefix range, invalid range, reader bị cancel và
  backpressure;
- crash trước/sau flush và promotion/complete, bao gồm lost completion response;
- short write, disk full, backend read-only, mất permission, timeout và transient
  provider error;
- stored checksum mismatch, truncation, byte thừa, stale ETag/version và corrupt
  metadata;
- abort/delete idempotent và delete-generation mismatch;
- fixture symlink/path traversal/race cục bộ và storage-root identity mismatch;
- S3 multipart ETag không được coi là canonical hash, eventually consistent
  list và recovery trực tiếp known-key;
- memory và file-descriptor use có giới hạn dưới các large stream concurrent.

### Tích hợp vòng đời và transaction

- không `FileVersion` nào có thể tham chiếu content không `VERIFIED`/sai domain;
- rename/move node nhiều gigabyte không thực hiện object operation;
- restore tạo head bất biến mới và giữ version cũ đã chọn;
- upload giống nhau concurrent dedup thành một canonical object mà không làm mất
  logical outcome nào;
- inconsistency SHA-256/length dẫn đến quarantine thay vì alias;
- object bền vững + ép DB rollback không để lại visible file và chỉ được
  recovery hoặc thu gom sau grace;
- DB commit + drop response replay kết quả y hệt;
- tạo reference race với GC hoặc commit trước `DELETING`, hoặc fail rồi retry;
  không bao giờ trỏ tới byte đã xóa;
- drift refcount không thể gây deletion khi authoritative reference tồn tại;
- corruption khi migration copy không bao giờ switch verified location;
- backend sole-replica không thể retire.

### Version và Trash

- rename, move, copy, content replace, restore version cũ và conflict có chính
  xác tác động version đã đặc tả;
- recursive Trash nguyên tử đối với client mà không inline mutation
  O(subtree);
- offline descendant mutation đối đầu Trash tuân theo conflict recovery trong
  [SYNC.md](SYNC.md) và không bao giờ âm thầm hồi sinh/xóa byte;
- restore thành công về parent cũ, báo parent đã bị xóa và chỉ xử lý occupied
  name qua policy tường minh;
- purge race với restore có một bên thắng tại điểm không thể quay lại;
- process crash ở mọi purge batch có thể tiếp tục mà không làm visible một
  partial subtree;
- reference Trash/version/backup giữ byte sống; chỉ khi reference cuối hết hạn
  object mới đủ điều kiện GC;
- stale client nhận tombstone hoặc bắt buộc rebaseline, không bao giờ nhận false
  active node.

### Property/fuzz và operation

- key được sinh không bao giờ thoát namespace sở hữu với Unicode, separator,
  NUL-like input, reserved name hoặc long input tùy ý;
- transition reference/lease/retention ngẫu nhiên không bao giờ xóa protected
  object;
- expiry job lease ngẫu nhiên đảm bảo at-least-once execution và chỉ một
  generation có thể ghi completion;
- đối soát lặp lại là idempotent dưới page listing thiếu, trùng và đảo thứ tự;
- restore drill từ backup PostgreSQL/object/configuration phối hợp tìm thấy mọi
  object được tham chiếu và báo mọi object thừa là safe candidate.

## Observability và gate vận hành

Expose metric cho staged/final byte, upload/orphan age, object theo lifecycle,
replica missing/corrupt, verification age, GC candidate/delete failure, độ trễ
logical/physical accounting, latency/error class storage, backend capacity, job
queue age/lease/dead letter và backlog Trash/purge. Structured log dùng
operation/request ID và public domain ID khi policy cho phép, nhưng redact user
path, object key, hash khi nhạy cảm và mọi credential.

Production gate đòi hỏi adapter conformance, bằng chứng crash-injection, xử lý
disk-full, chế độ GC dry-run/report, backup/restore drill thành công và runbook
cho quarantine, root-identity mismatch, full disk, dead letter và orphan
backlog.

## Quyết định mở

OPEN DECISION OD-STORAGE-001: durability profile cục bộ
Owner: Storage / Operations
Needed by: Gate production adapter cục bộ Phase 1
Options: yêu cầu `fsync` data và directory cho mọi object đã commit; group durable flush với loss window có giới hạn tường minh; expose profile strict và relaxed để operator chọn
Recommendation: phát hành `STRICT` làm mặc định được ghi tài liệu và không cho phép profile relaxed cho tới khi test crash/power-loss trên filesystem được hỗ trợ định lượng và gắn nhãn rõ bảo đảm yếu hơn
Decision evidence: ma trận crash filesystem/container/NAS, benchmark latency và recovery drill

OPEN DECISION OD-STORAGE-002: so sánh namespace portable
Owner: Architecture / Storage / Sync
Needed by: Đóng băng schema và API Phase 1
Options: so sánh NFC phân biệt hoa thường; so sánh portable không phân biệt hoa thường; policy bất biến theo library
Recommendation: dùng comparison key portable không phân biệt hoa thường làm baseline, giữ nguyên display name gốc và version hóa chính xác algorithm normalization/folding Unicode trước migration
Decision evidence: fixture Unicode, normalization, reserved-name và case-collision trên Windows/macOS/Linux

OPEN DECISION OD-STORAGE-003: representation ban đầu và chiến lược range
Owner: Storage / API / Performance
Needed by: Gate format compression Phase 6; không chặn pass-through storage
Options: chỉ byte chuẩn pass-through; Zstandard toàn object với server decoding; chunk nén độc lập có version cùng seek index
Recommendation: ban đầu dùng byte pass-through; chỉ dùng compression sau khi bằng chứng range-read, corruption-repair và space/CPU đo được chọn một format có version
Decision evidence: benchmark corpus đại diện, conformance random-range, memory limit và corruption test

OPEN DECISION OD-STORAGE-004: policy quota phiên bản và Trash
Owner: Product / Storage / Backup
Needed by: Gate UI quota và retention Phase 2
Options: tính mọi logical byte được giữ lại; tính current/live byte cùng retention cap riêng; accounting cấu hình theo library
Recommendation: tính mọi logical byte được giữ lại vào giới hạn riêng theo domain minh bạch và luôn báo physical usage riêng; tránh để dedup thay đổi logical quota của người dùng
Decision evidence: product policy, abuse analysis, hành vi migration và UI usability test

OPEN DECISION OD-STORAGE-005: ma trận capability đa nền tảng ban đầu
Owner: Storage / Platform / Clients / QA
Needed by: cross-platform foundation gate và release adapter local/native đầu tiên
Options: chỉ portable pass-through; profile tăng tốc theo filesystem; capability
probe với fallback bảo thủ
Recommendation: implement một portable correctness profile trước, chỉ thêm
filesystem accelerator sau khi capability probe, crash evidence và hành vi
disable/fallback được version hóa
Decision evidence: fixture NTFS/ReFS/APFS/Btrfs/ext4/XFS/NAS, power-loss và
disconnect test, capability false-positive test và performance evidence

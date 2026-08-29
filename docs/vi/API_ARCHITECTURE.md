# Kiến trúc API Synveil

Trạng thái: **Foundation transport, browser/bootstrap auth, logical metadata,
exact-offset resumable upload HTTP transport, content-read bất biến trung lập
transport đã authorize theo owner và HTTP download full/single-range đã
implement, cùng metadata version-history bất biến, safe historical-version
restore và retention metadata của Trash đã authenticate; metadata purge
execution nội bộ, physical object GC nội bộ crash-safe và GC-worker
orchestration/reconciliation nội bộ bounded đã implement; nền tảng change
journal bền vững theo owner/library, checkpoint theo device, server change feed
một chiều bounded đã authenticate, checkpoint acknowledgment và bootstrap
snapshot/rebaseline logical materialized đã VALIDATED; client mutation
submission logical typed với UUID idempotency, fingerprint SHA-256 canonical,
optimistic precondition, conflict deterministic và journal integration chính
xác đã IMPLEMENTED; durable conflict record, manual conflict inspection và
explicit manual resolution cũng đã IMPLEMENTED; Prompt 37 implement enrollment
Device một lần, bearer scoped thu hồi được, desktop profile bền vững, native
SecretStore adapter và HTTP inbound production. Upload UI, automatic conflict
resolution, backup, sharing, desktop GUI, watcher và outbound generation còn
NOT IMPLEMENTED/PLANNED**

## Trạng thái đồng bộ tại gate Prompt 35 (lịch sử)

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

Tài liệu này xác định HTTP contract mục tiêu và blueprint cho
`api/openapi.yaml`. Foundation hiện triển khai transport health hữu hạn, subset
browser authentication (`POST /auth/login`, `POST /auth/logout`,
`GET /auth/session`, `GET /auth/csrf`) và subset first-run bootstrap
(`GET /system/bootstrap-status`, `POST /bootstrap/admin`). Setup, login,
session, route guard và logout shell của React được bao phủ bởi frontend test.
Subset metadata logical theo ownership cũng đã implement:

- `GET /libraries` với bounded pagination của library owner;
- `GET /libraries/{library_id}/nodes` với bounded pagination direct child active;
- `POST /libraries/{library_id}/nodes` cho logical directory rỗng;
- `GET/PATCH /nodes/{node_id}` cho read an toàn, rename và move;
- `POST /nodes/{node_id}/trash` và `POST /nodes/{node_id}/restore` cho đổi state
  logical có điều kiện.
- Node response thêm `trashed_at` an toàn, `restore_deadline` dẫn xuất và
  `purge_eligible` do server evaluate; internal retention và metadata-purge
  worker boundary không phải user API thông thường. Metadata purge giữ nguyên
  row Object/ObjectReplica và byte.
- `GET /nodes/{node_id}/versions` cho metadata version-history bất biến,
  newest-first và bounded;
- `GET /versions/{version_id}` cho direct safe metadata lookup dùng cùng
  immutable ID mà historical content route chấp nhận.
- `POST /nodes/{node_id}/versions/{version_id}/restore` cho owner đã
  authenticate restore bằng cách append một immutable version mới từ object
  historical đã verify.

Crate metadata expose các boundary `ChangeJournalService`, `DeviceSyncService`
và `SyncBootstrapService` trung lập transport. DeviceSyncService tạo một
checkpoint bền vững cho mỗi device đã
register và đang `ACTIVE` cùng library thuộc owner, đọc journal theo keyset tăng
dần sau checkpoint, trả stable high watermark và chỉ advance sau khi evidence
delivery có chữ ký được verify. Checkpoint không phải mutation authority. Sai
journal epoch hiện tại hoặc checkpoint thấp hơn minimum retained sequence trả
metadata rebaseline-required ổn định. Bootstrap service thiết lập baseline thay
thế nhất quán mà không xóa hay reset checkpoint khi start.

HTTP transport một chiều đã implement gồm:

- `GET /devices/{device_id}/libraries/{library_id}/checkpoint` — authenticated,
  đọc/tạo checkpoint ở lần dùng đầu;
- `GET /devices/{device_id}/libraries/{library_id}/changes?limit=100` — page
  logical tăng dần, private/no-store, bounded 1..500; và
- `POST /devices/{device_id}/libraries/{library_id}/changes/ack` — bounded,
  yêu cầu CSRF với browser, chỉ nhận token evidence page đã ký từ server;
- `POST /devices/{device_id}/libraries/{library_id}/rebaseline` — start/retry
  yêu cầu CSRF với browser, body bounded 2 KiB;
- `GET /devices/{device_id}/libraries/{library_id}/rebaseline/{bootstrap_id}/nodes`
  — page manifest logical bất biến private/no-store, mặc định 200 và tối đa
  1000 Node, dùng signed keyset cursor bounded; và
- `POST /devices/{device_id}/libraries/{library_id}/rebaseline/{bootstrap_id}/complete`
  — completion terminal-proof yêu cầu CSRF với browser, body bounded 2 KiB.
- `POST /devices/{device_id}/libraries/{library_id}/mutations` — submission
  một logical mutation typed, body bounded 16 KiB và yêu cầu CSRF; request có
  UUID mutation ID bền vững, fingerprint semantic canonical, base
  epoch/sequence và precondition revision/parent explicit. Success append đúng
  một journal event; stale state trả conflict an toàn đã persist; route không
  nhận file byte;
- `GET /devices/{device_id}/libraries/{library_id}/conflicts` — list conflict
  OPEN đã authenticate, không cần CSRF, private/no-store, limit 1..100 và cursor
  keyset `(created_at, conflict_id)` opaque có HMAC, bounded 384 byte;
- `GET /devices/{device_id}/libraries/{library_id}/conflicts/{conflict_id}` —
  inspection đã authenticate, không cần CSRF, private/no-store cho typed intent
  bất biến, server observation được ghi nhãn lịch sử, lifecycle và terminal
  resolution metadata; và
- `POST /devices/{device_id}/libraries/{library_id}/conflicts/{conflict_id}/resolve`
  — quyết định manual strict đã authenticate, yêu cầu CSRF, private/no-store,
  body bounded 16 KiB cùng resolution ID UUIDv7 bền vững.

Body resolution chỉ cho `ACCEPT_SERVER` hoặc `APPLY_CLIENT_INTENT`.
Accept-server chuyển OPEN thành DISMISSED mà không đổi Node, không publish
journal event và không advance checkpoint. Apply-client-intent bắt buộc current
revision mới, kể cả parent/destination revision cho move và restore, rồi dùng
chung executor canonical Prompt 34 trong transaction. Success commit đúng một
resource journal event thông thường cùng RESOLVED và linkage resolution. State
hiện tại stale persist `resolution_conflict` replay được, giữ conflict OPEN và
commit zero canonical mutation/event. Fingerprint SHA-256 canonical có type bind
conflict ID, action và presence/value của mọi fresh precondition; không hash raw
JSON. Cùng resolution ID/fingerprint trả chính xác kết quả đã commit sau mất
response; semantic khác trả `resolution_id_conflict`. Quyết định mới sau state
terminal trả `conflict_not_open`. Resource đã purge không thể resurrect nhưng
evidence conflict vẫn inspect được. Không policy tự động chọn action.

Mọi operation đều scope theo owner đã authenticate và library thuộc owner.
Sáu operation checkpoint/feed/rebaseline còn nhận Device bearer đã verify và
bind đúng Device trong route; chỉ principal đó được miễn browser CSRF.
Mutation và conflict route vẫn chỉ nhận browser session. Prompt 37 implement
enrollment một lần và Device credential thu hồi được như mô tả bên dưới.
Mutation route chỉ nhận năm kind đóng `CREATE_DIRECTORY`, `RENAME_NODE`,
`MOVE_NODE`, `TRASH_NODE`, `RESTORE_NODE`; không có arbitrary JSON patch,
object/replica identity, path, storage locator hay byte. Không có automatic
conflict engine, watcher, broker, WebSocket hay SSE trong slice này.

Route metadata dùng action path phân cách bằng slash vì path grammar hiện tại
của Axum không hỗ trợ parameter nối với literal suffix trong cùng segment.
Chúng vẫn chỉ là metadata. Repository cũng expose persisted upload-session
service trung lập transport qua route `/upload-sessions` đã authenticate cho
create/status/exact-offset append/complete/abort. Mutation node PATCH/upload
dùng CSRF boundary hiện có, stream raw byte chỉ qua upload path bounded riêng
và status là recovery path có thẩm quyền sau response mơ hồ. Client mutation
route ở trên là logical JSON strict và không mang byte. Content-read service đã
được
wire vào các route authenticate `GET /nodes/{node_id}/content` và
`GET /versions/{version_id}/content`. Các route này resolve current hoặc
immutable version theo owner, parse single range, verify SHA-256, đặt safe
header và stream có giới hạn mà không expose storage key hay physical path.
Download UI, automatic conflict resolution, backup, sharing, guided pairing UI và
product endpoint khác bên dưới vẫn là kế hoạch. Các endpoint one-way sync,
rebaseline và logical client mutation phía trên đã IMPLEMENTED/VALIDATED theo
status tương ứng.
Version-history read và version restore được authenticate;
read không cần CSRF nhưng restore cần CSRF, signed current-node `If-Match` và
`Idempotency-Key` bounded. Cả hai chỉ cho active file, private/no-store và
không mở ObjectStore; node/version unknown, cross-owner, trashed và purging
đều bị che giấu. Restore transaction lock và recheck quan hệ
owner/library/file/source cùng một replica `VERIFIED` matching, insert đúng một
`FileVersion` mới có parent là version current trước đó, advance revision node
và không tạo `UploadSession`. Ý
nghĩa và state entity chuẩn đến từ [DOMAIN_MODEL.md](DOMAIN_MODEL.md); đặc tả
storage, upload, sync, backup, photo, AI và integration tinh chỉnh hành vi mà
không phát minh ID, error hay mutation semantic thay thế.

GC planning và execution là boundary nội bộ trung lập transport, cố ý không có
HTTP route hay OpenAPI operation. Execution chỉ nhận candidate `READY` có
lease/generation matching còn hạn, lặp proof reference/hold trong transaction
PostgreSQL ngắn trước mỗi action, ghi operation bền trước ObjectStore I/O và
xóa/đối soát từng replica đã verify. `GC_DELETING` reject FileVersion, replica
và active hold mới; completion chỉ dọn metadata sau khi mọi replica absent.
`synveil-worker` opt-in gọi các service đã chấp thuận đó qua cycle `run_once()`
nội bộ bounded, không có HTTP route, OpenAPI operation hay control dành cho
người dùng thông thường. Producer hold backup/share/sync vẫn PLANNED, không
được suy ra từ status API nội bộ này.

## Ranh giới contract

Mọi product client được hỗ trợ dùng API có version:

```text
/api/v1
```

Hai canonical operational probe HTTP không có version là `/health/live` và
`/health/ready`. Foundation cũng phục vụ `/live` và `/ready` như compatibility
alias ngắn. Các route này là signal deployment, không phải API resource sản
phẩm, và không expose version, topology hay detail dependency. Operational view
restricted vẫn nằm ở `/api/v1/system/health`.

Caddy có thể terminate TLS và route traffic, nhưng hành vi proxy không thuộc
domain semantic. Client không bao giờ kết nối PostgreSQL, tạo object-store key
hay gọi optional worker trực tiếp.

API chịu trách nhiệm:

- xác thực principal và authorize mọi resource action;
- parse typed input hữu hạn trước khi domain execution;
- enforce quota, rate limit, idempotency và optimistic concurrency;
- stream byte mà không load file hoàn chỉnh vào memory;
- ánh xạ domain outcome sang HTTP contract và machine error contract ổn định;
- commit metadata, journal, audit và outbox fact cùng nhau; và
- expose trung thực freshness và asynchronous-operation state.

API không chịu trách nhiệm biến thành công của thumbnail, OCR, AI, notification
hoặc Forgejo indexing thành prerequisite cho core content commit.

## Protocol profile

### Transport và media type

- Production traffic dùng HTTPS. Plain HTTP chỉ được phép trên container hop
  private rõ ràng phía sau TLS proxy được cấu hình đúng.
- JSON request và response body dùng `application/json` với UTF-8.
- Canonical content upload và download dùng binary body với `Content-Type` rõ
  ràng cùng `Content-Length` hữu hạn hoặc streaming framing đã review.
- Server từ chối media type không được hỗ trợ và duplicate header mơ hồ.
- Empty response thành công dùng `204 No Content`. JSON response không bao giờ
  trả empty body trong khi khai báo `application/json`.
- Compression cho API JSON hoặc download response được negotiate ở HTTP layer;
  nó không đổi canonical integrity hash của `Object`.

### Name, ID, time và number

- JSON property name là `snake_case`.
- Public ID là chuỗi UUIDv7 mờ đục. Lexical order của chúng không có ý nghĩa API.
- Instant là chuỗi UTC RFC 3339. Giá trị chỉ có date dùng `YYYY-MM-DD` chỉ khi
  domain thực sự biểu diễn civil date.
- Revision, change/entry sequence, byte length/count, quantity quota/accounting
  và mọi giá trị không âm khác có thể vượt safe integer range của JavaScript
  được biểu diễn chuẩn bằng chuỗi thập phân unsigned. OpenAPI dùng một shared
  scalar có pattern `^(0|[1-9][0-9]*)$` cùng extension/format
  `uint64-decimal` có tên. Leading sign, whitespace, exponent notation,
  fraction và leading zero đều không hợp lệ.
- Bounded protocol control như page `limit`, part index, attempt count và
  enumerated progress percentage vẫn là JSON integer với schema minimum/maximum
  rõ ràng. Một field không bao giờ đổi giữa number và string theo client, value
  hay deployment.
- ETag, keyset/change cursor, ID và hash vẫn là opaque string, không phải numeric
  scalar.
- Hash value gồm algorithm, ví dụ `sha256:<lowercase-hex>`. Client không dùng
  hash làm object ID.
- Unknown JSON property trong request bị từ chối đối với command nhạy cảm về
  security và được xử lý theo reviewed schema ở nơi khác. Response có thể thêm
  optional field; client bỏ qua field không hiểu.
- Nullable và omitted khác nhau. Omitted nghĩa là “không được cung cấp/không
  được chọn”; `null` chỉ được phép khi schema gán một domain action.

### Resource representation

Thành công cho single resource dùng:

```json
{
  "data": {
    "id": "opaque-uuidv7",
    "type": "node",
    "revision": "7",
    "attributes": {}
  },
  "meta": {
    "request_id": "opaque-request-id"
  }
}
```

Resource collection thông thường dùng array `data` và:

```json
{
  "page": {
    "next_cursor": "opaque-or-null",
    "has_more": true
  },
  "meta": {
    "request_id": "opaque-request-id"
  }
}
```

Feed riêng theo protocol có thể dùng specialized envelope đã đăng ký khi
semantic yêu cầu. Cụ thể, sync contract dùng `events`, `next_cursor` và
`has_more` như định nghĩa trong [SYNC.md](SYNC.md); nó không âm thầm luân phiên
giữa generic envelope và feed envelope.

Resource relationship có thể được biểu diễn bằng ID hoặc link, nhưng recursive
embedding là hữu hạn và không bao giờ thay thế authorization. Internal field
nhạy cảm—password verifier, token hash, storage key, secret reference, raw
provider error và server path—không có public schema.

### Request correlation

- Server trả `X-Request-Id` trên mọi response và phản chiếu value trong JSON
  error/meta body.
- Client request ID hợp lệ về syntax có thể được nhận làm correlation hint,
  nhưng server tạo trusted trace identity riêng và ngăn log injection.
- Request ID là diagnostic identifier, không phải idempotency key hay credential.

## Authentication và authorization

### Credential profile dự kiến

- Browser session dùng opaque credential entropy cao trong cookie `Secure`,
  `HttpOnly` với policy `SameSite` phù hợp. Server chỉ lưu verifier/hash.
- State-changing request được xác thực bằng cookie cần CSRF defense đã chọn và
  allowed-origin check; CORS mặc định từ chối.
- Device/API client dùng opaque bearer credential có scope độc lập, có thể
  rotate và revoke, liên kết với `Device` hoặc API grant. Credential đi trong
  header `Authorization`, không bao giờ trong URL và không tái sử dụng password
  của user.
- Refresh/rotation là one-time và phát hiện replay. Logout/revocation là
  idempotent.
- Public share access dùng bounded capability flow riêng; nó không bao giờ trở
  thành user session thông thường hay lộ credential của source owner.

Mọi operation còn authorize action dựa trên current ownership, quan hệ
library/share, device scope và resource state. Có thể trả `404` thay cho `403`
khi phân biệt existence sẽ làm rò rỉ resource của user khác.

Authentication endpoint cần có abuse control ở deployment/application, nhưng
phase này không có distributed rate-limiter subsystem. Log, error, response
list/read và mọi response trừ first one-time issuance response được nêu tường
minh không bao giờ chứa password, refresh secret, credential API/device,
capability share, recovery code, object-store credential hay Git integration
token. Response bootstrap và browser authentication dùng `Cache-Control:
no-store`, bị redact khỏi tracing và loại raw material khỏi persisted
idempotency outcome cùng replay.

## Conditional mutation

Mutable resource read trả quoted `ETag` dẫn xuất từ opaque resource identity và
domain `revision`. Encoding không do client tự tạo và không expose SQL
transaction ID.

- `PATCH`, `PUT` và `DELETE` trên existing mutable resource yêu cầu `If-Match`
  trừ khi endpoint có named base-version field mạnh hơn.
- Endpoint relation desired-state đã document có thể là ngoại lệ tường minh:
  ví dụ PUT favorite nghĩa là “bảo đảm relation của tôi tồn tại” và DELETE là
  “bảo đảm nó vắng mặt”. Endpoint đó phải idempotent, phải nêu ngoại lệ trong
  OpenAPI và phải tôn trọng relation `If-Match` được gửi; caller không được suy
  ra ngoại lệ này cho mutable resource thông thường.
- Recursive subtree command còn trình opaque subtree precondition do server cấp
  được định nghĩa bởi OD-SYNC-004 trong [SYNC.md](SYNC.md). Nó tách khỏi node
  metadata ETag để client không thể phê duyệt deletion bằng view có trước một
  descendant edit. Field/header OpenAPI chính xác và backing schema được đóng
  băng tại gate schema/API Phase 1 trước Trash recursive Phase 2, không do từng
  client phát minh; Phase 4 tái sử dụng contract đó cho sync.
- Thiếu precondition trả `precondition_required`.
- Precondition stale trả `version_conflict` cùng safe current revision/ETag chỉ
  khi caller vẫn được phép read resource.
- Thay file content khai báo `base_version_id` trong initiation. Sync conflict
  policy có thể giữ stale offline edit thay vì chỉ từ chối, nhưng không bao giờ
  âm thầm overwrite byte.
- Immutable version, committed snapshot, audit fact và repository backup
  manifest không được patch. Restore tạo resource/state mới.
- Collection-level command validate precondition của từng target hoặc dùng
  operation snapshot do server tạo. Chúng không bao giờ áp dụng wildcard không
  giới hạn, chỉ được authorize một phần.

## Idempotency và retry

### Key bắt buộc

`Idempotency-Key` bắt buộc cho command không naturally-idempotent, gồm:

- upload initiation khi replay có thể tạo session trùng;
- upload completion;
- node copy và large/bulk operation;
- restore file version, trash entry, backup snapshot hoặc repository;
- tạo share/public link;
- issue hoặc rotate credential API/device và generate recovery-code set;
- commit backup snapshot và repository backup; và
- integration-triggered backup hoặc reindex command.

Key là opaque client random value có length hữu hạn. Server scope key theo
authenticated principal, operation family và target boundary; hash canonical
request fingerprint; đồng thời lưu terminal HTTP status, safe header, response
body đã redact secret và created resource ID trong cùng transaction với outcome.

Retry giống hệt trả stored outcome và đánh dấu response là replay qua header đã
ghi. Tái sử dụng key với material input khác trả `idempotency_conflict`.
Concurrent duplicate chờ hoặc quan sát single outcome; nó không execute độc lập.

Endpoint cấp secret là ngoại lệ được nêu tên cho replay response body. Raw
credential API/device, capability public link và recovery-code set chỉ được trả
trong response thành công đầu tiên, bị loại khỏi persisted idempotency outcome
và không bao giờ tái tạo. Replay cùng key trả safe candidate metadata với
`one_time_secret_unavailable`. Vì candidate vẫn `PENDING` và không usable,
caller được authorize có thể revoke hoặc replace tường minh bằng issuance
command cùng key mới; retry giống hệt không bao giờ âm thầm tạo secret khác.

### Phân loại retry

- Client chỉ có thể automatic retry khi error nói `retryable: true` và method
  naturally idempotent hoặc có persisted idempotency key.
- `Retry-After` được trả cho rate limiting và bounded service backoff khi biết.
- Transport failure sau khi gửi body là unknown outcome. Client query
  resource/session hoặc retry cùng key; không được chọn key mới tới khi đã quan
  sát outcome. Với endpoint one-time-secret, safe replay sau đó hướng dẫn
  command revoke/replace tường minh thay vì replay secret material.
- Server không bao giờ báo core mutation thành công trước khi database
  transaction commit và object được tham chiếu bền vững, đã verify.

## Pagination, sorting, filtering và snapshot

Collection thông thường dùng keyset cursor:

- `limit` bị giới hạn bởi operation OpenAPI;
- `cursor` mờ đục, có version, được bảo vệ toàn vẹn và scope theo principal,
  collection, filter, sort và authorization context;
- `next_cursor: null` và `has_more: false` kết thúc traversal;
- đổi filter hoặc sort cần traversal mới;
- malformed token trả `invalid_cursor`, còn retained snapshot/change position
  hết hạn trả domain-specific expiry response.

Offset pagination không phải public default. Supported sort key được enumerate;
arbitrary field name hoặc SQL expression bị từ chối. Mọi sort có immutable
unique tie-breaker, thường là resource ID.

Search cursor còn bind query normalization, search layer, index generation và
authorization scope. Nếu index đổi quá nhiều để tôn trọng cursor, API cho cursor
hết hạn thay vì âm thầm trộn ranking không tương thích.

### Sync rebaseline

Bootstrap server-side đã implement dùng manifest logical materialized, không
dùng high watermark rồi đọc row `nodes` mutable ở các request sau:

1. Start lấy namespace guard theo library trong transaction hiện có, lock
   library/head và checkpoint đúng scope, verify owner đã authenticate, device
   `ACTIVE` và library thuộc owner, rồi tăng generation rebaseline của
   checkpoint.
2. Trong chính PostgreSQL transaction đó, service capture `snapshot_epoch` và
   `snapshot_resume_sequence`, copy canonical root cùng projection Node
   `ACTIVE`/`TRASHED` hiện tại vào `sync_bootstrap_nodes`, rồi seal item count và
   terminal Node ID bất biến. Row `PURGING`/đã purge, historical version, byte
   và toàn bộ field storage vật lý đều bị loại.
3. GET sau đó chỉ page manifest bền theo `node_id > after_node_id`, không dùng
   `OFFSET` hay thứ tự name/path mutable. Mặc định là 200, tối đa 1000 và SQL chỉ
   đọc `limit + 1`; không buffer toàn library trong Rust.
4. Cursor tối đa 320 byte bind version, owner, device, library, session,
   generation, epoch, resume sequence và Node ID cuối bằng HMAC domain riêng.
   Chỉ terminal page thật trả token tối đa 336 byte còn bind manifest count và
   terminal marker bằng HMAC domain khác. Token chứng minh integrity, không cấp
   authorization.
5. Completion recheck scope, expiry theo PostgreSQL/server time, state session,
   generation, journal epoch hiện tại, retained-history boundary, terminal
   proof và vị trí checkpoint trong khi giữ row lock. Cùng transaction đánh dấu
   `COMPLETED` và đặt checkpoint chính xác bằng epoch/resume sequence đã capture;
   progress mới hơn không bao giờ bị rewind.
6. Replay completion đã commit trả kết quả completed hiện tại. Change feed
   thường sau đó bắt đầu nghiêm ngặt sau resume sequence đã capture.

Thiết kế này không giữ database transaction qua nhiều HTTP request nhưng mọi
page vẫn thuộc một cut có thể restart. Mutation serialize trước capture nằm
trong manifest; mutation serialize sau capture có journal sequence lớn hơn cut.
Vì vậy không logical mutation đã commit nào vắng ở cả snapshot lẫn feed tiếp
theo. Expiry, replacement generation, epoch rotation hay retention invalidation
đều fail closed và buộc bootstrap mới.

## Error contract

Mọi non-success JSON response dùng:

```json
{
  "error": {
    "code": "version_conflict",
    "message": "The resource changed before this request was applied.",
    "request_id": "opaque-request-id",
    "retryable": false,
    "details": {
      "resource_type": "node",
      "current_revision": "8"
    }
  }
}
```

`code` ổn định và machine-readable. `message` an toàn, human-readable và không
dùng để branch. `details` là allow-listed schema theo code và không được chứa
stack trace, SQL, filesystem path, secret value, raw provider body, ID của user
khác hoặc content excerpt. Internal diagnostic được correlate bằng request ID
trong redacted log.

### Ranh giới trình bày error

API contract là nguồn ổn định nằm dưới mọi client surface. Response có thể có
non-secret remediation/action key bên cạnh code, retryability, message/detail
an toàn và request ID; không bao giờ expose raw path, secret, provider body,
stack trace hoặc privileged diagnostic bundle. Cùng một state nền được trình
bày ở ba mức:

- **User:** ngôn ngữ dễ hiểu, safe action tiếp theo và việc retry có an toàn hay
  không; user thông thường không cần đọc HTTP, SQL, filesystem hay service
  terminology.
- **Administrator:** explanation cho user cộng stable code, health context,
  backend/service bị ảnh hưởng và diagnostic reference được authorize.
- **Developer/operator:** structured diagnostic đã redact, correlate bằng
  request ID, giữ failure classification gốc và evidence remediation.

Presentation layer không được tạo state khác hoặc âm thầm biến unknown outcome
thành success. Ví dụ, storage backend đầy được map thành “Storage đã đầy; hãy
chọn vị trí khác hoặc giải phóng dung lượng”, database unavailable thành
“Synveil đang sửa local service; hãy thử lại sau ít phút”, pairing code hết hạn
hoặc đã dùng thành “Hãy tạo pairing code mới”, filesystem capability không hỗ
trợ thành “Vị trí storage này không thực hiện được action này”, signed update
thất bại thành “Update chưa được cài; giữ version hiện tại và xem chi tiết
update”, migration cần restore thành “Hãy restore từ recovery package đã verify
trước khi tiếp tục”, và remote access unavailable thành “Local access vẫn hoạt
động; hãy kiểm tra network path đã cấu hình”.

Các message này là product copy, không phải error semantic thứ hai. Stable code,
retry rule, safe action, request ID và authorization boundary vẫn có sẵn cho
client và diagnostic.

### Error registry baseline

| HTTP | Code | Ý nghĩa và quy tắc retry |
|---:|---|---|
| 400 | `invalid_request` | Syntax, schema hoặc bounded validation thất bại; không retry khi chưa đổi input. |
| 400 | `invalid_cursor` | Cursor malformed, sai version, sai query hoặc sai resource scope; theo restart route của operation. |
| 401 | `authentication_failed` | Credential thiếu, không hợp lệ, hết hạn, đã rotate hoặc revoke. |
| 403 | `permission_denied` | Authenticated principal thiếu action; không automatic retry. |
| 404 | `not_found` | Resource vắng mặt hoặc bị authorization policy cố ý che giấu. |
| 409 | `version_conflict` | ETag/base version hoặc concurrent state không còn khớp. |
| 409 | `name_conflict` | Destination comparison key đã tồn tại. |
| 409 | `idempotency_conflict` | Key đã được bind với material input khác. |
| 409 | `one_time_secret_unavailable` | Không thể replay response chứa secret đầu tiên; candidate được trả là inert và phải inspect, revoke hoặc replace tường minh. |
| 409 | `invalid_state` | Resource tồn tại nhưng transition không hợp lệ từ current state. |
| 409 | `part_conflict` | Upload part identity đã có verified content khác. |
| 409 | `completion_conflict` | Upload completion identity/manifest conflict với stored attempt. |
| 409 | `upload_sealed` | Upload manifest đã đóng băng và không còn nhận part mutation. |
| 409 | `cursor_epoch_mismatch` | Cursor thuộc library journal epoch cũ/mới; thực hiện directed bootstrap. |
| 409 | `unsupported_event_version` | Client không thể áp dụng an toàn event schema bắt buộc; dừng tiến cursor và upgrade/recover. |
| 410 | `cursor_expired` | Retained change/bootstrap/search position đã mất; theo rebootstrap instruction. |
| 410 | `upload_expired` | Upload session không thể nhận part hoặc completion nữa. |
| 412 | `checksum_mismatch` | Observed byte không khớp declared checksum; chỉ retransmit verified source byte. |
| 413 | `payload_too_large` | Body, part, manifest, result set hoặc operation count vượt documented bound. |
| 415 | `unsupported_media_type` | Representation không được endpoint này chấp nhận. |
| 422 | `invalid_manifest` | Upload part/range bắt buộc bị thiếu, overlap, không nhất quán hoặc không hợp lệ theo cách khác. |
| 428 | `precondition_required` | `If-Match` hoặc base version bắt buộc đã bị bỏ qua. |
| 429 | `rate_limited` | Retry theo `Retry-After`; key outcome chưa được áp dụng trừ khi có tài liệu khác. |
| 500 | `internal_error` | Failure ngoài dự kiến; outcome của mutation đã gửi chưa rõ, vì vậy tái sử dụng cùng key. |
| 503 | `storage_unavailable` | Required backend/durability check thất bại; chỉ retry khi được chỉ định. |
| 503 | `internal_dependency_unavailable` | PostgreSQL hoặc required internal dependency riêng cho operation không sẵn có. |
| 507 | `quota_exceeded` | Logical/staging/account quota reservation thất bại; không retry đến khi capacity hoặc policy đổi. |

Operation-specific code phải được đăng ký trong một OpenAPI enum/extension
registry cùng privacy-reviewed details schema. Backup protocol dành riêng
`backup_set_paused`, `snapshot_not_restorable`, `snapshot_incomplete`,
`manifest_conflict`, `capture_inconsistent`, `object_corrupt`,
`restore_conflict`, `unsupported_entry_type` và `device_revoked` ngoài common
code. Integration protocol và AI protocol tương tự đăng ký
`integration_reauth_required` và `ai_disabled` thay vì trả raw provider/worker
string. Adapter ánh xạ failure vào registry này; provider-specific diagnostic
không thoát ra.

## Asynchronous operation contract

Command có thể traverse subtree, restore nhiều entry, tạo snapshot lớn hoặc gọi
external integration trả `202 Accepted` với
`Location: /api/v1/operations/{operation_id}`.

Representation `Operation` gồm:

- opaque ID, type, owner và target link;
- state `QUEUED`, `RUNNING`, `SUCCEEDED`, `PARTIAL`, `FAILED`,
  `CANCEL_REQUESTED` hoặc `CANCELED`;
- instant created/started/finished;
- bounded progress count/byte khi có ý nghĩa, không bao giờ bịa percentage;
- safe terminal error hoặc paginated per-item result;
- idempotency key correlation và ETag/revision.

Cancellation có điều kiện và best-effort. Nó không bao giờ rollback child
mutation đã commit bằng cách giả vờ distributed work là nguyên tử. Partial
operation nêu mọi item đã commit, skip, conflict và fail để retry nhắm vào phần
còn lại.

## Danh mục endpoint và trạng thái implementation

Bốn browser authentication route, hai bootstrap route, metadata route,
exact-offset upload-session route và hai content download route authenticate
nêu trên là `IMPLEMENTED` và được đặc tả trong `api/openapi.yaml`. Các endpoint
group không được đánh dấu bên dưới là
`PLANNED`. Ngoại trừ hai probe absolute `/health/*` được nêu rõ, path
trong inventory là relative với `/api/v1`. Collection route không bao giờ loại
bỏ nhu cầu authorize từng resource được trả.

### Bootstrap, authentication và user

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `GET /system/bootstrap-status` | **IMPLEMENTED** — Chỉ báo setup có cần thiết hay không, không kèm configuration secret. | Public status bounded; `no-store`; deployment phải giữ first-run exposure ở mạng trusted/private hoặc TLS terminate đúng cách. |
| `POST /bootstrap/admin` | **IMPLEMENTED** — Tạo administrator đầu tiên qua bootstrap service race-safe hiện có và đóng setup. | Chỉ public khi setup open; JSON strict 16 KiB; provenance same-origin khi có browser header; không có setup-secret field hoặc automatic session; sau đó login tường minh. |
| `POST /auth/login` | **IMPLEMENTED** — Verify canonical login identifier/password và issue browser session cookie. | Public; JSON input bounded; raw session chỉ ở cookie Secure/HttpOnly; không có raw token trong JSON. |
| `POST /auth/refresh` | Consume nguyên tử refresh credential và cấp successor; phát hiện replay. | Existing refresh grant; response bị mất mơ hồ fail-closed và cần đăng nhập lại, không transparent retry. |
| `POST /auth/logout` | **IMPLEMENTED** — Revoke browser session hiện tại và clear cookie session/CSRF. | Session hợp lệ cần CSRF bound theo session và provenance same-origin; logout thiếu/expired/revoked được lặp an toàn. |
| `GET /auth/session` | **IMPLEMENTED** — Trả về current browser principal an toàn. | Browser session authenticated; không token, verifier, password hay database row. |
| `GET /auth/csrf` | **IMPLEMENTED** — Cấp proof mới ký và bound với browser session hiện tại. | Browser session authenticated; response `no-store`; proof đi trong `X-CSRF-Token`, không dùng query parameter. |
| `GET /sessions` | List metadata an toàn của browser/API session của caller, marker current session, expiry, last use và revocation state; device grant link tới Devices. | Authenticated owner; keyset pagination; không token/verifier. |
| `POST /sessions/{session_id}:revoke` | Revoke độc lập một browser/API session family được sở hữu; đây là public authority duy nhất để revoke API grant. | Authenticated owner; `If-Match` và idempotency key; current session còn clear cookie; device revoke dùng endpoint Device. |
| `GET /api-grants` | List named API grant của caller, scope, status, expiry, last use và safe generation metadata. | Authenticated owner; keyset pagination; không credential/verifier. |
| `POST /api-grants` | Tạo API grant/generation `PENDING` có giới hạn và display raw bearer secret đúng một lần; chưa dùng được tới activation. | Recent step-up; `Idempotency-Key` bắt buộc; scope/expiry least-privilege rõ ràng; audited/rate-limited; quy tắc one-time-secret; response mất để lại pending grant sẽ expire. |
| `POST /api-grants/{grant_id}:activate` | Activate API grant pending mới cấp sau khi owner confirm đã lưu secret. | Owner; grant `If-Match` cộng idempotency key; không trả raw secret. |
| `POST /api-grants/{grant_id}/credentials:rotate` | Với key mới, expire/replace nguyên tử pending candidate trước và generate một replacement pending trong khi generation hiện tại vẫn active. | Owner với recent step-up; grant `If-Match`; `Idempotency-Key` bắt buộc; quy tắc one-time-secret. |
| `POST /api-grants/{grant_id}/credentials/{generation}:activate` | Activate replacement đã lưu và retire prior credential generation nguyên tử. | Owner; generation precondition cộng idempotency key. |
| `GET /users/me/recovery-code-status` | Read active/pending generation, count code còn lại và safe timestamp. | Authenticated owner; không bao giờ trả verifier hay raw code. |
| `POST /users/me/recovery-code-sets` | Với key mới, expire/replace nguyên tử pending candidate trước; generate một pending replacement set có giới hạn và display raw code một lần trong khi active set cũ vẫn hợp lệ. | Recent step-up authentication; `Idempotency-Key` bắt buộc; audited/rate-limited; quy tắc one-time-secret; response mất chỉ để lại pending candidate sẽ expire. |
| `POST /users/me/recovery-code-sets/{set_id}:activate` | Confirm code đã được lưu, activate pending set, retire prior set nguyên tử và expire pending recovery transaction/reservation của set đó. | Authenticated owner; candidate `If-Match` cộng idempotency key; không trả raw code. |
| `POST /auth/recovery/code-exchanges` | Validate/reserve một active code chưa dùng; chuyển nguyên tử transaction cũ của nó từ `PENDING` sang `EXPIRED`, rebind reservation và cấp một lần secret của short-lived transaction `PENDING`; chưa consume code. | Public, chống enumeration, serialize, strict rate limit theo account/source, field secret được redact; retry cùng code replace response mất an toàn. |
| `POST /auth/recovery/password-reset` | Revalidate set/generation được tham chiếu vẫn `ACTIVE`; consume nguyên tử cả current recovery transaction và code đã reserve, thay password, tăng `session_epoch` chuẩn, revoke mọi grant `WEB`/`API`/`DEVICE` và pause device được giữ. | Public với current transaction one-use hợp lệ; audited; không trả normal session; mọi client phải reauthenticate. |
| `GET /users/me` | Read safe profile và capability của current user. | Authenticated owner. |
| `PATCH /users/me` | Đổi profile/account setting được phép. | Authenticated owner; `If-Match`. |
| `GET/POST /admin/users` | List/create account theo instance policy. | Instance administrator; keyset pagination; create cần idempotency key. |
| `GET/PATCH /admin/users/{user_id}` | Read/lock/disable/administer account mà không âm thầm purge data. | Instance administrator; `If-Match`; audited. |

Bootstrap status fail closed khi database state unavailable hoặc mơ hồ; create
operation dùng serialized PostgreSQL service boundary hiện có nên chỉ một claim
administrator thành công. Bootstrap đã đóng không bao giờ được re-enable bằng
cách xóa browser cookie. HTTP contract hiện tại không có setup-secret field:
cho tới khi có installer hoặc secret-gate contract được review, operator phải
chỉ expose first-run endpoint trên mạng trusted/private hoặc TLS terminate đúng
cách và dùng canonical origin cấu hình. Endpoint có body bound và provenance
check nghiêm ngặt, nhưng phase này chưa implement distributed rate limiter.

Response create thành công chỉ chứa `setup_required=false` và metadata
correlation. Nó không issue browser session; web client chuyển sang login tường
minh. Password, verifier, cookie và raw session/CSRF material không bao giờ
được trả về hoặc log trong bootstrap response.

Browser refresh ưu tiên ứng phó theft hơn availability trong suốt. Nếu
transaction consume-and-issue commit nhưng response bị mất, reuse credential đã
consume sẽ đặt tất định `Session` đó cùng mọi credential chưa terminal thành
`REVOKED`; client discard nó và thực hiện login mới. Cả ambiguous loss và
malicious replay trả response generic `authentication_failed`, còn audit ghi
`REFRESH_REPLAY_DETECTED`. Không có public quarantine state và credential cũ
không bao giờ được chấp nhận lại.

Contract recovery ban đầu chỉ dùng recovery code. Candidate đã generate chưa
active tới khi confirm tường minh, nên mất response một lần không invalidate
active set trước. Code exchange reserve thay vì tiêu code: retry replace pending
transaction không thể tiếp cận và chỉ reset password mới consume nguyên tử cả
current transaction lẫn code. Không endpoint nào nhận email/admin override. Các path tùy chọn
đó vẫn bị chặn bởi OD-005. Session revocation được check ở request xác thực kế
tiếp, chỉ chịu bounded cache lag đã document.
Issue/rotation API grant dùng cùng safety rule pending-then-activate như
replacement recovery code: không response một lần bị mất nào tạo secret usable
không biết. Grant chỉ mang explicit bounded scope và không bao giờ khuếch đại
authorization hiện tại của owner.

`ApiGrant` là projection riêng cho API của `Session.kind = API`; `grant_id`
chính là `Session.id`, không phải identity thứ hai. Credential generation là
record con, nên retire một generation không tạo session status `RETIRED`.
`POST /sessions/{session_id}:revoke` là public command duy nhất để revoke family
và revoke nguyên tử mọi generation còn lại; `/api-grants` cố ý không định nghĩa
route revoke cạnh tranh.

### Device và credential

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `POST /devices` | Đăng ký named device `PENDING` và negotiate protocol capability mà không cấp credential. | Authenticated user; idempotency key ordinary có thể replay. |
| `GET /devices` | List device của caller cùng freshness/status. | Owner; keyset pagination. |
| `GET/PATCH /devices/{device_id}` | Read hoặc rename/pause device, policy và safe credential-generation status. | Owner; `If-Match`; không verifier/raw credential. |
| `POST /devices/{device_id}/credentials` | Với device `PENDING` mới, issue/replace một pending initial credential; với device `PAUSED` do reset password, tạo và bind `Session`/generation `DEVICE` pending mới trong khi family cũ vẫn revoked. Display raw bearer secret một lần. | Owner với recent step-up; device `If-Match`; `Idempotency-Key` bắt buộc; quy tắc one-time-secret. |
| `POST /devices/{device_id}/credentials:rotate` | Issue hoặc replace một pending rotation candidate trong khi generation hiện tại vẫn active; display candidate secret một lần. | Owner với recent step-up; device `If-Match`; quy tắc idempotency one-time-secret. |
| `POST /devices/{device_id}/credentials/{generation}:activate` | Activate initial/replacement generation đã lưu; initial activation activate device, còn rotation retire nguyên tử generation cũ. | Owner đã authenticate với recent step-up; generation `If-Match` cộng idempotency key; không trả secret và pending credential không thể tự activate. |
| `POST /devices/{device_id}:revoke` | Revoke credential và future activity; tùy chọn queue request best-effort để xóa cache do Synveil quản lý. | Owner; `If-Match` cộng idempotency key; audited; cache request không phải bằng chứng đã thực thi. |
| `GET /devices/{device_id}/status` | Read trạng thái sync, backup, storage/cache và last-contact cùng freshness. | Owner; không client claim nào được coi là verified server health. |

Không response nào tuyên bố xóa sạch hệ điều hành từ xa. Device offline hoặc bị
compromise có thể không bao giờ xử lý managed-cache request; revocation server
credential là hành động security có thẩm quyền.

Các nhóm platform contract tương lai sẽ bao phủ short-lived pairing session,
storage discovery và capability evidence, layered health/maintenance status,
signed update state, uninstall/data-preservation state, machine migration và
recovery, cùng remote-access configuration/status. Chúng có thể chạy qua
authenticated API, local platform IPC hoặc cả hai. Không thêm public endpoint
đầu cơ cho đến khi schema, authorization, idempotency, redaction và recovery
semantic ổn định; [`api/openapi.yaml`](../../api/openapi.yaml) vẫn không đổi
trong revision tài liệu này.

Pending device credential không thể authenticate. Mất issuance response vì thế
giữ prior state của device an toàn: không active credential khi registration
ban đầu, hoặc generation active cũ trong rotation. Sau khi quan sát
`one_time_secret_unavailable`, owner replace tường minh pending candidate bằng
issuance cùng key mới; activation là điểm duy nhất retire generation cũ.

Reset password chuyển device chưa revoke được giữ thành `PAUSED` và giữ mỗi old
credential family ở `REVOKED`. Re-enrollment bằng owner step-up qua
`POST /devices/{device_id}/credentials` tạo/rebind fresh pending family; chỉ
activation mới trả device về `ACTIVE`. Flow này không bao giờ hồi sinh device
record đã revoke.

### Library, node, file và folder

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `POST /libraries` | Tạo ownership/policy/sync namespace và root node. | Authenticated user; idempotency key. |
| `GET /libraries` | **IMPLEMENTED** — List library do authenticated user sở hữu với opaque cursor bounded. | Authenticated owner; library không access không xuất hiện. |
| `GET/PATCH /libraries/{library_id}` | Read hoặc update safe library metadata/policy. | Authorized owner/admin; `If-Match`. |
| `GET /libraries/{library_id}/nodes` | **IMPLEMENTED** — List direct child active dưới root hoặc directory active theo node ID ổn định. | Authenticated owner; opaque cursor bounded bind theo library và parent. |
| `GET /libraries/{library_id}/favorites` | List favorite node active mà caller còn đọc được, không expose relation không còn access. | Current user và library read; keyset pagination; authorize lại từng result. |
| `GET /recent-nodes?library_id=...` | List active node caller đọc được theo mutation server-side đã commit gần đây, tùy chọn trong một library; đây không phải view tracking. | Authenticated; keyset cursor có snapshot watermark và bind caller/filter; authorize lại từng result. |
| `POST /libraries/{library_id}/nodes` | **IMPLEMENTED** — Chỉ tạo logical directory rỗng; không có upload intent hoặc directory vật lý. | Authenticated owner; JSON strict 16 KiB; raw logical name; duplicate sibling vẫn được schema hiện tại cho phép. |
| `GET/PATCH /nodes/{node_id}` | **IMPLEMENTED** — Read metadata an toàn hoặc thực hiện một rename/move có điều kiện. | Node owner; signed `ETag`/`If-Match`; move validate destination và cycle trong transaction. |
| `PUT /nodes/{node_id}/favorite` | Bảo đảm `NodeFavorite` cá nhân của caller tồn tại và trả ETag của relation. | Current user có node read; idempotency key desired-state; relation precondition tùy chọn; không bao giờ đổi `Node`. |
| `DELETE /nodes/{node_id}/favorite` | Bảo đảm favorite cá nhân của caller không còn. | Owner của relation; idempotent và không làm lộ trạng thái absent/inaccessible; relation `If-Match` tùy chọn. |
| `POST /nodes/{node_id}:copy` | Copy node hoặc enqueue bounded subtree copy. | Source read cộng destination write; idempotency key và destination precondition. |
| `POST /nodes/{node_id}/trash` | **IMPLEMENTED** — Logical-delete một node không phải root; directory không rỗng bị reject khi subtree precondition còn open. Response thêm timestamp Trash chuẩn và retention status dẫn xuất. | Node owner; CSRF và `If-Match`; FileVersion/Object không bị chạm. |
| `POST /nodes/{node_id}/restore` | **IMPLEMENTED** — Restore một node trashed khi parent cũ còn hợp lệ và active. | Node owner; CSRF và `If-Match`; không đoán recovery location. |
| `DELETE /nodes/{node_id}` | Chỉ request purge khi retention/policy cho phép. | Owner/admin; `If-Match`, idempotency key, irreversible intent rõ ràng. |
| `GET /nodes/{node_id}/content` | **IMPLEMENTED** — Stream current version đã authorize. | Node read đã authenticate; không CSRF; hỗ trợ validator strong và một byte range. |
| `GET /versions/{version_id}/content` | **IMPLEMENTED** — Stream một immutable historical version đã authorize. | Node/version read đã authenticate; không CSRF; hỗ trợ validator strong và một byte range. |
| `POST /libraries/{library_id}/node-operations` | Execute bounded multi-item move/copy/trash/metadata command. | Per-item authorization/precondition, gồm subtree precondition khi đệ quy; idempotency; chỉ synchronous dưới reviewed bound. |

Retention field trên public node DTO chỉ mang tính thông tin: `purge_eligible=true`
nghĩa là internal worker có thể thử `begin_node_purge`, không phải capability
delete-now public. Restore vẫn được phép sau deadline cho tới khi transition
transactional vào `PURGING` thắng. Candidate enumeration và begin-purge là
operation metadata nội bộ, không phải route user thông thường, và không expose
physical metadata purge hay object-byte purge.

Subset metadata hiện tại cố ý giữ rõ các quyết định chưa đóng. PostgreSQL
schema hiện không có `name_key` của sibling hoặc unique constraint, vì vậy raw
logical name trùng nhau dưới cùng parent vẫn hợp lệ. Service không normalize
Unicode, fold case, reject reserved name của host platform hoặc tạo filesystem
path. `POST /nodes/{node_id}/trash` chỉ chuyển state logical của một node:
root được bảo vệ và directory không rỗng trả `invalid_state` cho tới khi quyết
định subtree precondition trong `SYNC.md` OD-SYNC-004 được đóng. Restore chỉ
dùng parent cũ; parent mất, bị xóa hoặc invalid trả conflict ổn định thay vì
đoán recovery location. Không operation nào trong subset này thay đổi
`FileVersion`, `Object`, journal, outbox hay content vật lý.

Folder và file là variant `Node.kind`, không phải identity system không tương
thích. Raw URL `Object` không phải browsing API.

Favorite là preference theo caller, không phải metadata library dùng chung.
Set hoặc clear favorite không cấp access, không giữ content khỏi retention và
không phát content-sync event. Server commit desired state đối nghịch đồng thời
theo transaction order; retry cùng idempotency key trả outcome đã lưu sau khi
mất response. Trash ẩn node khỏi query Favorites mặc định nhưng giữ preference
được authorize để restore; purge hoặc access cleanup xóa relation.

Recent node được sắp theo server-observed update time và immutable ID dưới
watermark `as_of` trong cursor. Read/preview/download không đổi list này. Mỗi
page bỏ node mới mất access hoặc bị trash và tối thiểu hóa context shared-root;
refresh bắt đầu watermark mới và gồm mutation muộn hơn.

### Download, range và object diagnostic

HTTP content transport đã authenticate và implement cho
`GET /nodes/{node_id}/content` (current content) và
`GET /versions/{version_id}/content` (immutable historical content). Hai route
delegate resolve content cùng verified read cho application service trung lập
transport; handler không bao giờ nhận object key, physical path hay direct
storage URL.

- Full response trả `200`; single range hợp lệ trả `206` với
  `Content-Length` chính xác và `Content-Range` inclusive.
- `Range` chỉ nhận đúng một dạng `bytes=start-end`, `bytes=start-` hoặc
  `bytes=-suffix`. Giá trị malformed, duplicate, multi-range và unsatisfiable
  trả `416` cùng `Content-Range: bytes */length` trước khi mở object stream.
  `Accept-Ranges: bytes` chỉ được emit khi backend cấu hình chứng minh
  capability range-read.
- ETag strong được tạo từ SHA-256 bất biến. `If-None-Match` khớp trả `304` từ
  metadata mà không mở byte stream.
- Response success dùng `application/octet-stream`, disposition attachment có
  filename encode theo RFC 5987 và `Cache-Control: private, no-store`. Safe GET
  đã authenticate này không cần CSRF proof.
- Body pull-driven qua content service và Axum stream; handler không aggregate
  toàn bộ file. Multi-range, `HEAD`, `If-Range`, direct object URL, download UI
  và object-integrity diagnostic endpoint vẫn ngoài scope.
- `GET /objects/{object_id}/integrity` là owner/operator diagnostic nếu được giữ
  trong OpenAPI. Endpoint trả safe state/hash/verification evidence, không bao
  giờ trả storage key, backend credential hoặc content URL bỏ qua authorization.
- Content hỏng hoặc quarantined trả stable failure và không bao giờ được stream
  với success status.

### Resumable upload

Subset exact-offset hiện đã implement là:

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `POST /upload-sessions` | **IMPLEMENTED** — Tạo intent tagged strict `CREATE_FILE` hoặc `REPLACE_CONTENT` cùng expected byte và SHA-256 chuẩn tùy chọn. | Principal đã authenticate trở thành owner; CSRF; JSON strict 16 KiB; không nhận `user_id`, object key hay staging handle từ client. |
| `GET /upload-sessions/{upload_session_id}` | **IMPLEMENTED** — Trả state, target, expiry, completion an toàn và offset có thẩm quyền. | Authentication theo owner; không CSRF; sau mọi kết quả PATCH không biết chắc, đây là recovery oracle duy nhất. |
| `PATCH /upload-sessions/{upload_session_id}` | **IMPLEMENTED** — Stream raw chunk không rỗng tại đúng `Upload-Offset`. | Owner + CSRF; chính xác `application/octet-stream`; offset unsigned decimal chuẩn; aggregate limit của service mặc định 8 MiB; success và `invalid_offset` trả offset có thẩm quyền. |
| `POST /upload-sessions/{upload_session_id}/complete` | **IMPLEMENTED** — Delegate verify/promotion/logical commit và trả completion metadata chuẩn. | Owner + CSRF; retry dùng lại outcome exactly-once của service đã validate. |
| `POST /upload-sessions/{upload_session_id}/abort` | **IMPLEMENTED** — Delegate abort mà không trực tiếp thao tác filesystem hay metadata. | Owner + CSRF; repeat an toàn. |

Handler không aggregate toàn bộ body PATCH. Transport frame được chấp nhận sẽ
được progress bền vững qua application service, vì vậy response bị mất hoặc
stream failure có thể để lại prefix đã chấp nhận. Client phải GET status rồi
resume từ `Upload-Offset`/`received_bytes`; không bao giờ blind replay offset cũ
hay tự tăng progress. Browser helper typed gửi trực tiếp `Blob`/`ArrayBuffer` và
không có upload UI product nào được implement.

Inventory part-manifest phong phú hơn sau đây vẫn `PLANNED`; path `/uploads`
không phải alias cho subset `/upload-sessions` đã implement:

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `POST /uploads` | Khởi tạo create/replace intent, expected length/hash, target library/destination, base version và negotiate part. | Library write; idempotency key; reserve quota. |
| `GET /uploads/{upload_id}` | Read state, expiry, constraint, summary hữu hạn của part/missing range, progress và terminal outcome/link relation. | Session owner/device; safe recovery sau unknown response; không embed part list không giới hạn. |
| `GET /uploads/{upload_id}/parts?cursor=...` | Page receipt/range/checksum part đã verify có thẩm quyền để resume. | Session owner/device; opaque keyset cursor bind với session và part generation. |
| `PUT /uploads/{upload_id}/parts/{part_number}` | Stream một declared range cùng length và checksum. | Session owner; chỉ naturally idempotent với fingerprint/byte giống nhau. |
| `POST /uploads/{upload_id}/complete` | Validate coverage, assemble/finalize, verify và expose nguyên tử một version. | Session owner; bắt buộc completion idempotency key. |
| `DELETE /uploads/{upload_id}` | Abort `OPEN`, hoặc request cancellation bền vững cho `VERIFYING`/`COMMITTING` trước commit; release reservation/staging hay finalized orphan byte qua cleanup an toàn. | Session owner; `If-Match`; idempotent; có thể trả `202` khi lease đi tới safe boundary; không thể undo `COMMITTED`. |

Initiation trả part size/count/range limit, expiry, accepted checksum algorithm
và direct-upload capability rõ ràng. Completion không thể tham chiếu byte ở
`OPEN` hoặc mới chỉ uploaded; object durability và verification đứng trước
metadata transaction. Complete state machine thuộc [UPLOADS.md](UPLOADS.md).

### Version và trash

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `GET /nodes/{node_id}/versions` | **IMPLEMENTED** — List safe immutable history metadata newest-first với keyset pagination bounded theo node. | Owner authenticate của active file; không CSRF; private/no-store; resource trashed/purging và cross-owner bị che giấu. |
| `GET /versions/{version_id}` | **IMPLEMENTED** — Read một record safe immutable version metadata đã authorize. | Owner authenticate của active file; không CSRF; private/no-store; ID tương thích historical content route. |
| `POST /nodes/{node_id}/versions/{version_id}/restore` | **IMPLEMENTED** — Append một current `FileVersion` bất biến mới tham chiếu object đã verify của version được chọn; row history và byte cũ không đổi. | Node write của owner; CSRF, signed current-node `If-Match`, idempotency key bounded; stale state trả revision/ETag an toàn. |
| `GET /libraries/{library_id}/trash` | List retained trash entry. | Library read; keyset pagination. |
| `POST /trash/{trash_entry_id}:restore` | Restore với destination/name collision policy rõ ràng. | Library write; idempotency và destination precondition. |
| `DELETE /trash/{trash_entry_id}` | Request permanent logical purge. | Owner; `If-Match`/explicit confirmation, idempotency, audited. |

Restore version tạo version và change event. Restore trash không âm thầm thay
active node đang conflict.

### Change feed và sync

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `GET /libraries/{library_id}/changes?cursor=...` | Trả ordered committed change của library sau khi validate cursor mang cùng library/epoch/profile. | Library/device read scope; cần cursor sau bootstrap; có thể thêm bounded long poll. |
| `POST /libraries/{library_id}/sync-bootstrap` | Thiết lập bounded bootstrap lease và capture primary journal head `H`. | Library/device read scope; idempotency tùy chọn khi request có stable client bootstrap ID. |
| `GET /sync-bootstraps/{bootstrap_id}/nodes` | Page authoritative current projection theo immutable ID order. | Bootstrap owner; opaque page cursor. |
| `POST /sync-bootstraps/{bootstrap_id}/complete` | Chứng minh listing hoàn tất và trả cursor đúng tại `H`. | Bootstrap owner; idempotent terminal outcome. |
| `DELETE /sync-bootstraps/{bootstrap_id}` | Release completed/abandoned lease. | Bootstrap owner; idempotent. |
| `POST /devices/{device_id}/sync-checkpoints` | Acknowledge safely applied cursor/status cho observability và retention policy. | Matching device; monotonic/idempotent; không thể bỏ qua server validation. |

Change được scope vào một `Library` và chỉ ordered bằng sequence của nó. Cursor
không phải authorization token. Client không thể yêu cầu server chấp nhận
arbitrary numeric sequence. [SYNC.md](SYNC.md) sở hữu event kind, conflict rule,
tombstone, compaction và rescan fixture.

### Backup và restore

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `POST/GET /backup-sets` | Định nghĩa device source/exclusion/schedule/retention policy hoặc list authorized set. | Owner và matching device policy; create dùng idempotency; list dùng keyset pagination. |
| `GET/PATCH /backup-sets/{backup_set_id}` | Read/update policy/status. | Owner; `If-Match`. |
| `POST /backup-sets/{backup_set_id}/snapshots` | Bắt đầu snapshot `BUILDING` và negotiate manifest/content submission. | Authorized source device; idempotency key. |
| `GET /backup-snapshots` | List authorized snapshot cùng state, consistency và retention. | Owner; backup-set filter và keyset pagination. |
| `GET /backup-snapshots/{snapshot_id}` | Read một snapshot cùng safe verification/completeness summary. | Owner; không expose object locator. |
| `POST /backup-snapshots/{snapshot_id}/entry-batches` | Submit bounded checksummed manifest batch. | Source device; batch identity và idempotent replay. |
| `POST /backup-snapshots/{snapshot_id}/content-claims` | Bind entry với verified/reusable content theo domain và quota check. | Source device; bounded idempotent claim; không bao giờ chỉ tin client hash. |
| `POST /backup-snapshots/{snapshot_id}/complete` | Seal, verify và nguyên tử làm complete manifest có thể restore. | Source device; idempotency key; một terminal outcome. |
| `GET /backup-snapshots/{snapshot_id}/entries` | Browse immutable manifest. | Owner; path/ID keyset pagination. |
| `POST /restores` | Tạo verified restore operation từ explicit snapshot/version đến explicit destination. | Owner; idempotency, non-destructive destination policy, async operation. |
| `GET /restores/{restore_id}` | Read restore state, verification summary và safe terminal result. | Owner; ETag/freshness. |
| `GET /restores/{restore_id}/entries` | Page per-entry restore result. | Owner; keyset pagination. |
| `POST /restores/{restore_id}/resume` | Resume eligible failed/pending entry mà không duplicate completed work. | Owner; idempotency và restore precondition. |
| `POST /restores/{restore_id}/cancel` | Request best-effort cancellation mà không giả vờ undo committed output. | Owner; conditional/idempotent intent. |

Snapshot `BUILDING` và `FAILED` không bao giờ được mô tả là restorable. Missing
source path không phải live deletion command. [BACKUP.md](BACKUP.md) sở hữu
retention, symlink, snapshot consistency, restore verification và device-loss
recovery.

### Share

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `GET /shares?direction=received` | Populate “được share với tôi” từ private grant active và projection tối thiểu đã authorize của node gốc share. | Authenticated grantee; authorize lại từng result; keyset cursor bind với caller, direction, status và sort. |
| `GET /shares?direction=sent` | List grant do caller tạo/quản trị, với status filter pending/active/expired/revoked tường minh và không có secret. | Grantor/delegation administrator; keyset pagination; check authority từng result; pending link bị mất vẫn quản lý được. |
| `POST /nodes/{node_id}/shares` | Tạo private user grant active, hoặc public-link grant `PENDING` có raw capability được display đúng một lần. | Resource owner có delegation right; idempotency key; public link theo quy tắc one-time-secret và không authorize trước activation. |
| `GET /nodes/{node_id}/shares` | List safe grant không có link/password secret. | Owner/delegation administrator; keyset pagination. |
| `GET/PATCH /shares/{share_id}` | Read/change permission, expiry hoặc allowed setting. | Grant owner/admin; `If-Match`. |
| `POST /shares/{share_id}:activate` | Activate pending public link sau khi owner confirm đã lưu capability. | Grant owner/admin; `If-Match` cộng idempotency key; không trả capability. |
| `POST /shares/{share_id}:revoke` | Revoke future access. | Grant owner/admin; `If-Match` và idempotency; audited. |
| `POST /public-share-sessions` | Exchange capability/password lấy short-lived share-scoped access context. | Public, strict rate/abuse limit; field chứa secret được redact. |
| `GET /public-share-sessions/{session_id}/content` | Chỉ browse/download resolved share scope. | Valid share context; recheck expiry/revocation và range scope. |

Public browser link nên giữ capability entropy cao khỏi proxy query log khi khả
thi, ví dụ dùng URL fragment và post từ static client. Exact transport là quyết
định Phase 3; nó không thể làm yếu revocation hoặc CSRF/XSS control.

Response tạo public link bị mất chỉ để lại share inert `PENDING`. Replay cùng
key trả `one_time_secret_unavailable` và safe share metadata, không bao giờ trả
capability. Không có replacement capability tại chỗ: owner phải revoke pending
share, tạo share mới bằng key mới rồi chỉ activate capability đã biết là được
lưu. Activation race với revoke/expiry theo share revision; một terminal
transition thắng và không pending link nào được
`POST /public-share-sessions` chấp nhận.

Collection received chỉ trả private share hiện còn authorize. Projection node
bắt đầu tại root được share và không được lộ ancestor không đọc được, grantee
khác, storage locator hay material public link. Revocation hoặc expiry có hiệu
lực tại authorization của request, nên page sau có thể bỏ item từng tồn tại khi
cursor trước được cấp; page không bao giờ được trả row giờ đã mất quyền.
Collection sent có thể expose lịch sử revoked an toàn cho grantor, nhưng raw
capability và password material không bao giờ là field của list.

### Photo và album

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `POST /photo-imports` | Bind uploaded immutable resource version vào device import identity/group. | Owner/device; idempotency theo source identity cộng content/version. |
| `GET /photos` | Timeline/filter/search photo projection cùng processing freshness. | Source-authorized user; keyset cursor bind timeline sort và filter. |
| `GET/PATCH /photos/{photo_asset_id}` | Read projection hoặc edit favorite/title/time correction do user sở hữu. | Owner; `If-Match`; không bao giờ rewrite original byte/EXIF. |
| `GET /photos/{photo_asset_id}/derivatives/{kind}` | Stream authorized replaceable rendition. | Source access; fallback/processing state rõ ràng. |
| `POST/GET /albums` | Create/list manual album. | Owner; creation idempotency và keyset list. |
| `PATCH/DELETE /albums/{album_id}` | Chỉ edit/delete album. | Owner; `If-Match`; không cascade-delete asset. |
| `POST/DELETE /albums/{album_id}/members/{photo_asset_id}` | Idempotent add/remove membership. | Owner; conditional album revision. |

Original dùng node/version content API thông thường. Hành vi photo API được xác
định trong [PHOTOS.md](PHOTOS.md).

### Search, tag và AI

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `GET /search` | Query named layer (`METADATA`, `FULL_TEXT`, `SEMANTIC`, `PHOTO`, `CODE`) và trả provenance/freshness. | Authenticated; authorization trước result; query-bound cursor. |
| `POST/GET /tags` | Create/list user tag. | Owner; creation idempotency, keyset listing. |
| `PUT/DELETE /subjects/{subject_type}/{subject_id}/tags/{tag_id}` | Idempotent set/remove user assignment. | Subject/tag owner hoặc allowed editor; không overwrite AI provenance. |
| `GET/PATCH /ai/settings` | Read/change mode, scope, remote consent và derived-retention policy. | Owner/admin theo scope; `If-Match`; audited. |
| `POST /ai/reindex-operations` | Reindex bounded scope/version/config bất đồng bộ. | Authorized source owner; idempotency key. |
| `DELETE /ai/index-records` | Request xóa authorized derived data theo bounded scope. | Owner; idempotency, async operation, audit. |
| `GET /ai/status` | Báo mode, configured pipeline generation, safe health và lag. | Authenticated/admin view tách biệt; không có indexed content. |

`ai_disabled` là state bình thường cho route chỉ AI, không phải core health
failure. Search luôn cho phép deterministic non-AI layer. [AI.md](AI.md) sở hữu
consent, data egress, model provenance, prompt injection và stale-index behavior.

### Git integration, repository và project

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `POST/GET /integrations/git` | Configure/list safe Forgejo connection. | Owner; create idempotency; credential write-only và redacted. |
| `GET/PATCH/DELETE /integrations/git/{integration_id}` | Read status, pause/reconfigure hoặc revoke connection. | Owner; `If-Match`; deletion không bao giờ xóa Forgejo data. |
| `POST /integrations/git/{integration_id}:test` | Thực hiện bounded SSRF-safe capability/credential test. | Owner; rate-limited, audited; safe provider error mapping. |
| `POST /integrations/git/{integration_id}:refresh` | Enqueue inventory refresh. | Owner; idempotency; async. |
| `GET /repositories` | List accessible cached repository cùng freshness. | Owner/current provider authorization policy; keyset pagination. |
| `GET /repositories/{repository_id}` | Read metadata, branch/tag summary, backup health. | Authorized owner; stale state rõ ràng. |
| `POST /repositories/{repository_id}/backups` | Enqueue repository recovery point. | Owner; idempotency; async. |
| `GET /repositories/{repository_id}/backups` | List verified/incomplete backup record. | Owner; keyset pagination. |
| `POST /repository-backups/{backup_id}:restore` | Restore đến explicit safe Forgejo destination. | Owner, fresh provider authorization, step-up/confirmation, idempotency. |
| `POST/GET /projects` | Create/list optional workspace. | Authenticated owner; idempotency/keyset rule. |
| `GET/PATCH/DELETE /projects/{project_id}` | Chỉ read/edit/delete project metadata. | Owner; `If-Match`; link/target không cascade. |
| `PUT/DELETE /projects/{project_id}/links/{link_id}` | Add/remove authorized typed link. | Phải có thể access cả project và target; idempotent, conditional. |

Forgejo vẫn là authority cho Git protocol và permission.
[CODE_INTEGRATION.md](CODE_INTEGRATION.md) sở hữu provider identity, backup
consistency, restore verification, webhook validation và stale-state rule.

### Activity, administration và health

| Method và path | Trách nhiệm | Access và retry |
|---|---|---|
| `GET /activity` | List product activity projection caller được authorize. | Authenticated; keyset pagination; redacted. |
| `GET /admin/audit-events` | Query append-only security event. | Instance admin/auditor role; bounded filter; không secret/content. |
| `GET /admin/storage/backends` | Read configured backend status và capacity evidence. | Instance administrator; bỏ secret. |
| `POST /admin/storage/verification-operations` | Enqueue bounded integrity verification. | Instance administrator; idempotency; async/audited. |
| `GET /system/health` | Aggregated authenticated operational health và freshness. | Administrator/monitor credential; safe detail. |
| `GET /health/live` | Probe absolute không version chỉ cho process liveness. | Deployment probe; không leak dependency/topology/version. |
| `GET /health/ready` | Probe absolute không version cho cached readiness của required configuration, DB và selected object backend. | Deployment probe; result tối thiểu. |
| `GET /operations/{operation_id}` | Read authorized async state/result. | Operation owner/admin; ETag và optional wait. |
| `POST /operations/{operation_id}:cancel` | Request safe cancellation. | Operation owner/admin; `If-Match`; idempotent intent. |

Public health endpoint không công bố database hostname, mount path, bucket name,
free-space total, provider URL, worker stack trace hay secret configuration.

## Bản đồ transaction và side effect

| API outcome | PostgreSQL atomic boundary | Object/external boundary | Recovery |
|---|---|---|---|
| Upload complete | session terminal outcome + identity `Object` chuẩn + binding backend/key/representation của `ObjectReplica` đã verify + version + node head + quota + change + audit + outbox | Byte của object được finalize và verify trước transaction | Unreferenced finalized byte được reconcile sau lease/grace; replay trả stored outcome. |
| Rename/move | node revision/ancestry/name + change + audit + outbox + idempotency | Không synchronous | Rollback không expose thay đổi; stale precondition conflict. |
| Trash/restore | node/trash state + change + audit + outbox + outcome | Physical deletion không bao giờ synchronous | GC về sau chứng minh mọi retention reference vắng mặt. |
| Share create/revoke | grant/revision + audit + outbox + outcome | Active stream/byte đã giao nằm ngoài rollback | Mọi request mới reauthorize; công bố giới hạn recall. |
| Backup snapshot commit | verified manifest + reference + accounting + state + audit + outbox | Mọi required object đã verify | Incomplete snapshot vẫn non-restorable; retry cùng key. |
| Photo import binding | asset/resource + revision + change nếu node mutation + outbox | Extraction/derivative generation bất đồng bộ | Processing có thể retry hoặc giữ failed; original vẫn truy cập được. |
| AI settings change | policy revision + audit + scoped invalidation/outbox | Cancellation remote queued/in-flight là best effort | Block egress mới ngay lập tức; expose deletion/drain state. |
| Repository backup commit | verified component manifest + ref + consistency class + audit/outbox | Forgejo capture đã xảy ra trước; optional service | Incomplete capture được label; orphan byte reconcile an toàn. |

PostgreSQL failure nghĩa là API không thể tuyên bố metadata mutation thành công.
Object-store failure nghĩa là content version mới không thể commit. External
provider tùy chọn failure chỉ ảnh hưởng operation và freshness của nó.

## Header caching và consistency

- Mutable metadata dùng `Cache-Control: private, no-cache` để client có thể
  revalidate bằng ETag mà không phục vụ cho user khác.
- Response chứa secret, authentication, share-resolution, recovery và sensitive
  setting dùng `Cache-Control: no-store`.
- Authorized immutable version content có thể privately cache theo immutable
  ETag; public caching cần share-specific policy riêng và không thể sống lâu hơn
  revocation promise.
- `Vary` gồm mọi header đổi hành vi representation/authorization. Bearer secret
  không bao giờ xuất hiện trong cache key nhìn thấy trong log.
- API không dùng database read bị replica lag cho immediate authorization,
  conditional mutation, upload completion hoặc change cursor.

## API version và compatibility policy

- `v1` định danh public protocol major version, không phải product release.
- Backward-compatible addition gồm optional response field, endpoint mới và
  opt-in enum value mới chỉ khi old client có unknown-value behavior xác định.
- Breaking change gồm đổi ý nghĩa/type field, làm yếu invariant, xóa state/code,
  đổi cursor interpretation hoặc đổi authorization/idempotency semantic. Chúng
  cần ADR và route/media version tương thích mới hoặc migration strategy.
- Deprecation được ghi trong OpenAPI và response metadata cùng minimum support
  window được công bố, gắn với release policy. Self-hosted upgrade không bao giờ
  giả định mọi client update đồng thời.
- Reader deploy trước writer khi stored/event representation mới có thể được
  mixed version quan sát.
- Client gửi protocol/capability declaration lúc device registration; mandatory
  capability không được hỗ trợ trả stable upgrade-required response thay vì
  xấp xỉ destructive behavior.

## OpenAPI blueprint và review gate

[`api/openapi.yaml`](../../api/openapi.yaml) là reviewed transport authority
phía dưới domain/protocol specification. File chứa health,
browser-authentication, bootstrap và logical file/folder metadata route đã
validate, convention dùng chung, safe resource DTO, bounded pagination,
conditional mutation response và schema tái sử dụng. Content, sync, backup,
sharing và device operation vẫn `PLANNED` cho tới khi domain contract tương
ứng sẵn sàng. File tiếp tục nên chứa:

```text
info and server/profile metadata
tags by domain
securitySchemes
paths and operationId values
components/
  schemas/
    Resource envelopes
    Error and registered error details
    IDs, timestamps, revisions, hashes
    Domain resource representations
    Operation, pagination, freshness
  parameters/
    Cursor, limit, If-Match, Idempotency-Key
  headers/
    ETag, X-Request-Id, Retry-After
  responses/
    Standard errors and conditional outcomes
  securitySchemes/
    BrowserSession, DeviceBearer, PublicShareContext
```

Mỗi operation khai báo:

- status (`PLANNED` đến khi evidence nâng cấp), owner, summary và stable
  `operationId`;
- exact auth scheme và authorization action;
- yêu cầu idempotency và conditional-request;
- request/response limit và streaming behavior;
- mọi success/error code gồm safe detail;
- semantic pagination/filter/sort/freshness;
- hiệu ứng audit và async operation/outbox; và
- example cho success, stale precondition, forbidden/not-found concealment,
  duplicate replay và dependency failure khi áp dụng.

Generated client chỉ đến từ reviewed contract và không bao giờ được hand-edit.
Contract test validate example, error registry, status code, cursor round-trip,
conditional behavior và unknown-field/enum compatibility. Implementation phải
fail CI nếu reachable operation lệch khỏi OpenAPI.

Blueprint này không claim speculative endpoint cho installer, service-manager,
storage-picker, pairing, update, uninstall, migration hay relay. Khi một nhóm
trở nên đủ điều kiện implementation, request/response schema, error registry,
authorization action, local-IPC boundary (nếu có), idempotency behavior và
recovery test của nó phải được review ở đây và trong contract tiếng Anh tương
ứng trước khi promote operation.

## Yêu cầu abuse và privacy

- Enforce giới hạn body, decompressed-body, header, filename, part, manifest,
  batch, page, recursion, search-query và operation-result trước allocation hoặc
  parsing tốn kém.
- Stream và backpressure upload/download. Checksum, compression, archive, image,
  document hoặc Git parsing nặng CPU chạy với bounded concurrency và giới hạn
  time/memory/disk bên ngoài async reactor.
- Không bao giờ fetch user/provider URL từ general file endpoint hoặc AI
  endpoint. Integration URL fetching phải qua SSRF validation, redirect
  revalidation, DNS rebinding control, egress policy và private-address policy.
- Sanitize `Content-Disposition` và mọi name được UI render; API JSON escaping
  không phải XSS defense duy nhất.
- Không reveal cross-user dedup hit, object existence, filename, search snippet,
  embedding, photo location, repository visibility, quota total hay timing
  difference.
- Public link và webhook receiver có strict rate limit, signature/secret
  handling, replay window, size bound và redacted log riêng.
- Remote AI request là bất khả thi trừ khi current policy và explicit consent
  authorize đúng provider và data category; API expose dữ liệu đang queued,
  derived và có thể delete.

## Yêu cầu failure và recovery

| Scenario | Hành vi API |
|---|---|
| Client disconnect giữa part | Part chỉ được chấp nhận nếu toàn bộ declared range và checksum đã verify; nếu không state vẫn retryable/rejected và không visible version. |
| Disk đầy sau một số staging write | Terminate an toàn, giữ committed object, chỉ release/retain bounded session state, trả storage/quota error và observability signal. |
| Completion chạy đua với expiry/abort | Row transition serialize một terminal winner; losing request nhận stored state, không bao giờ outcome thứ hai. |
| DB commit thành công nhưng response bị mất | Cùng idempotency key hoặc `GET /uploads/{upload_id}` trả exact committed ID/ETag; không duplicate change. |
| Cursor token bị copy sang library/user khác | Integrity/scope validation fail trước query; trả concealed `invalid_cursor` mà không leak original scope. |
| Recursive operation thành công một phần trước worker crash | Durable per-item checkpoint và idempotency resume; terminal result là `PARTIAL` đến khi reconcile. |
| Share bị revoke trong cached/public access | Resolution/range request mới fail; response không tuyên bố recall byte đã giao hoặc bị client cache sai. |
| AI/thumbnail worker offline | Core commit response vẫn thành công với derived state `PENDING`/`STALE`; core health vẫn healthy và worker lag được hiển thị riêng. |
| Forgejo trả 401/500 hoặc malicious body | Ánh xạ sang safe integration status/error, giới hạn body parsing, không bao giờ trả raw secret/provider trace và giữ Drive dùng được. |
| Object fail checksum khi read | Không trả `200` cùng corrupt byte; quarantine/abort stream khi phát hiện, ghi integrity incident, tìm verified replica/restore path. |

## Open decision hữu hạn

OPEN DECISION OD-API-001: retention của idempotency outcome
Owner: Architecture / API / Database / Sync
Needed by: Gate mutating OpenAPI đầu tiên Phase 1
Options: retention cố định bảy ngày; operation-specific retention ít nhất bằng upload/session expiry cộng retry window; giữ compact outcome suốt resource lifetime
Recommendation: operation-specific retention với documented minimum bằng offline retry window dài nhất được hỗ trợ, cùng compact long-lived tombstone cho key mà replay có thể duplicate durable user data
Decision evidence: yêu cầu client retry/offline, benchmark database growth, purge safety proof và replay contract test

OPEN DECISION OD-API-002: giới hạn collection page
Owner: API / Web / Client / Performance
Needed by: Gate listing OpenAPI Phase 1
Options: một global default/maximum; operation-specific bound; server-advertised adaptive maximum
Recommendation: operation-specific fixed default và maximum được ghi trong OpenAPI, cùng measurement Phase 1 bảo thủ và không có adaptive behavior khiến client memory khó đoán
Decision evidence: measurement metadata-row size, web/mobile memory test, database plan và abuse-load test

OPEN DECISION OD-API-003: representation sync baseline
Owner: Sync / API / Database
Needed by: Protocol freeze Phase 4
Options: bounded live ID-order scan cộng retained resume cursor; materialized manifest; exported database snapshot giữ qua nhiều request
Recommendation: bounded live ID-order scan cộng retained sequence-N resume cursor và revision-idempotent replay; chỉ materialize nếu adversarial churn test không thể hội tụ trong lease bound
Decision evidence: property test dưới create/move/delete churn, journal-retention load test, stale-baseline recovery test và mobile interruption test

OPEN DECISION OD-API-004: transport của public share capability
Owner: Security / API / Web
Needed by: Gate public sharing Phase 3
Options: URL path token; URL fragment được post vào scoped session; one-time code exchange
Recommendation: URL fragment cộng explicit POST exchange vào short-lived share-scoped context để common proxy log không nhận capability entropy cao
Decision evidence: browser/XSS/CSRF threat review, quyết định product accessibility và no-JavaScript, proxy-log test và revocation test

OPEN DECISION OD-API-005: threshold cho large collection command
Owner: API / Worker / Performance
Needed by: Gate bulk và recursive operation Phase 2
Options: luôn asynchronous; synchronous dưới fixed item/work estimate; client-selected preference bị server giới hạn
Recommendation: dùng work bound operation-specific cố định trên server và trả `202` phía trên bound; không bao giờ quyết định chỉ từ item count khi subtree expansion hoặc byte work chưa biết
Decision evidence: transaction-lock duration benchmark, worker recovery test, API timeout limit và partial-result UX review

## Boundary client/HTTP Prompt 36

Prompt 36 không thêm hay thay đổi public HTTP route hoặc OpenAPI schema. Core
inbound desktop nhận contract bootstrap, feed, acknowledgement, completion và
logical content có kiểu qua trait `SyncRemote`. Nhờ vậy server transport,
authentication, redaction token, ánh xạ HTTP error, backoff và connection
lifecycle không nằm trong code recovery filesystem/SQLite.

Prompt 37 implement adapter đó mà không đổi apply-before-acknowledge, durable
evidence hay filesystem recovery của engine. Adapter giữ nguyên byte evidence
server, bound JSON trong lúc nhận, stream content theo length/hash budget strict
và trả typed error đã sanitize.

## Boundary Device enrollment và HTTP inbound đã implement ở Prompt 37

Đây là contract đã implement. Các family challenge, discovery, refresh-token,
guided pairing và remote relay rộng hơn ở phần blueprint khác chưa được phase
này implement.

Mọi path dưới đây có prefix `/api/v1`. Body là JSON strict tối đa 2 KiB, không
nhận field lạ. Enrollment thành công và mọi auth error đều `private, no-store`.

| Method và path | Principal và request | Kết quả |
|---|---|---|
| `POST /devices/enrollment-grants` | Browser + CSRF; `target` là `{"kind":"new","display_name":"Desktop"}` hoặc `{"kind":"existing","device_id":"<canonical UUIDv7>"}` | `201`: owner/Device/grant ID, token một lần, thời điểm tạo và hết hạn |
| `POST /device-enrollment/exchange` | Không browser session; một `enrollment_token` | `201`: owner/Device/credential ID, một bearer secret, thời điểm tạo |
| `POST /devices/{device_id}/credentials/{credential_id}/revoke` | Browser + CSRF; `{}` | `204`: revoke credential đúng owner |
| `POST /devices/{device_id}/credentials/revoke-all` | Browser + CSRF; `{}` | `204`: revoke mọi credential và grant còn mở của Device |

Credential thuộc `Device` canonical hiện có, không có device identity song
song. Enrollment mới tạo Device `PENDING`, rồi activate trong cùng transaction
consume grant và insert credential. Grant sống mặc định mười phút, entropy
cao, chỉ lưu digest, single-use bằng row lock PostgreSQL và không thể mint hai
credential khi exchange đồng thời. Response exchange thành công bị mất không
được retry/replay: browser revoke credential của Device, tạo grant mới rồi
re-enroll explicit. Grant hết hạn/đã dùng/không tồn tại cùng trả
`invalid_enrollment` an toàn. API không phục hồi secret từ digest. Mọi request,
kể cả metadata/download, đều kiểm tra revocation.

`BrowserSession { owner_user_id, session_id }` và
`DeviceCredential { owner_user_id, device_id, credential_id }` là hai principal
khác nhau; bearer không tạo SessionId giả. Có Authorization header thì bắt buộc
parse bearer; malformed, duplicate, invalid hoặc Cookie/Authorization trộn lẫn
không fallback sang cookie. Chỉ Device principal đã verify thành công được
miễn CSRF ở ba POST inbound.

Device bearer chỉ được vào sáu operation checkpoint/feed/rebaseline cộng
`GET /nodes/{node_id}`, `GET /versions/{version_id}` và hai route `/content`.
Bearer bị từ chối ở session administration, tạo/revoke enrollment grant,
library browsing, upload, mutation submission, conflict management, version
restore và route tương lai chưa review. Content vẫn đi qua owner/file/version
service hiện có: che giấu sai owner, unknown, trashed hoặc purging; ObjectStore
key và physical path không vào URL desktop.

`HttpSyncRemote` implement đủ bảy method `SyncRemote`. Event incremental có
`schema_version: 1`; Node response thêm `current_version_id`. Current Node và
immutable version metadata resolve projection feed. Revision đã đổi sẽ yêu
cầu rebaseline, không bịa tên lịch sử. Revision zero hợp lệ cho Node mới;
epoch/generation vẫn dương và mọi decimal/UUID vẫn phải canonical.

Profile chỉ nhận HTTPS origin đã verify, normalize host/default port bằng
parser `url`, reject userinfo, query/fragment, non-root/dot-segment path, port
lỗi, control, whitespace và URL repair. Constructor test numeric-loopback
explicit là ngoại lệ HTTP duy nhất. Chưa có stable server-installation ID:
binding dựa trên canonical origin đã verify, không suy ID từ hostname.
Envelope trong secure store còn bind origin cùng profile/owner/Device/
credential ID; copy SQLite metadata không thể gán secret cũ sang origin khác.
Public enrollment persistence chỉ nhận receipt bind với HTTP exchange profile.

Client reqwest/rustls verify certificate bình thường, minimum TLS 1.2, không
follow redirect, không proxy/cookie jar/referer/transparent compression và
không auto-retry. Sensitive Authorization header gắn riêng từng request cùng
origin. Mặc định: connect 10 giây, headers 20 giây, metadata 30 giây, stream idle
30 giây, tổng download một giờ. Duration cấu hình phải dương và không quá 24
giờ. JSON tối đa 8 MiB, error 64 KiB, manifest page 1.000 Node, feed page 500
event, chunk content phát ra 1 MiB. Download dùng immutable version ID chính
xác, bound expected length tối đa 1 TiB và verify SHA-256 incremental. Sai
redirect, encoding, scope, schema, ID, sequence, bootstrap generation,
content-type hoặc evidence đều fail closed.

401 phân biệt `AuthRequired` với `DeviceRevoked` chỉ khi đã chứng minh secret;
403/404 giữ forbidden/not-found, 409/410 giữ outcome checkpoint/rebaseline/
evidence, 429 là rate-limited, 5xx thành dependency/internal error an toàn.
TLS, timeout, body-limit, redirect, JSON lỗi và response bất thường có category
transport đã sanitize riêng. Health trả `ONLINE`, `AUTH_REQUIRED`,
`DEVICE_REVOKED`, `SERVER_UNAVAILABLE`, `TLS_ERROR` hoặc `PROTOCOL_ERROR`,
không lộ response message server, URL, credential hay TLS implementation text.

Trace ghi route template và request ID, không ghi URI/query thô, body,
Authorization, cookie hay enrollment/ack/completion token. Hint request ID từ
client chứa prefix machine-secret dành riêng (`svd1_` hoặc `sve1_`) được thay
bằng ID server mới trước khi trace hoặc ghi response header. Proxy deployment
cũng phải tắt sensitive header/body/query logging. Auth/offline failure giữ
nguyên managed file, bootstrap state, applied/acknowledged sequence và pending
evidence. Local forget không phải revoke server; caller phải drop direct
transport còn giữ, engine kiểm tra credential identity đã persist trước mỗi
lần sync.

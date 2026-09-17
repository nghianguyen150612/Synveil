# ADR-037: Production desktop process bootstrap, platform lifecycle, and root availability

Status / Trạng thái: **LOCKED**

Date / Ngày: 2026-09-13

Decision owners / Chủ sở hữu quyết định: Architecture, Client Sync, Platform, Security

## Context / Bối cảnh

ADR-036 established `DesktopSyncHost` as the single application composition
root for the bounded Prompt 91–94 client stack, but the repository still
needed a production foreground executable boundary. A desktop process must
acquire non-secret configuration, reopen existing profile-bound local state,
install native process hooks, and terminate gracefully without introducing a
second synchronization composition root or another Tokio runtime.

The physical library root is also an availability boundary rather than an
ordinary missing-directory case. A removable volume, unmounted share, or
temporarily unavailable path must not be interpreted as a user deletion. The
process must preserve each library's durable binding and keep the observer and
sync cycle fenced until the same approved root reappears.

The existing contracts are authoritative: server migrations remain at 36,
client SQLite schema remains V6, `LocalStateStore` already holds the adjacent
`fs2` writer lock, profile/device credentials remain in the existing secure
`SecretStore`, and the server has no setup-secret or distributed rate-limiter
contract for this boundary.

## Decision / Quyết định

1. The `synveil-client` package is the production foreground process. Its
   binary `main` is intentionally thin: initialize bounded logging, select
   the current platform adapter, load the non-secret `client.conf` manifest,
   create exactly one Tokio runtime, and call `DesktopClientProcess`. All
   synchronization composition remains in the one `DesktopSyncHost` owned by
   `synveil-client-sync`; the process crate does not construct a second
   runtime, engine, scheduler, watcher, or transport graph.
2. The process manifest contains only one canonical UUIDv7 `profile_id` and
   explicit canonical UUIDv7 `library.<library-id>=<absolute-root>` entries.
   The existing SQLite profile/replica rows and the platform `SecretStore`
   supply the server origin, owner/device scope, root binding, enrollment
   metadata, and credential. Configuration/debug/status formatting does not
   echo root paths, credentials, cookies, tokens, or file content. A manifest
   can reopen a previously bound root, but cannot create a root, replace a
   binding, or select a different account/device.
3. `DesktopClientProcess` owns exactly one `DesktopSyncHost`, one process
   lifecycle source, and one network-hint source. Linux maps `SIGINT` and
   `SIGTERM` to the shared `ShutdownRequested` event; Windows maps Ctrl-C to
   the same event. Shutdown first stops the process adapters, then follows
   the host's one graceful shutdown path, allows active bounded cycles to
   finish, joins owned tasks, and is idempotent. There is no forced exit,
   daemon loop, service-manager mutation, tray, GUI, autostart, installer, or
   service-unit behavior in this decision.
4. Network integration is a best-effort hint adapter only. Linux may inspect
   bounded non-secret interface state and Windows may inspect a bounded local
   route hint. A changed positive hint calls the existing host
   `network_available()` surface; it never authenticates, writes a checkpoint,
   calls a sync engine directly, or bypasses conflict/recovery gates. If the
   native adapter cannot initialize or fails, a bounded periodic hint source
   is substituted. There is one monitor per process and it is stopped during
   graceful shutdown.
5. Root availability is process-ephemeral and tracked independently for each
   registered library as `Available`, `Unavailable`, or the short-lived
   `Recovering` state. It is not a PostgreSQL row, SQLite column, migration,
   change-journal fact, runtime event payload, or user deletion signal.
6. If a configured root is missing or inaccessible during bootstrap, the
   process opens a deferred replica using the existing durable binding ID. It
   does not create the directory, marker, control directories, replacement
   binding, or delete/trash intents. The host remains healthy enough to serve
   the other libraries, while that library is `Unavailable` and its sync
   runtime is `RootBlocked`. Manual/network wakes remain safe no-ops/retries
   behind the same root gate.
7. The observer is fenced before draining queued hints and before durable
   observation intent creation. Root loss stops observation delivery and
   records only the existing bounded observation issue. A reappearance is
   accepted only when the canonical path, managed-root marker, profile, scope,
   and durable binding match. The existing watcher is restarted once, one
   bounded canonical rescan reconciles changes made while the root was away,
   and only then does the library return to `Available` and receive one
   `RootAvailable` runtime wake. A failure in one library does not stop a
   healthy sibling library.
8. The existing adjacent `state.sqlite3.writer.lock` remains the process
   boundary for a shared local state database. The production process adds no
   redundant PID lock or second state store: a second opener receives the
   existing typed `ConcurrentWriter`/`LOCAL_REPLICA_ALREADY_OPEN` result.
   Process and host status are ephemeral and are never used as durability
   evidence.
9. This phase adds no server migration, client migration, route, OpenAPI
   operation, frontend state, service/autostart/deployment artifact, setup
   secret, distributed limiter, upload/share/backup feature, MFA/OAuth/OIDC,
   device-credential redesign, or GUI/tray surface. Existing deployment and
   scheduled-maintenance artifacts remain separate contracts.

## Consequences / Hệ quả

- A real Linux/Windows foreground process now has one auditable owner for
  configuration, signal handling, network hints, host startup, and shutdown.
- A lost removable root cannot generate a mass delete or replacement binding;
  its durable local work waits behind a per-library availability fence and is
  recoverable after an identity-checked reappearance.
- Missing-root libraries do not prevent healthy siblings from progressing, but
  root state is intentionally transient and must be rediscovered after every
  process restart.
- Native network and signal APIs stay at the platform edge. Their failures are
  nonfatal where the bounded fallback is safe, and the synchronization core
  remains transport- and OS-policy-neutral.
- The live PostgreSQL process target can verify the production bootstrap over
  the real authenticated HTTP path, while deterministic Linux root tests and
  cross-target compilation cover the portability/safety boundary. A complete
  readiness claim still requires the configured live PostgreSQL 17 and
  Windows gates to pass.

## Alternatives rejected / Phương án loại bỏ

- A second Tokio runtime or a process-local scheduler was rejected because it
  would split ownership from ADR-036 and make duplicate cycles possible.
- Treating a missing root as an empty directory was rejected because it could
  turn an unmount or outage into deletion/trash observations.
- Auto-creating a configured root or silently rebinding a reappeared path was
  rejected because a path string is not proof of the same user-approved root.
- A persistent root-availability table or durable process-status row was
  rejected because availability is environmental and can become stale across
  restart; existing replica/profile bindings remain the durable identity.
- A setup secret, distributed rate limiter, OS network daemon, system service,
  autostart, tray, or installer was rejected as an unapproved new contract and
  product scope.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No migration is required. Review this ADR before adding a service manager,
autostart/installer, tray/UI, multiple account contexts, a setup-secret
contract, a distributed rate limiter, dynamic credential transport, or a
different root-rebind workflow. Any future process boundary must preserve one
`DesktopSyncHost`/runtime per supported account context, durable-before-wake
ordering, root-loss fencing, same-binding reappearance validation, bounded
rescan, graceful signal equivalence, and non-sensitive status surfaces.

---

## Bản tiếng Việt

### Bối cảnh

ADR-036 xác lập `DesktopSyncHost` là composition root duy nhất cho stack
client bounded Prompt 91–94, nhưng repository vẫn thiếu boundary của
executable foreground production. Một process desktop phải lấy configuration
không bí mật, mở lại local state đã bind theo profile, gắn process hook native
và kết thúc graceful mà không tạo composition root đồng bộ thứ hai hoặc một
Tokio runtime khác.

Root vật lý của Library cũng là boundary availability, không phải trường hợp
thư mục thiếu thông thường. Volume tháo ra, share chưa mount hoặc path tạm
thời không truy cập được không được diễn giải thành user deletion. Process
phải giữ durable binding của từng Library và fence observer cùng sync cycle
cho đến khi đúng root đã được phê duyệt xuất hiện lại.

Các contract hiện có là authoritative: server giữ 36 migration, SQLite client
giữ schema V6, `LocalStateStore` đã có writer lock `fs2` liền kề, credential
profile/device vẫn ở `SecretStore` bảo mật hiện có, và server không có contract
setup-secret hay distributed rate limiter cho boundary này.

### Quyết định

1. Package `synveil-client` là foreground process production. Binary `main`
   chỉ làm logging bounded, chọn platform adapter hiện tại, đọc manifest
   `client.conf` không bí mật, tạo đúng một Tokio runtime và gọi
   `DesktopClientProcess`. Toàn bộ sync composition vẫn nằm trong một
   `DesktopSyncHost` của `synveil-client-sync`; crate process không tạo runtime,
   engine, scheduler, watcher hay transport graph thứ hai.
2. Manifest chỉ có một `profile_id` UUIDv7 canonical và các entry
   `library.<library-id>=<absolute-root>` UUIDv7 canonical. SQLite profile/
   replica hiện có cùng `SecretStore` platform cung cấp origin server,
   owner/device scope, root binding, enrollment metadata và credential. Debug/
   status/configuration không echo root path, credential, cookie, token hay
   file content. Manifest có thể mở lại root đã bind nhưng không tạo root,
   thay binding hoặc chọn account/device khác.
3. `DesktopClientProcess` sở hữu đúng một `DesktopSyncHost`, một lifecycle
   source và một network-hint source. Linux map `SIGINT` và `SIGTERM` thành
   event chung `ShutdownRequested`; Windows map Ctrl-C thành cùng event đó.
   Shutdown dừng adapter process trước, đi qua một graceful shutdown path của
   host, cho bounded cycle đang chạy hoàn tất, join task và idempotent. Không
   có forced exit, daemon loop, mutation service manager, tray, GUI, autostart,
   installer hay service-unit trong quyết định này.
4. Network integration chỉ là best-effort hint adapter. Linux có thể kiểm tra
   interface state bounded không bí mật và Windows có thể kiểm tra local route
   hint bounded. Positive hint thay đổi chỉ gọi `network_available()` hiện có;
   không authenticate, ghi checkpoint, gọi engine trực tiếp hay bypass gate
   conflict/recovery. Nếu native adapter không init hoặc lỗi, dùng periodic
   hint bounded fallback. Mỗi process chỉ có một monitor và monitor dừng khi
   graceful shutdown.
5. Root availability là state ephemeral của process, theo từng Library:
   `Available`, `Unavailable` hoặc `Recovering` ngắn hạn. State này không phải
   PostgreSQL row, SQLite column, migration, change-journal fact, runtime event
   payload hay tín hiệu user deletion.
6. Nếu root configured thiếu hoặc inaccessible lúc bootstrap, process mở
   deferred replica bằng durable binding ID hiện có. Nó không tạo directory,
   marker, control directory, replacement binding hoặc delete/trash intent.
   Host vẫn đủ healthy để sibling Library tiếp tục; Library đó là
   `Unavailable`, sync runtime là `RootBlocked`. Manual/network wake vẫn an
   toàn vì đi sau cùng root gate.
7. Observer bị fence trước khi drain hint đã queue và trước khi tạo durable
   observation intent. Root loss dừng observation delivery và chỉ ghi
   observation issue bounded hiện có. Reappearance chỉ được chấp nhận khi
   canonical path, managed-root marker, profile, scope và durable binding
   cùng khớp. Watcher hiện có được restart một lần, một canonical rescan
   bounded reconcile thay đổi khi root vắng, rồi Library mới trở lại
   `Available` và nhận một runtime wake `RootAvailable`. Lỗi một Library không
   dừng sibling healthy.
8. `state.sqlite3.writer.lock` liền kề hiện có tiếp tục là boundary process cho
   cùng local state database. Production process không thêm PID lock thừa hay
   state store thứ hai: opener thứ hai nhận result typed
   `ConcurrentWriter`/`LOCAL_REPLICA_ALREADY_OPEN` hiện có. Process/host status
   là ephemeral và không được dùng làm durability evidence.
9. Phase này không thêm server/client migration, route, OpenAPI operation,
   frontend state, service/autostart/deployment artifact, setup secret,
   distributed limiter, upload/share/backup feature, MFA/OAuth/OIDC,
   redesign device credential hay GUI/tray. Deployment và scheduled
   maintenance artifact hiện có vẫn là contract tách biệt.

### Hệ quả

- Foreground process Linux/Windows thật có một owner có thể audit cho config,
  signal, network hint, host startup và shutdown.
- Root removable bị mất không thể tạo mass delete hoặc replacement binding;
  durable work chờ sau availability fence theo Library và recover sau khi
  reappearance đã kiểm tra identity.
- Library mất root không chặn sibling healthy, nhưng root state có chủ ý là
  transient và phải rediscover sau mỗi process restart.
- Native signal/network API nằm ở edge platform. Lỗi của chúng nonfatal khi
  fallback bounded an toàn; sync core vẫn trung lập với transport và OS policy.
- Live PostgreSQL process target có thể verify bootstrap production qua HTTP đã
  authenticate thật; Linux root tests deterministic và cross-target compile
  cover portability/safety. Readiness hoàn chỉnh vẫn cần live PostgreSQL 17 và
  Windows gate đã cấu hình chạy pass.

### Phương án loại bỏ

- Tạo Tokio runtime thứ hai hoặc scheduler process-local thứ hai bị loại vì
  chia ownership khỏi ADR-036 và cho phép duplicate cycle.
- Xem root thiếu như directory rỗng bị loại vì unmount/outage có thể thành
  deletion/trash observation.
- Tự tạo root configured hoặc âm thầm rebind path xuất hiện lại bị loại vì
  path string không chứng minh đúng root user đã approve.
- Tạo root-availability table hoặc durable process-status row bị loại vì
  availability là environmental và có thể stale sau restart; replica/profile
  binding hiện có mới là identity durable.
- Setup secret, distributed rate limiter, OS network daemon, service,
  autostart, tray hoặc installer bị loại vì là contract mới chưa được duyệt và
  vượt product scope.

### Điều kiện di chuyển và xem xét lại

Không cần migration. Phải review ADR này trước khi thêm service manager,
autostart/installer, tray/UI, nhiều account context, setup-secret contract,
distributed rate limiter, dynamic credential transport hoặc workflow rebind
root khác. Mọi process boundary tương lai phải giữ một
`DesktopSyncHost`/runtime cho mỗi account context được hỗ trợ, thứ tự
durable-before-wake, root-loss fencing, kiểm tra same-binding khi reappear,
rescan bounded, signal semantics tương đương và status không nhạy cảm.

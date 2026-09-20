# ADR-042: Secure desktop authentication and credential lifecycle / Xác thực desktop an toàn và lifecycle credential

Status / Trạng thái

Accepted — LOCKED

Date / Ngày

2026-09-18

Decision owners / Chủ sở hữu quyết định

Synveil maintainers / Nhóm maintainers Synveil

## Context / Bối cảnh

The production desktop has two processes. `synveil-desktop` is a Qt/QML
presentation shell and `synveil-client` is the process-owned synchronization
runtime. Prompt 96 already defines their local control boundary and Prompt 97
already defines the reconnecting, generation-fenced controller. The remaining
authentication operation must fit those boundaries without moving credentials
into the GUI or creating a second authentication model.

Desktop authentication is an existing one-time device-enrollment grant
exchange. It is not OAuth, a password login, an API-key import, a session
cookie, or a new server authentication scheme. The canonical exchange is the
existing `POST /api/v1/device-enrollment/exchange` handled by
`HttpEnrollmentClient`. The returned `EnrollmentCredentials` receipt is
promoted by the existing profile-bound `LocalStateStore` and `SecretStore`
lifecycle.

The boundary must also remain safe when a user clicks repeatedly, a GUI loses
its IPC response, the client restarts, the GUI restarts, a profile is changed,
or secure-store cleanup fails. Authentication state is useful to the runtime,
but raw enrollment input and bearer material are never safe UI state.

Desktop authentication must not introduce a server route, OpenAPI operation,
schema migration, desktop-owned HTTP client, desktop-owned SecretStore access,
or Prompt 102 product scope.

## Decision / Quyết định

### 1. Ownership and request path / Sở hữu và đường request

The only supported request path is:

```text
QML transient TextField
        -> DesktopUiBridge
           -> DesktopController
              -> Prompt 96 version-1 local IPC
                 -> synveil-client DesktopControlHandle
                    -> DesktopSyncHostHandle
                       -> HttpEnrollmentClient
                          -> existing enrollment exchange
                             -> LocalStateStore + SecretStore
                                -> CredentialChanged wake
                                   -> SyncRuntime
```

QML may collect a transient enrollment token and dispatch it. The bridge may
carry the call across the Qt thread boundary, but it does not parse, persist,
log, snapshot, copy, clipboard, or inspect the token. The bridge and QML have
no HTTP, `SecretStore`, SQLite, or bearer-token API.

`DesktopController` validates the input at the existing domain boundary and
admits at most one authentication operation at a time for its profile. Invalid
input returns a typed safe result before IPC and cannot start a task or reach
the host. The bounded Prompt 96 command channel and the controller's
authentication gate reject repetition; they do not create one task per click.

The local wire command is a backward-compatible extension of the existing
version-1 tagged command set:

```text
Authenticate { enrollment_token: bounded string }
SignOut
```

The version-1 handshake capability list remains compatible with existing
clients. An old peer treats an unknown tagged command as `UnknownCommand`; the
new controller reports a safe protocol result. No raw response body or remote
diagnostic crosses the boundary.

### 2. Input and secret handling / Xử lý input và secret

The QML enrollment field is masked, has a maximum encoded length of 69 bytes
(`DEVICE_SECRET_ENCODED_BYTES`), and clears immediately after dispatch. It is
not persisted by QML or Qt settings and is not placed in a snapshot, event,
tray label, clipboard operation, or UI log. The Rust IPC wrapper owns a
zeroizing string and redacts its `Debug` representation. The domain
`EnrollmentSecret` parser remains authoritative for the exact alphabet and
length.

The only durable secret write is the existing profile-bound SecretStore write
performed by `LocalStateStore::store_enrollment` or
`LocalStateStore::replace_enrollment`. The enrollment receipt validates the
configured immutable server profile and device scope before promotion. The
desktop shell never receives the returned device bearer secret or credential
ID as a user-facing value.

### 3. Durable-first authentication ordering / Thứ tự durable trước

For successful enrollment, the process performs the following ordering:

1. Validate the transient enrollment input.
2. Exchange it once through `HttpEnrollmentClient`.
3. Validate the returned profile/device receipt.
4. Persist the profile metadata and SecretStore value through the existing
   lifecycle, including readback/verification and cleanup of superseded
   values.
5. Only after durable success, send the existing `CredentialChanged` wake to
   the affected HTTP libraries.
6. Let the runtime reload the durable credential and publish its normal
   authenticated or blocked status.
7. Return only a category result such as `Authenticated` or a typed failure.

Persistence failure suppresses the success result and suppresses the
post-persistence wake. A wake that is stopped or coalesced does not roll back
the durable credential; the runtime's startup, periodic refresh, and
authoritative status path remain the correctness fallback.

### 4. Typed outcomes and unknown outcomes / Kết quả typed và kết quả unknown

The local result vocabulary is finite and contains no secret or diagnostic:

```text
Authenticated
SignedOut
InvalidCredentials
NetworkUnavailable
ServerUnavailable
RateLimited
SecureStoreUnavailable
Busy
ProtocolError
OutcomeUnknown
```

Invalid typed input and rejected enrollment produce `InvalidCredentials` and
zero SecretStore writes. Network/TLS/offline/timeout, server availability,
rate-limit, secure-store, and protocol failures remain distinguishable to the
controller and are mapped to safe generic UI copy. Raw transport errors are
not displayed.

If the IPC connection or response is lost after an authentication command was
admitted, the controller returns `OutcomeUnknown` and never replays the
enrollment exchange or Sign Out after reconnect. The UI refreshes canonical
status; it does not assume success or failure from a missing response. A
process restart similarly reloads only the durable profile-bound state in the
background client.

### 5. Explicit Sign Out / Sign Out tường minh

Sign Out is a separate explicit command and is not coupled to QML window close,
tray quit, GUI restart, client restart, or process termination. The background
host uses the existing forget lifecycle:

```text
durable forgotten/tombstone marker
        -> SecretStore deletion and verified cleanup
           -> CredentialChanged wake
              -> runtime reload and AuthBlocked state
```

The wake occurs only after the durable marker and secure-store cleanup succeed.
If deletion or secure storage is unavailable, the operation returns
`SecureStoreUnavailable` (or another safe typed failure) and no unauthenticated
runtime wake is published. The existing cleanup-on-restart path remains able
to finish an interrupted cleanup without a new network exchange.

### 6. Profile isolation and restart behavior / Cô lập profile và restart

Authentication is bound to the configured `ServerProfileId`, owner, device,
and registered HTTP libraries. A returned enrollment for another profile or
scope is rejected by the existing origin/scope checks. Libraries using another
profile are not woken or modified. The process host claims one compatible HTTP
profile for its configured runtime; incompatible registration is rejected.

After a background-process restart, the client reconstructs runtime state from
the existing durable SQLite metadata and SecretStore. After a GUI restart, the
controller reconstructs only safe status and the Sign Out affordance from its
fresh snapshot; it never sends a credential to QML. An IPC disconnect does not
cause automatic replay. Multiple GUI instances remain bounded by the local
IPC connection/command limits and the controller's per-instance in-flight
gate; the existing profile writer lock remains authoritative for durable
ownership.

### 7. Scope and non-decisions / Phạm vi và điều không quyết định

This ADR adds no server route, OpenAPI operation, migration, database table,
OAuth/OIDC flow, password login, API-key import, browser session, MFA, device
credential UI, upload/share behavior, or synchronization correctness rewrite.
It does not define a setup secret, distributed rate limiter, tray-auth flow,
or a new authentication store. The server's existing enrollment endpoint and
the existing local credential lifecycle remain the only authorities.

Native Windows execution, live server enrollment, and PostgreSQL end-to-end
coverage remain validation gates rather than claims made by this ADR. A Linux
build or static Windows policy test cannot be reported as native Windows
execution. The readiness marker for the full Prompt 101 acceptance is not
emitted unless all required live, restart, cross-platform, regression, and
repository gates are genuinely demonstrated.

## Consequences / Hệ quả

The GUI gains a minimal sign-in/sign-out surface without becoming a credential
owner. The background process retains the only HTTP and secure-storage
authority, and runtime wakeups are ordered after durable state. Authenticated
status can recover after a GUI restart, while a lost response remains
conservative and non-replaying.

The exchange is intentionally one-shot. A user must obtain a fresh enrollment
grant when an admitted exchange has an unknown result; the client must not
guess whether a device credential was issued. Sign Out is explicit and can
leave a safe retryable cleanup marker when secure-store deletion is interrupted.

No schema or server-contract migration is required. The principal new test
surface is the local control/controller result mapping, bounded input,
durable-first lifecycle signal, QML transient behavior, profile isolation, and
restart/unknown-outcome policy.

## Alternatives / Phương án khác

- Let QML call the enrollment endpoint: rejected because it would expose HTTP,
  credential, response, and retry policy to the GUI.
- Let `synveil-desktop` write the SecretStore: rejected because the process
  split would create two credential authorities and weaken restart recovery.
- Add password, OAuth/OIDC, API-key, or browser-session login: rejected because
  the existing enrollment exchange is the canonical desktop authentication
  contract.
- Persist the enrollment token in settings or a desktop database: rejected
  because the token is transient grant input, not durable application state.
- Replay an exchange after reconnect: rejected because a lost one-time
  exchange has an unknown server-side outcome.
- Treat GUI close or tray quit as Sign Out: rejected because UI lifecycle and
  durable credential lifecycle are independent.
- Wake the runtime before SecretStore verification: rejected because a worker
  could observe an unauthenticated or partially written credential state.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No migration is required. Review this ADR only if the canonical server
enrollment contract, profile/device binding, SecretStore transaction semantics,
Prompt 96 wire compatibility, or the explicit separation between GUI and
background process changes. Any materially different authentication scheme or
durable credential model requires a new superseding ADR.

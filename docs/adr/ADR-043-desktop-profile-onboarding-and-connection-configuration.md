# ADR-043: Desktop profile onboarding and connection configuration / Onboarding profile desktop và cấu hình kết nối

Status / Trạng thái

Accepted — LOCKED Prompt 102

Date / Ngày

2026-09-19

Decision owners / Chủ sở hữu quyết định

Synveil maintainers / Nhóm maintainers Synveil

## Context / Bối cảnh

Prompt 101 added device enrollment and credential lifecycle to the native
desktop path, but a first-run desktop process could not create the canonical
server profile required by that path. The Qt shell must collect only the
minimum non-secret connection input while `synveil-client` remains the owner of
profile state, HTTP verification, durable SQLite state, and `SecretStore`
credential fencing.

The existing profile subsystem already owns `ServerProfileId`,
`CanonicalBaseUrl`, profile-bound enrollment metadata, origin-bound credential
envelopes, and the single-writer local SQLite store. The existing HTTP client
already owns rustls validation, redirect rejection, bounded timeouts, and the
anonymous `/health/ready` contract. A second GUI profile registry or a new
server route would create competing authorities.

## Decision / Quyết định

### 1. Ownership and request path / Sở hữu và đường request

The only supported onboarding path is:

```text
QML server URL + label
    -> DesktopUiBridge
       -> DesktopController
          -> Prompt 96 version-1 local IPC
             -> synveil-client DesktopControlHandle
                -> DesktopSyncHostHandle
                   -> canonical profile store and HttpEnrollmentClient probe
```

QML does not write a manifest, SQLite row, secure-store value, or network
request. The bridge exposes only bounded, non-secret profile metadata and
category-only outcomes. The background client starts with a process-owned
opaque UUIDv7 identity even when zero libraries and no server row exist; this
makes first-run configuration a valid state without inventing a library.

The version-1 tagged IPC set adds the bounded operations
`GetProfileConfiguration`, `ValidateProfileConfiguration`,
`CreateOrConfigureProfile`, and `UpdateProfileConfiguration`. Complete
configuration payloads are not copied into change events; a successful
mutation emits a coalescible configuration-changed invalidation and the
controller refetches the authoritative snapshot.

### 2. Canonical URL and verification policy / Chính sách URL và kiểm tra

`CanonicalBaseUrl` remains the only production parser. It accepts a strict
HTTPS origin with a hostname, IPv4 literal, IPv6 literal, or explicit valid
port, and normalizes the canonical trailing root slash through the existing URL
type. Userinfo, credentials, query, fragment, application subpaths, malformed
ports, whitespace/control characters, backslashes, and unsupported schemes are
rejected. The existing explicit numeric-loopback HTTP constructor remains a
test-only exception; onboarding uses production HTTPS parsing and does not
silently upgrade or downgrade the input.

Syntactic acceptance is not configuration success. The client constructs the
existing rustls HTTP transport and performs one bounded anonymous `GET
/health/ready` request, requiring the strict `{ "status": "ready" }` DTO. The
transport keeps certificate verification, no redirects, no proxy, bounded
body/deadline behavior, and no retry policy. Invalid input, offline/timeout,
TLS failure, incompatible response, and server failure map to finite typed
outcomes; raw HTML, response bodies, headers, and diagnostics never reach QML.

### 3. Durable-first create/update / Tạo và cập nhật theo durable-first

`ValidateProfileConfiguration` probes without mutation. The onboarding apply
operation probes first, then calls `LocalStateStore::configure_server_profile`.
The client reports success only after the canonical state write completes. A
new process-owned identity creates one `server_profiles` row; an exact repeat
is idempotent; a label-only edit preserves the existing connection timestamp.
The profile ID and creation time remain stable.

The existing URL/identity trigger previously rejected every URL edit. Migration
`0007_profile_reconfiguration.sql` narrows that database invariant to the
opaque `profile_id`; the Rust canonical configuration transaction is now the
only application mutation path for origin edits. This is a justified client
migration: without it, the required native correction flow could not update a
misconfigured profile. No server migration is added.

### 4. Server identity changes fence credentials / Thay đổi identity server

When the origin changes, the state store serializes the transition, marks the
active enrollment forgotten, records cleanup intent, completes old-origin
SecretStore cleanup while the old profile still exists, and only then commits
the new origin. The profile ID remains immutable. The runtime receives its
credential-change wake only after the durable transition, and the next status
snapshot is unauthenticated for the new origin. A cleanup or persistence
failure does not report the replacement as configured; no credential issued by
server A can be loaded or sent as server B's credential.

### 5. Admission, generations, and lost responses / Admission, generation và mất response

The controller and process control handle each admit at most one profile
validation/apply operation per process context. The Qt bridge has a separate
bounded gate and disables the native apply affordance while work is pending.
The existing latest-value snapshot and generation-fenced reconnect path remain
authoritative; a stale probe cannot overwrite a later state. A lost IPC
response maps to `OutcomeUnknown`, requests one coalesced authoritative
refresh, and never blindly replays a possibly committed configuration
mutation.

### 6. Onboarding, authentication, and restart / Onboarding, xác thực và restart

The safe state transition is:

```text
profile identity only
  -> ConfigurationRequired
  -> verified durable profile
  -> Unauthenticated
  -> Prompt 101 device enrollment
  -> Authenticated
```

The authentication flow remains a separate Prompt 101 operation. Configuration
does not accept an enrollment token and does not write credentials. GUI close,
tray quit, or GUI restart never stops the client or signs the device out.
After either process restarts, the client/controller reconstructs the profile
from the canonical manifest/SQLite/SecretStore paths; QML has no cache that is
required for recovery. An empty library list remains valid and is not treated
as deletion or mass root loss.

## Consequences / Hệ quả

First-run users receive a usable native setup form and can correct a server
address without hand-editing files or databases. The profile URL and label are
safe presentation metadata, while credentials remain outside the GUI. The
desktop can reach the unauthenticated Prompt 101 state before any library is
configured.

The client schema advances from 6 to 7 only for the reconfiguration trigger
change. Upgrade tests must cover migration 6 -> 7 and fresh databases; no
server schema or API contract changes are required. Native Windows execution,
real server acceptance, PostgreSQL end-to-end setup, and interactive display
testing remain validation evidence rather than claims made by this ADR.

## Alternatives / Phương án khác

- A GUI-owned config file or HTTP client was rejected because it would create a
  second authority and bypass the secure local IPC/client boundary.
- Persisting before probing was rejected because a syntactically valid but
  incompatible target would become authoritative false state.
- Reusing one profile ID per URL was rejected because URL aliases and origin
  changes must not silently reuse device credentials.
- A new server health/version route was rejected because the existing
  anonymous readiness DTO already provides the narrow compatibility probe.
- Deleting and recreating a profile was rejected because it would lose durable
  identity and weaken profile/library isolation; correction preserves the
  opaque ID and fences credentials explicitly.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

Review this ADR if the server introduces a signed installation identity or a
version/capability endpoint that is stronger than anonymous readiness, or when
multi-profile selection and library setup become product scope. Revisit the
client migration only if a future schema can represent the same immutable
identity plus origin-edit transaction without the current trigger change.

## Bản tóm tắt tiếng Việt

`synveil-desktop` chỉ nhận URL HTTPS và nhãn profile không bí mật. Mọi parse,
probe, persistence và lifecycle credential đều đi qua `DesktopController`, IPC
cục bộ Prompt 96 và `synveil-client`; QML không có HTTP, SQLite hay
`SecretStore`. Client dùng `CanonicalBaseUrl`, probe `GET /health/ready` với
DTO `status=ready`, rồi mới ghi profile bền vững. Profile ID opaque UUIDv7 và
creation time không đổi; không có library vẫn là state hợp lệ.

Khi đổi origin, transaction đánh dấu enrollment cũ forgotten, dọn SecretStore
theo cleanup intent trước khi ghi origin mới, rồi mới wake runtime. Vì vậy
credential của server A không thể được dùng cho server B. Apply/validate được
admission bounded; response bị mất trả `OutcomeUnknown`, refresh state và
không replay mutation. Restart đọc lại manifest, SQLite và SecretStore chuẩn;
GUI không cần cache riêng. Migration client `0007_profile_reconfiguration.sql`
chỉ nới invariant để Rust canonical path sửa origin; không có server migration.

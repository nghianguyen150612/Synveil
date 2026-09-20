# ADR-048: Production desktop recovery and resilience UX

Status: Accepted — LOCKED Prompt 107
Date: 2026-09-19
Owners: Synveil client-sync, client, desktop, and documentation maintainers

## Context

Prompts 101–106 established durable authentication, profile and library
onboarding, existing-root bootstrap, process launch, pause/resume, and
attention/conflict semantics. Those subsystems already know whether a client,
profile, credential, root, runtime, or setup operation can make progress. The
remaining production gap was the desktop explanation and safe entry point for
those states. A GUI-only repair model would create a second source of truth and
would be unsafe around missing roots, uncertain mutations, and interrupted
bootstrap.

The recovery surface therefore has to be a derived projection:

```text
canonical state -> typed recovery summary -> safe explanation
  -> existing supported owner -> authoritative refresh
```

It must compose with Prompt 105's user pause and Prompt 106's durable
attention without treating either as a generic failure.

## Decision

### Recovery is a derived controller projection

`DesktopControllerSnapshot::recovery_summary()` derives a bounded
`DesktopControllerRecoverySummary` from the latest coherent process/profile/
library snapshot. It is not persisted and it does not add a recovery database,
IPC journal, or parallel runtime state. The projection contains stable library
IDs, typed categories/actions, action-required versus waiting flags, and the
connection generation. It contains no path, credential, token, cookie, HTTP
body, database row, or operating-system diagnostic.

The client-side recovery categories are deliberately narrow:

| Category | Source evidence | Presentation | Supported owner |
|---|---|---|---|
| `ClientUnavailable` | controller disconnect, terminal process state, or an unavailable control endpoint | Background client unavailable | `BackgroundClientManager` |
| `ProfileConfigurationRequired` | fresh connected profile without configuration | Configure connection | existing profile form/controller |
| `AuthenticationRequired` | missing/blocked/revoked auth or `AuthBlocked` runtime | Sign in | Prompt 101 auth path |
| `RootUnavailable` | unavailable root or `RootBlocked` runtime | Local folder unavailable | existing root validation/runtime wake |
| `ServerRetryable` | offline/transient/rate-limited outcome or bounded runtime backoff | Waiting for server | existing `SyncNow` wake |
| `LocalFailure` | durable recovery-blocked/fatal/panicked outcome or faulted runtime | Local sync needs attention | existing runtime/controller recheck |
| `LibrarySetupIncomplete` | authenticated profile with no active library | Resume setup | existing pending/active setup reconciliation |

`Recovering`, reconnect, and bounded backoff are waiting states rather than
intrusive action-required alerts. User pause and conflict attention are not
duplicated into the recovery list. They remain independently visible and
authoritative in their Prompt 105/106 surfaces.

The source-backed ownership matrix is:

| Canonical state | Owner | Durable or transient | User action | Automatic recovery | Safe manual action |
|---|---|---|---|---|---|
| client unavailable/reconnecting | `DesktopController` + `BackgroundClientManager` | controller/process transient | only when unavailable | reconnect/launch manager | Start client |
| profile not configured | profile/config subsystem | profile durable absence | yes | none | Configure connection |
| auth missing/blocked/revoked | `SecretStore` + host/auth pipeline | credential/profile durable, runtime state transient | yes | no blind re-auth | Prompt 101 sign-in |
| `SecureStoreUnavailable` | `SecretStore` result | transient operation result | retry may be useful | no deletion or overwrite | retry existing auth |
| root unavailable/recovering | `DesktopSyncHost` root lifecycle | observation transient; binding remains durable | only after external root fix | bounded probe/wake | Check again |
| offline/server transient/rate limit/backoff | `SyncRuntime` | runtime/backoff transient | normally no | bounded backoff | optional bounded `SyncNow` |
| recovery-blocked/fatal local/faulted | `SyncRuntime` + durable issue/attention owner | outcome/runtime plus durable issue where applicable | yes if canonical issue remains | no destructive repair | Check again / existing attention |
| pending library/bootstrap | client config + host setup | pending binding durable | yes when form must be resumed | reconciliation on setup/restart | Resume setup with same root |
| uncertain command outcome | controller transport boundary | transient and deliberately unknown | wait for refresh | authoritative refresh | no immediate replay |
| `PausedByUser` | client sync-control store/runtime | durable user setting | only if user wants resume | none | Resume in Settings |
| conflict attention | `LocalStateStore`/Prompt 106 | durable | yes | never auto-resolve | existing attention action |

This matrix deliberately does not promote raw internal error strings or
unsupported categories such as root relocation, database repair, server admin,
or filesystem permission changes into desktop actions.

### No new IPC command is necessary

The projection reuses the Prompt 96 v1 typed surfaces already present:

- controller snapshots and bounded event invalidation provide coherent status;
- `SyncNow` is the canonical bounded runtime wake for root, server, and local
  recovery checks;
- existing profile/auth/setup commands remain the owners of onboarding and
  authentication;
- existing `BackgroundClientManager` owns process start/coalescing;
- existing attention commands remain the only conflict decision path.

The bridge does not add a generic `Repair`, `CheckRoot`, or `GetRecoveryState`
protocol command. QML receives only a bounded QVariant list of fixed safe
labels, stable IDs, booleans, and the generation fence. QML action routing uses
fixed typed invokables (`startBackgroundClient`, `retrySelectedRecovery`, and
`resumePendingSetup`); no free-form command string crosses the boundary.

### Action ownership and ordering

The only recovery mutations are existing supported operations:

- Starting the client calls `BackgroundClientManager::ensure_running`, which
  retains canonical executable resolution, no shell/PATH interpolation, one
  in-flight admission, and cooldown behavior.
- `Check again` calls the existing controller `SyncNow` admission. It is a
  bounded wake, not a backoff reset, database repair, broad rescan, or blind
  replay. A lost response becomes `OutcomeUnknown`; the controller refreshes
  and does not issue a second wake.
- Sign in and profile configuration route to their existing forms and Prompt
  101/102 controller paths. A `SecureStoreUnavailable` result is shown as
  sanitized credential-store-unavailable feedback and does not delete or
  overwrite the stored credential.
- Resume setup reopens the existing library setup surface. Choosing the same
  configured folder lets the client reuse its durable pending identity and
  perform the existing server reconciliation/bootstrap sequence. A different
  root is not silently adopted.

Every durable transition remains owned by the canonical subsystem. The
existing durable-first rules therefore remain in force: persist a credential,
setup, conflict, or root transition first; signal/wake second; then refresh the
authoritative snapshot. The bridge never opens SQLite, edits configuration
manifests, deletes state, changes root markers, calls server HTTP, or executes
repair commands.

### Root safety and restart behavior

An unavailable or inaccessible root is never interpreted as an empty tree and
never creates a deletion storm. The UI says that the configured local folder is
unavailable and offers only a bounded check. Same-path restoration follows the
existing root probe and runtime wake; no root relocation operation is added.
Marker/identity mismatch remains fail-closed under the existing setup/root
validation path.

Recovery state survives GUI absence because the client/runtime and existing
durable setup/attention records remain the authorities. A fresh controller
after reconnect or GUI restart derives the current state again. Recovery items
carry the controller connection generation; stale UI items are refreshed before
an action is admitted. Recovery presentation is capped at 128 items and marks
truncation.

### Schema, server, and feature boundaries

This decision adds no server route, OpenAPI operation, server migration, client
SQLite migration, credential table, notification, updater, or generic repair
console. Migration counts remain server **36**, `LOCAL_SCHEMA_VERSION` **7**,
and **7 client migrations**. Database repair, root relocation, undelete,
conflict redesign, server administration, and filesystem recovery remain
deferred.

## Consequences

- Users can distinguish an action-required blocker from automatic waiting.
- Root disappearance remains data-loss-safe and same-root recovery remains
  canonical.
- Client restart, GUI restart, lost IPC responses, pause, and conflict state do
  not create duplicate operations or silently change durable state.
- Credential-store failure is not misreported as an invalid password and does
  not cause secret deletion.
- There is intentionally no recovery-specific IPC command or durable table;
  source-level/unit evidence must be complemented by native, live, and restart
  acceptance before any readiness marker is claimed.

## Alternatives rejected

- A generic repair console or SQLite editor would bypass canonical ownership.
- Treating root loss as an empty folder could create destructive deletion
  intents.
- Adding a second retry engine could bypass runtime backoff and replay an
  operation whose response was lost.
- Reusing a selected different folder would be an unreviewed root relocation
  and could bind the wrong library identity.
- Duplicating Prompt 106 conflicts in recovery would make one durable problem
  appear to have two decision owners.
- Adding a `GetRecoveryState` IPC command would duplicate the already coherent
  Prompt 96 status/event surfaces without adding canonical information.

## Review trigger

Review this ADR if canonical recovery requires a new durable state, if a
server-side recovery operation becomes necessary, if root relocation is
designed, or if a future platform can provide a supported credential-store
repair action. Such changes require a new contract and security review.

## Tóm tắt tiếng Việt

### Bối cảnh và quyết định

Prompt 101–106 đã có state durable cho authentication, profile/library
onboarding, bootstrap root đã có dữ liệu, launch process, pause/resume và
attention/conflict. Khoảng trống còn lại là cách desktop giải thích blocker và
mở đúng action an toàn. Không tạo repair console hay source of truth thứ hai;
recovery chỉ là projection derived:

```text
canonical state -> recovery summary typed -> giải thích an toàn
  -> subsystem hiện có -> refresh authoritative
```

`DesktopControllerSnapshot::recovery_summary()` trả
`DesktopControllerRecoverySummary` bounded, không persist. Item chỉ có library
ID ổn định, category/action typed, cờ action-required/waiting và connection
generation. Không có path, credential, token, cookie, HTTP body, database row
hay raw OS diagnostic.

Category gồm `ClientUnavailable`, `ProfileConfigurationRequired`,
`AuthenticationRequired`, `RootUnavailable`, `ServerRetryable`,
`LocalFailure` và `LibrarySetupIncomplete`. Reconnect, `Recovering` và backoff
bounded là waiting; pause của user và conflict attention không bị duplicate.

Không thêm IPC command mới. Snapshot/event Prompt 96 cung cấp status coherent;
`SyncNow` là wake bounded canonical; profile/auth/setup vẫn đi qua controller
hiện có; launch dùng `BackgroundClientManager`; conflict vẫn do attention
surface Prompt 106 sở hữu. QML chỉ nhận list bounded gồm ID/label/boolean/
generation an toàn và gọi invokable typed cố định, không gửi command string tự
do.

`Check again` chỉ wake runtime hiện có, không reset backoff, repair database,
broad rescan hay replay. Mất response là `OutcomeUnknown`, refresh và không
gọi lại mù. `SecureStoreUnavailable` hiển thị feedback generic, không xóa hoặc
ghi đè credential. `Resume setup` dùng lại setup/pending identity và
reconciliation hiện có; không tự nhận một root khác.

Root unavailable không bao giờ có nghĩa tree rỗng hoặc tạo deletion storm.
Khôi phục đúng same path sẽ probe/wake theo subsystem hiện có; root relocation,
marker overwrite và repair nguy hiểm vẫn fail-closed/deferred. Recovery sống
qua GUI restart vì client/runtime và durable state là authority; generation
fence ngăn action từ snapshot cũ. Projection tối đa 128 item và báo
`truncated`.

Không thêm server route/OpenAPI, server migration, client SQLite migration,
credential table, notification, updater hay generic repair console. Counts giữ
server **36**, `LOCAL_SCHEMA_VERSION` **7**, **7 client migrations**. Readiness
marker chỉ hợp lệ sau khi native/live/restart gates thật sự pass; unit/source
evidence không tự biến thành live acceptance.

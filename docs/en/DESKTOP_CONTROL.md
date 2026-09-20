# Secure local desktop control IPC (Prompt 96)

This document describes the local process-control protocol owned by the
already-running `synveil-client`. It is not the Synveil server HTTP API and is
not exposed through OpenAPI.

## Boundary and ownership

```text
future UI / tray / diagnostics
            |
     DesktopControlClient
            |
  local v1 framed protocol
            |
  synveil-client control server
            |
  DesktopSyncHostHandle
            |
  one SyncRuntime -> existing sync cycle
```

`DesktopClientProcess` owns exactly one control server, one
`DesktopSyncHost`, and one `SyncRuntime`. Control code never opens SQLite,
loads a secret, constructs another runtime/host, invokes the Prompt 91 engine,
or maintains a second synchronization state machine. Secure control binding is
part of production bootstrap; failure is a typed fatal bootstrap error. The
server stops before the existing Prompt 95 graceful host shutdown continues.

## Transports and endpoint identity

On Linux the endpoint is a Unix-domain socket at:

```text
<canonical platform runtime directory>/synveil/<opaque profile ID>.sock
```

The existing platform path resolver supplies the runtime directory. The
resolver uses `XDG_RUNTIME_DIR` when present and an application-owned state
fallback otherwise; process code does not parse those environment variables.
The profile ID is the existing opaque UUIDv7 profile identity. No token,
credential, URL, username, or filesystem root participates in the name.

On Windows the real transport branch uses a deterministic profile-scoped
named pipe. Windows syntax is confined to the Windows transport branch; there
is no TCP or localhost fallback. macOS and generic adapters report an explicit
unsupported-platform control error in this phase rather than weakening the
security model.

Routine `Debug` and server diagnostics expose only endpoint kind. A caller that
explicitly needs to connect may obtain the generated endpoint from the client
API; ordinary process/library status never contains the endpoint path.

## Local authorization

Linux validates the runtime directory as absolute, owned by the current user,
non-symlink, and free of group/other write permission. It creates the
`synveil` control directory with owner-only mode `0700` and verifies the
socket after bind as an owner-only `0600` socket. A peer is admitted only when
Unix peer credentials report the same UID as the process.

If the exact expected path already exists, only an owned socket is eligible for
stale inspection. A successful connect means an active endpoint and returns
`CONTROL_ENDPOINT_ALREADY_ACTIVE`. A refused/not-found socket is rechecked and
removed only when it is still the exact expected non-symlink socket owned by
the current user. A symlink, regular file, directory, device, wrong owner, or
uncertain state is refused and never replaced. The server's drop guard also
removes only its own verified socket.

Windows uses Tokio named pipes with remote clients rejected, a bounded maximum
of 32 instances, and a protected security descriptor with an owner-rights
allow ACE. There is no Everyone or Anonymous ACE. Failure to create that
descriptor is a control bootstrap failure, not a reason to relax permissions.

## Protocol and framing

The initial protocol version is `1`. Every connection sends:

```json
{"protocol_version":1}
```

and receives a `ServerHello` containing the version and capabilities. An
unsupported version receives `ProtocolVersionUnsupported` and the connection
closes safely. Every subsequent request is a structured JSON object with a
non-zero `request_id`; its response echoes the same ID.

Each JSON payload is framed as a four-byte big-endian unsigned length followed
by exactly that many bytes. The payload maximum is `64 KiB`; zero-length,
oversized, truncated, malformed, and invalid JSON frames are rejected without
allocation based on an unchecked peer length. No control read uses
`read_to_end`.

One connection processes requests sequentially. The server admits at most 32
active connection tasks. Handshake input is bounded by a short timeout;
post-handshake idle connections are bounded by an idle timeout. Slow writers,
disconnects, and malformed peers are isolated to their task and cannot block
the sync runtime or the server's shutdown join.

## Gen-1 commands and safe data

| Command | Result |
|---|---|
| `Ping` | Current safe process state. |
| `GetProcessStatus` | `Starting`, `Running`, `Stopping`, `Stopped`, or `Faulted`, plus control readiness. |
| `ListLibraries` | Bounded list of stable library IDs and summarized categories. |
| `GetLibraryStatus` | One stable library ID's summarized runtime/root/auth/conflict status. |
| `SyncNow` | `Queued`, `Coalesced`, or `AlreadyRunningFollowupRecorded`; scheduling only. |
| `Shutdown` | `ShutdownAccepted` after the request is accepted by the outer process lifecycle. |
| `SubscribeEvents` | A bounded event connection. |

Library status is translated from the existing host/runtime. Root state is
`Available`, `Unavailable`, or `Recovering`; the raw root is absent. Auth is
reported only when existing runtime evidence proves `Blocked` or a successful
authenticated outcome; otherwise it remains `Unknown`. Credential values,
credential IDs, cookies, raw authorization headers, server URLs, file names,
content, checkpoint/snapshot/handoff data, and conflict payloads are absent.

`SyncNow` calls `DesktopSyncHostHandle::sync_now`, which is the existing
Prompt 93/92 scheduling path. It never calls Prompt 91 directly and never
claims that a sync cycle completed. `Shutdown` never calls `process::exit`,
aborts the runtime, or kills a task. The handler writes `ShutdownAccepted`
before waking the outer `DesktopClientProcess`; the process then stops the
control listener and follows the canonical Prompt 95 adapter/host shutdown.

## Events and reconnect

Events are best-effort invalidations from the existing runtime, process
lifecycle, and root-availability signals:

- `ProcessStateChanged`
- `LibraryStatusChanged { library_id }`
- `RootAvailabilityChanged { library_id, state }`
- `SyncCycleCompleted { library_id, outcome }`
- `ControlServerStopping`
- `Lagged { dropped_count }`

The broadcast channel has capacity 256. A slow subscriber does not backpressure
the producer; it receives a lag indication and must reconnect/refetch status.
Events do not form a durable journal and loss cannot affect synchronization
correctness. A client can reconnect and handshake again after its own restart
or after the process restarts. No IPC table, session, token, event journal, or
migration is added.

## Validation and limitations

The focused Linux target is `cargo test -p synveil-client --test
desktop_control_ipc --locked`. It uses a short disposable runtime root and
checks real socket type, owner, modes, handshake, request IDs, version
mismatch, unknown commands, malformed frames, reconnect, events, and shutdown
acknowledgement ordering. Unit coverage checks active endpoint collision,
stale recovery, symlink/regular-file/directory refusal, and bounded framing.

The Prompt 95 PostgreSQL process target additionally exercises the real
production bootstrap and control client around a live configured process; it
must be run with the repository's disposable PostgreSQL 17 infrastructure.
Native Windows execution is not claimed by Linux-local validation. The
Windows named-pipe branch is nevertheless real code and must pass native
Windows and explicit cross-target compilation gates before the readiness marker
is valid.

## Authentication commands and credential boundary (Prompt 101)

Prompt 101 extends the existing version-1 tagged command set with:

```text
Authenticate { enrollment_token: bounded string }
SignOut
```

The request token is transient wire input only. It is bounded to the existing
69-byte encoded secret limit, parsed again by the process-owned domain boundary,
and held in a zeroizing/redacted wrapper. It is never written to a snapshot,
event, tray label, client-side settings store, or log. The request necessarily
contains the token while crossing the authenticated local IPC connection; the
security property is that QML/client presentation code does not persist,
inspect, or own it.

The process handles the commands through `DesktopControlHandle` and
`DesktopSyncHostHandle`. Only that background path may call
`HttpEnrollmentClient`, `LocalStateStore`, or `SecretStore`. The response is
category-only: `Authenticated`, `SignedOut`, `InvalidCredentials`,
`NetworkUnavailable`, `ServerUnavailable`, `RateLimited`,
`SecureStoreUnavailable`, `Busy`, `ProtocolError`, or `OutcomeUnknown`.
Responses, status snapshots, and events contain no token, bearer secret, cookie,
authorization header, raw URL, root path, or remote diagnostic.

No new handshake capability is required, preserving version-1 compatibility.
An old peer returns the existing unknown-command protocol error, which the
controller maps to safe protocol feedback. An admitted command whose response
is lost is not retried after reconnect; the controller reports
`OutcomeUnknown` and refreshes authoritative status. Authentication and Sign
Out use the existing durable-first credential lifecycle, and no new IPC
session, auth table, migration, HTTP route, or OpenAPI operation is added. See
[`ADR-042`](../adr/ADR-042-secure-desktop-authentication-and-credential-lifecycle.md).

## Profile onboarding and connection configuration (Prompt 102)

The version-1 local control protocol adds these bounded, non-secret commands:

```text
GetProfileConfiguration
ValidateProfileConfiguration { base_url, display_label }
CreateOrConfigureProfile { profile_id, base_url, display_label }
UpdateProfileConfiguration { profile_id, base_url, display_label }
```

The response contains only a safe profile ID/origin/label/timestamp view or a
finite outcome such as `Validated`, `Created`, `Updated`,
`AlreadyConfigured`, `InvalidServerAddress`, `NetworkUnavailable`, `Timeout`,
`TlsFailure`, `IncompatibleServer`, `ServerFailure`, `PersistenceFailure`,
`Busy`, or `OutcomeUnknown`. A successful mutation emits only a bounded
configuration-change invalidation; the controller refetches the authoritative
profile snapshot. No raw URL parser error, HTTP body, TLS object, credential,
SecretStore record, or database row crosses IPC. A lost response is not
replayed and instead causes an authoritative refresh. See
[`ADR-043`](../adr/ADR-043-desktop-profile-onboarding-and-connection-configuration.md).

## Library setup and root binding (Prompt 104)

Version-1 control adds the bounded non-secret command
`SetupLibrary { name, root_path }` and the `LibrarySetup` capability. The
client validates the root and logical name, performs the authenticated server
library create/reconciliation, commits the existing local profile-bound
replica/root state, and only then registers the host/runtime. The response is
one finite category such as `Configured`, `AlreadyConfigured`, invalid name or
root, `AuthenticationRequired`, server/network failure, `PersistenceFailure`,
`OutcomeUnknown`, or `Busy`.

The bridge accepts a native folder-picker result, but it does not expose the
absolute path in snapshots, diagnostics, or feedback. A zero-library
authenticated snapshot sets `library_setup_required`; setup admission is
bounded and a successful or uncertain result triggers an authoritative refresh.
The client owns pending/active manifest recovery, so a GUI restart does not
lose an interrupted setup and closing the GUI does not stop the client or
runtime. A newly created remote library may be initialized from a writable
existing local tree. The client first admits the exact profiled root, seeds the
authoritative remote root NodeId into local state, and lets the existing
bounded observer create ordinary outbound intents; it does not expose a bulk
uploader or a separate import path. An existing remote-library attach/import
flow remains unsupported. See
[`ADR-044`](../adr/ADR-044-desktop-library-onboarding-and-local-root-binding.md)
and [`ADR-045`](../adr/ADR-045-existing-root-bootstrap-and-initial-upload-admission.md).

## Essential sync controls and settings (Prompt 105)

Version-1 control also exposes the process-owned global sync control:

```text
GetSyncControlState
PauseSync
ResumeSync
```

The handshake advertises the bounded `SyncControl` capability. The state is
`Running` or `PausedByUser`; the mutation result is one of `Paused`,
`Resumed`, `AlreadyPaused`, `AlreadyRunning`, `PersistenceFailure`, or
`Busy`. `SyncNow` while paused returns the typed `Paused` scheduling result and
does not resume or bypass the pause.

The client writes `paused`/`running` to its small non-secret process setting
beside `client.conf` before it signals the existing runtime. A write failure
does not change runtime state. While paused, periodic, local-change,
network/inbound, credential, and manual scheduling paths are blocked at the
one runtime admission gate; an already-running bounded cycle is allowed to
finish. Local durable observation and profile/auth/control operations remain
available. Resume clears only the user-pause reason and lets existing queued
eligibility proceed; it does not perform a broad rescan or create a second
queue. `SyncControlStateChanged` is an invalidation event, so the controller
refetches state after lag or reconnect.

The bridge exposes only the state label, generic feedback, and busy flag. A
lost pause/resume response is `OutcomeUnknown` and causes refresh rather than
blind replay. No pause file path, IPC endpoint, runtime object, root path,
credential, or raw OS diagnostic crosses this boundary. The `SyncControl`
extension adds no server route, OpenAPI operation, SQLite migration, or
credential/session state.

## Production attention and conflict resolution (Prompt 106)

Version-1 control exposes the durable local attention projection through the
`Attention` capability:

```text
GetAttentionSnapshot
ResolveConflict {
    library_id,
    conflict_id,
    intent_id,
    detected_at_ms,
    action: AcceptRemote | RetryLocalAgainstCurrentBase
}
```

The snapshot contains exact unresolved counts for canonical sync conflicts and
other durable local blockers, per-library summaries, and at most 32 conflict
items by default (hard maximum 128). It reports truncation explicitly. An item
contains only stable IDs, one of the six existing conflict kinds, a bounded
managed relative path/previous path, file-or-directory kind, known lengths,
revision/state metadata, and the actions allowed by the canonical conflict
policy. It contains no content bytes, hashes, absolute roots, staging paths,
credentials, cookies, raw HTTP bodies, or server diagnostics. The UI must not
turn an unavailable root into an empty-tree action.

The client-sync state store remains the authority. The host persists a
resolution in the existing conflict transaction before requesting a normal
runtime wake. `PausedByUser` remains active: resolving a conflict does not
resume or bypass synchronization. Library, conflict, intent, and detection
timestamp form a generation fence; stale requests are rejected. Duplicate
resolution is surfaced as `AlreadyResolved`, unsupported retry as
`UnsupportedAction`, and only one resolution is admitted at a time in each
control/presentation boundary.

`AttentionStateChanged` is a bounded invalidation event. A fresh snapshot is
authoritative after reconnect, restart, event loss, stale results, or an
uncertain response. A lost response is `OutcomeUnknown`; the controller
refreshes and never replays the action. The desktop therefore displays durable
attention after a GUI restart without becoming a second sync engine. This
extension adds no server route, OpenAPI operation, SQLite migration, or
credential/session state. See
[`ADR-047`](../adr/ADR-047-production-desktop-attention-and-conflict-resolution.md).

## Production recovery and resilience UX (Prompt 107)

Recovery is a derived projection over the latest coherent controller snapshot;
it is not a second durable store or a generic repair console. The controller
classifies only evidence already owned by the client/process/runtime boundary:

| Typed category | Safe desktop explanation | Action owner |
|---|---|---|
| `ClientUnavailable` | Background client unavailable | Existing `BackgroundClientManager` |
| `ProfileConfigurationRequired` | Server connection required | Existing profile form/controller |
| `AuthenticationRequired` | Sign-in required | Prompt 101 authentication path |
| `RootUnavailable` | Local folder unavailable | Existing root validation/runtime wake |
| `ServerRetryable` | Waiting for server | Existing bounded `SyncNow` wake |
| `LocalFailure` | Local sync needs attention | Existing runtime/controller recheck |
| `LibrarySetupIncomplete` | Library setup incomplete | Existing pending/active setup reconciliation |

Reconnect, root `Recovering`, and runtime backoff are waiting states with
bounded automatic recovery; they are not presented as action-required failures.
`PausedByUser` remains a user control, and Prompt 106 conflicts remain in the
durable attention surface. Both can coexist with recovery without duplicate
controls.

The typed `DesktopControllerRecoverySummary` is derived from fresh connected
state only. An unavailable or stale controller exposes a client-level waiting
or unavailable item and does not combine it with a falsely fresh library list.
Each item carries only a stable library ID, fixed category/action enums,
action-required/waiting flags, and the connection generation. The projection is
bounded at 128 items and reports truncation. No root path, SecretStore detail,
token, cookie, HTTP body, SQLite row, or raw OS error crosses into QML.

No new Prompt 96 command is needed. The bridge's fixed invokables reuse existing
owners:

- `Start client` calls `BackgroundClientManager::ensure_running`, retaining
  canonical sibling executable resolution, no shell/PATH interpolation, one
  in-flight admission, and launch cooldown.
- `Check again` calls the controller's existing `SyncNow` scheduling path. It
  wakes the runtime but does not reset backoff, wipe state, broad-rescan, or
  replay a possibly committed mutation. A lost response is `OutcomeUnknown`,
  followed by authoritative refresh without replay.
- `Sign in` and connection configuration remain the existing Prompt 101/102
  forms. `SecureStoreUnavailable` is sanitized as temporary secure-storage
  unavailability and does not delete or overwrite a credential.
- `Resume setup` reopens the existing library setup surface. The user must
  choose the same configured folder; pending identity and bootstrap
  reconciliation remain owned by the client. Root relocation is not introduced.

Root disappearance is explicitly not an empty tree: it remains fenced and
cannot generate a mass deletion. When the exact root returns, the existing
root probe and runtime wake re-establish canonical status without resetting
sync state. Marker or identity mismatch continues to fail closed. Durable
setup, attention, and runtime state belongs to the client, so GUI absence or a
fresh controller after reconnect reconstructs the same recovery view.

All durable transitions retain durable-first ordering: persist the canonical
credential/setup/conflict/root transition, then signal or wake, then refresh.
The desktop bridge never opens SQLite, edits a manifest, deletes state,
rewrites markers, calls server HTTP, or executes shell repair commands. See
[`ADR-048`](../adr/ADR-048-production-desktop-recovery-and-resilience-ux.md).

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

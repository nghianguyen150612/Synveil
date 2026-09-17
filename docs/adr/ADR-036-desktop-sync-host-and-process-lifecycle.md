# ADR-036: Desktop synchronization host and process lifecycle composition

Status / Trạng thái: **LOCKED**

Date / Ngày: 2026-09-13

Decision owners / Chủ sở hữu quyết định: Architecture, Client Sync, Security

## Context / Bối cảnh

Prompts 91–93 provide the bounded bidirectional cycle, the process-local
`SyncRuntime`, and durable-change-first runtime signals. The repository does
not yet contain a desktop executable or a second client composition framework.
The missing boundary is therefore application composition: one process-owned
object must assemble the accepted graph, expose narrow controller operations,
and coordinate process lifecycle without becoming another synchronization state
machine.

The current desktop account model is one explicitly scoped owner/Device/
Library context at a time. `LocalStateStore`, `ServerProfile`/
`HttpSyncRemote`, `LocalReplica`, `OutboundObservationEngine`, and the existing
platform `SecretStore` remain the authoritative resource and security
boundaries. Runtime lifecycle is ephemeral; SQLite schema V6 and server
migration 36 must not change for this composition step.

## Decision / Quyết định

1. `synveil-client-sync` owns the canonical `DesktopSyncHost` composition root.
   It owns exactly one `SyncRuntime` for its process/account synchronization
   context. The first registered Library establishes the owner/Device context;
   later registrations from another account or Device are rejected with the
   typed `WrongScope` error. Cloning `DesktopSyncHost` or
   `DesktopSyncHostHandle` only clones a reference to that host; it never
   creates another supervisor.
2. The host accepts explicit library configurations and registers every
   library exactly once before any configured observer is started. The durable
   SQLite library/replica registry remains canonical; the host keeps only
   ephemeral process references and does not add a host registry table.
3. For production HTTP libraries the host validates the profile/root binding,
   prepares the existing replica row without requiring a credential, and
   lazily loads the credential through the existing profile-bound `SecretStore`.
   Because `HttpSyncRemote` captures an immutable loaded credential, only that
   library's Prompt 91 transport graph is rebuilt after a verified credential
   ID change; the host, runtime, observers, and notifier remain the same
   objects. A missing credential is an `AuthBlocked` runtime outcome, not a
   construction failure.
4. The host composes `InboundSyncEngine`,
   `RebaselineConvergenceCoordinator`, `OutboundSubmissionEngine`, and
   `BidirectionalSyncCycleRunner` only through their existing contracts. Manual
   sync, network availability, credential changes, and observation commits all
   enter through Prompt 92/93 control or notifier surfaces. No controller or
   platform adapter calls Prompt 91 directly.
5. `SyncRuntime`, its handle, `SyncWakeNotifier`, the outbound intent producer,
   credential controller, and each observer are wired to one runtime identity.
   Statuses/events remain bounded and non-sensitive; no token, cookie,
   credential bytes, content, or unnecessary absolute path crosses the host
   control surface.
6. `DesktopLifecycleAdapter` and `DesktopNetworkAdapter` are narrow,
   platform-neutral input seams. Linux and Windows aliases share identical
   lifecycle and scheduling semantics. They deliver shutdown or
   network-available hints; they do not implement a network monitor, retry
   policy, conflict policy, checkpoint write, service manager, or sync loop.
7. Startup is two-stage: validate/open state and profiles, compose the graph,
   register libraries, then `start()` the one runtime and start observers. A
   startup failure rolls back process-owned runtime/observer resources without
   deleting durable sync state. A stopped host is terminal; process restart
   constructs a new host and reuses existing SQLite/server durability.
8. Shutdown is explicit and graceful: reject new lifecycle work at the stop
   boundary, cancel observer poll tasks, flush/mark observer reconciliation,
   request Prompt 92 shutdown, allow active bounded Prompt 91 calls to finish,
   join runtime tasks, and close state owned by `open`/`from_platform`.
   Repeated shutdown/join is idempotent. `Drop` only makes best-effort
   cancellation; application code must call `shutdown`/`join` before process
   exit.

Prompts 91–93 remain the only owners of synchronization correctness,
scheduling decisions, durable signal ordering, candidate/handoff/conflict
state, idempotency, and content transfer. Prompt 94 adds no server route,
OpenAPI operation, migration, client migration, frontend surface, deployment
unit, autostart, tray, or UI behavior.

## Consequences / Hệ quả

- The composition graph has one observable owner and one bounded runtime, so a
  filesystem observer, credential controller, manual controller, and network
  adapter cannot silently schedule through separate supervisors.
- Durable work remains correct across a missed wake, shutdown race, crash, or
  process restart because the host does not make ephemeral lifecycle state part
  of the synchronization contract.
- The repository can test lifecycle and adapter semantics without a desktop
  executable or OS signal injection. Real Linux/Windows entrypoint wiring,
  platform network callbacks, installers, services, autostart, and GUI remain
  later work.
- The host has a small amount of ephemeral reference state and a per-observer
  polling task. These are bounded, explicitly joined, and never represented as
  durable scheduler rows.

## Alternatives rejected / Phương án loại bỏ

- One `SyncRuntime` per library: rejected because it duplicates process
  scheduling state and weakens fairness/ownership. Prompt 92 already provides
  the multi-library supervisor.
- Letting the host or controller call inbound/outbound engines directly:
  rejected because it bypasses Prompt 91 ordering and Prompt 93 durable-before-
  wake semantics.
- Reconstructing a host from SQLite alone: rejected because a durable row does
  not safely identify the approved filesystem root, secure provider, profile,
  remote, or process-owned executor.
- Adding a Linux/Windows connectivity monitor, service, daemon, autostart, or
  UI: rejected as product/platform scope outside this composition decision.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

No migration is required. The decision is reviewed when a real desktop/client
entrypoint is introduced, when multiple accounts are supported in one process,
when an HTTP transport can refresh credentials dynamically, or when a platform
service/autostart policy is approved. Any such change must preserve one runtime
per supported account context, explicit registration before observation, the
durable-before-wake ordering, graceful Prompt 91 completion, and Linux/Windows
semantic equivalence.

# Backup, snapshots, retention, and restore

Status: **durable backup scheduling, occurrence identity, exactly-once
scheduled-maintenance handoff, deterministic manual scheduler tick,
bounded restart-safe misfire policy, fenced scheduled-maintenance worker
step, manually invoked bounded scheduler+worker cycle, service
integration boundary, canonical lock ordering, internal one-shot
runtime, and external systemd oneshot+timer lifecycle IMPLEMENTED/VALIDATED;
snapshot, capture, restore, and automatic execution remain PLANNED
normative blueprint**

This document specifies device-to-Synveil backup, immutable backup snapshots,
retention, recovery workflows, and instance disaster recovery. It follows
ADR-007 and the entities in [DOMAIN_MODEL.md](DOMAIN_MODEL.md). The object
lifecycle is defined in [STORAGE.md](STORAGE.md); resumable byte transfer in
[UPLOADS.md](UPLOADS.md); live current-state synchronization in
[SYNC.md](SYNC.md).

The Prompt 61 scheduling foundation, Prompt 62 durable occurrence ledger,
Prompt 63 occurrence-to-maintenance handoff, Prompt 64 deterministic manual
scheduler tick, Prompt 65 bounded missed-occurrence policy, and Prompt 66
fenced scheduled-maintenance worker step are implemented,
but no snapshot, restore, or automatic backup
capability is `IMPLEMENTED` merely because this blueprint exists. A materialized
occurrence means Synveil durably recognized a due firing identity; a handoff
binds that identity exactly once to a canonical maintenance run. Neither fact
means an automatic backup ran. Synveil can manually claim and execute one fenced
scheduled-maintenance transition at a time; scheduled backups do not run
continuously in the background. Prompt 67 adds one manually invoked bounded
cycle composing exactly one scheduler tick followed by at most one worker
transition per invocation; it adds no daemon, polling loop, retry system, or
background execution.

## Two different backup responsibilities

Synveil uses the word backup for two related but distinct responsibilities:

1. **User/device backup:** a `BackupSet` captures selected client sources into
   immutable `BackupSnapshot` manifests and restores individual or complete
   content after deletion, corruption, or device loss.
2. **Synveil instance disaster recovery:** the operator backs up PostgreSQL,
   object storage, configuration, and required secrets together so the server
   itself can be rebuilt.

Device snapshots stored only on the same sole disk as live Synveil data protect
history from sync deletion but do not protect against that disk's loss. The UI
and operations guide distinguish historical retention from an independent
failure-domain copy.

## Backup is not synchronization

| Synchronization | Backup |
|---|---|
| Converges one current library namespace across devices. | Preserves immutable historical manifest views. |
| Delete becomes Trash/tombstone and propagates. | Source absence changes only a newly captured manifest. |
| Uses `Node`, `FileVersion`, `ChangeEvent`, and `SyncCursor`. | Uses `BackupSet`, `BackupSnapshot`, `BackupEntry`, and restore operations. |
| Resolves concurrent current-state edits/conflicts. | Captures what a device observed, with a declared consistency class. |
| Journal retention enables incremental convergence. | Snapshot retention determines recoverability. |

A backup client never calls the live delete endpoint to represent a missing
source file. Retention never emits a live sync tombstone. Restoring into a
library intentionally creates new live nodes/versions and then uses the normal
sync journal.

## Durable schedule, occurrence, and handoff foundation (Prompts 61–63)

One `BackupSet` may have one owner-scoped `BackupSchedule`. Its stable schedule
ID points to exactly one current immutable `BackupScheduleRevision`, while
older revisions remain readable. A revision stores only normalized logical
intent: `DAILY` or `WEEKLY`, an explicit IANA timezone, a local `HH:MM` minute,
(for weekly recurrence) one or more Monday-through-Sunday weekdays,
`REPLAY_ONE_BY_ONE` or `LATEST_ONLY`, and bounded `max_lateness_seconds`.
`LATEST_ONLY` with 604800 seconds (seven days) is the safe default; valid
lateness is 60 through 2678400 seconds (31 days).

Configuration writes are transactionally serialized by the owning BackupSet.
They are idempotent through a bounded operation identity and a versioned
canonical semantic fingerprint: a same-semantic request is a no-op that
returns the current revision, and reusing an operation identity for different
semantics fails closed. Disabling a schedule preserves current and historical
configuration and produces no new occurrence. Effective planning and new
materialization require both the schedule to be enabled and the BackupSet to
be `ACTIVE`.

The pure planner takes an exclusive UTC reference and returns the next local
recurrence strictly after it. It uses the stored IANA rules, advances a local
time in a DST gap to the first valid minute on that date, and chooses the
earlier absolute instant for a DST overlap.

`BackupSchedule.effective_from` is the strict activation boundary for the
current enabled configuration. First configuration, semantic revision edits,
and re-enables move it; semantic no-ops do not. A due candidate can be newly
materialized only when its canonical UTC instant is strictly after that
boundary, preventing backfill from disabled intervals and edits to an earlier
same-day time.

`BackupScheduleOccurrence` is an immutable owner-scoped ledger row for one
logical key `(schedule_revision_id, local_calendar_date)`. It records the
actual resolved local minute and canonical UTC instant recomputed from the
immutable revision; callers cannot submit those values. A second database
fence prevents two revisions of one schedule from creating two rows for the
same exact UTC instant. Concurrent requests converge to one opaque UUIDv7
occurrence ID. A retry after a lost response returns the existing row even if
the schedule was later edited or disabled; without an existing row, a
historical revision cannot newly materialize.

Materialized is not executed. Prompt 63 adds a separate immutable handoff
relation: an already-materialized occurrence can be bound once to the
existing Prompt 49 `BackupMaintenanceRun`, which starts in `CREATED` and binds
the same retention-policy revision and child operation identities as a manual
run. Existing handoff replay is authoritative after later schedule edits or
disablement; a new handoff requires an enabled schedule and an `ACTIVE`
BackupSet. The handoff itself does not capture a snapshot, create an expiry
plan, advance the run, or emit journal/sync/GC/prune evidence.

This remains a manually invoked control-plane capability. There is no claim or
execution state on the occurrence, job, lease, retry, worker, poll
loop, cron, systemd, Task Scheduler, automatic snapshot execution, HTTP route,
or UI behavior. A future prompt must add any bounded executor separately.

### Exactly-once scheduled maintenance handoff (Prompt 63)

`handoff_backup_schedule_occurrence(owner, occurrence_id, observed_at_utc)`
accepts only an existing durable `BackupScheduleOccurrence`; it never accepts
a revision/date pair as a substitute for materialization. One PostgreSQL
transaction locks `BackupSet`, `BackupSchedule`, and `Occurrence` in that
order, checks an existing handoff before new-effectivity fences, invokes the
canonical Prompt 49 maintenance-run creation primitive, and inserts the
immutable relation atomically. The database enforces one handoff per
occurrence and one scheduled occurrence per maintenance run, including owner,
BackupSet, and schedule scope.

The typed result is `CREATED`, `EXISTING`, or `NOT_EFFECTIVE` with bounded
schedule-disabled or BackupSet-inactive reasons. A lost response or concurrent
retry returns the same maintenance run and the same durable child operation
IDs. A committed handoff is provenance/control metadata, not a completed
backup: progress remains the canonical maintenance state machine and only an
explicit later call may advance it.

### Manual single-step scheduler tick and misfire policy (Prompts 64–65)

`BackupSchedulerService::run_scheduler_tick(observed_at_utc)` is one explicit
server-side orchestration attempt. The caller supplies the UTC observation
instant; the tick does not read an uncontrolled wall clock, sleep, poll, or
repeat internally. It performs at most one policy action: one expired-prefix
skip, or one materialization/handoff and at most one new
`BackupMaintenanceRun`.

Discovery is state-derived and restart-safe. In the current activation epoch
`(schedule_id, revision_id, effective_from)`, the resolution reference is the
maximum of `effective_from`, the latest successfully handed-off occurrence,
and the latest immutable misfire skip boundary. Materialization alone is not
progress. Each schedule derives at most one action; actions are compared
globally by the selected firing time or skip `resolved_through_utc`, then stable
schedule and revision IDs. No cursor, checkpoint, lease, or process-local
scheduler state is required.

The cutoff is `observed_at_utc - max_lateness_seconds`. An occurrence exactly
at the cutoff remains eligible; only an occurrence strictly before it is
expired for automatic execution. `REPLAY_ONE_BY_ONE` first writes one skip
range through an expired prefix, then on later ticks chooses the oldest
eligible occurrence. `LATEST_ONLY` chooses the newest eligible occurrence and
lets its successful handoff collapse all earlier unresolved due dates without
materializing them. If no eligible occurrence exists, it writes one immutable
skip range through the newest expired canonical occurrence. A skip creates no
occurrence, handoff, maintenance run, or invented skipped-count value.

The selected path always composes the canonical services:

1. a new candidate is materialized through Prompt 62, which recomputes the
   canonical local/UTC occurrence and applies the current revision and
   `effective_from` fences;
2. the durable occurrence is handed off through Prompt 63 and the Prompt 49
   maintenance-run creator; and
3. the tick returns only the logical occurrence, schedule, revision,
   `BackupSet`, scheduled instant, and maintenance-run references.

The scheduler path applies an additional atomic current-revision/activation
epoch check before a new handoff. A schedule edit, disable/re-enable, or
`BackupSet` disable therefore cannot make the tick silently execute superseded
or disabled-period work. A materialized occurrence is never deleted when a
handoff is ineffective or fails. A crash after materialization is recovered by
the next tick's materialized-unhanded priority; a committed handoff replays
without creating another run.

The result is `IDLE`, `SKIPPED_EXPIRED`, `HANDED_OFF_EXISTING`, or
`MATERIALIZED_AND_HANDED_OFF`. `IDLE` is normal when no eligible due candidate
exists. A successful tick leaves the maintenance run in `CREATED` and does not
advance it, capture a snapshot, plan or execute expiry, prune or release pins,
perform GC, append journal or sync state, or perform ObjectStore I/O. This is
not a daemon, retry/backoff mechanism, worker lease, HTTP API, or web
scheduling UI. The missed-occurrence policy is explicit, but no background
component invokes it continuously.

### Fenced scheduled-maintenance worker step (Prompt 66)

`ScheduledMaintenanceWorkerService` is a manually invoked, test-driven worker
primitive. One explicit invocation claims or reconciles at most one eligible
scheduled step and executes or reconciles at most one canonical Prompt 49
maintenance transition: `CREATED → SNAPSHOT_CAPTURED`,
`SNAPSHOT_CAPTURED → EXPIRY_PLANNED`, or `EXPIRY_PLANNED → COMPLETED`. It never
runs `CREATED → COMPLETED` in one call, never loops, polls, sleeps, spawns,
renews a lease, or retries with backoff.

Discovery applies only to maintenance runs with a durable Prompt 63 handoff;
manual Prompt 49 runs are never claimed. A committed handoff is execution
authority: the worker never rechecks the current schedule revision, misfire
policy, or enabled state to revoke handed-off work, so an already-authorized
run still advances after a schedule disable or policy edit. A Prompt 65
`SKIPPED_EXPIRED` prefix creates no run and therefore no claim.

Each claim in `backup_scheduled_maintenance_claims` authorizes exactly one
transition from one expected state. Claim identity is
`(maintenance_run_id, expected_state)` with database uniqueness; the
predetermined resulting state cannot be rebound (`CREATED` always completes to
`SNAPSHOT_CAPTURED`, and so on). Every lease carries an opaque worker ID, an
unpredictable lease token, a monotonically increasing `lease_generation`
starting at 1, and a bounded duration (default 120 seconds, minimum 10,
maximum 900; out-of-range durations are rejected, never clamped). A second
worker takes over only at or after `lease_expires_at`, atomically rotating the
token and incrementing the generation; an early steal is rejected by both the
service and a database trigger. The same holder replaying a valid lease
receives the canonical open claim without a new row.

Safety rests on fencing, not on workers promising to stop. Every
maintenance-state commit re-verifies the active
`(worker, token, generation, expected state, unexpired lease)` fence inside
the same authoritative transaction, so a stale generation commits zero
transitions even when its canonical child work (snapshot capture, expiry
planning, expiry execution, all replayed through the run's durable
Prompt 49 child-operation identities) already exists. Child logic is never
duplicated: the worker calls the canonical `BackupService` capture, expiry
planning, and expiry execution operations, then seals the fenced run-state and
claim-receipt commits. Claim acquisition alone performs zero snapshot, expiry,
prune, GC, ObjectStore, journal, or sync work.

Crash recovery is durable because all worker state lives in PostgreSQL.
Claim-before-advance crashes are taken over after expiry with the same claim
identity. Advance-before-receipt crashes are reconciled: the next holder
detects that the run already equals the claim's resulting state, seals the
receipt as `RECOVERED_COMPLETION` without advancing again, and stops — a
recovery never runs the next transition in the same invocation. A lost
completion response replays the canonical completed receipt. An unexpected run
state fails closed with a typed inconsistency, and a `STALE` run reports a
typed stale outcome instead of a normal receipt. Twelve concurrent claimants
converge to one claim row and one lease holder; twelve concurrent executors
commit exactly one transition with one canonical child operation.

Global discovery order is `occurrence.scheduled_for_utc`, then `schedule_id`,
then `maintenance_run_id`: oldest scheduled execution first. An unexpired
foreign lease skips that run without blocking newer claimable work; an expired
oldest lease is taken over first; an oldest incomplete claim whose run already
reached its resulting state is reconciled first. Completed receipts are
immutable, history is never deleted, and claims create no public operation-feed
entries: Prompt 55 still exposes exactly one `MAINTENANCE` operation per run.
There are no worker HTTP routes, OpenAPI changes, web UI changes,
`client-sync` changes, or physical storage identities in the claim types.

### Manually invoked bounded scheduler + worker cycle (Prompt 67)

`ScheduledMaintenanceCycleService::run_scheduled_maintenance_cycle(worker_id,
observed_at_utc, lease_duration_seconds)` composes exactly one canonical
`run_scheduler_tick` followed by exactly one canonical
`run_scheduled_maintenance_worker_step`, using the same injected
`observed_at_utc` where semantics permit and reading no wall clock itself.
Tick-before-worker ordering lets a newly handed-off run become eligible for
the same invocation's single worker transition, while global worker ordering
still applies: newly created work is never privileged over older eligible
work. The combined result keeps the tick outcome (`Idle`, `SkippedExpired`,
`HandedOffExisting`, `MaterializedAndHandedOff`) and the worker outcome
(`Idle` or `Stepped`) typed and separate. A scheduler error returns without
executing the worker step; a worker error preserves already-committed
scheduler work, with no giant transaction spanning both phases. Prompt 63
handoff remains execution authority and Prompt 65 policy stays resolved at
fire time. One cycle advances at most one maintenance transition
(`CREATED → SNAPSHOT_CAPTURED`, `SNAPSHOT_CAPTURED → EXPIRY_PLANNED`, or
`EXPIRY_PLANNED → COMPLETED`), so full maintenance needs three distinct
manual invocations. There is no daemon, polling loop, sleep, timer, heartbeat,
lease renewal, retry/backoff loop, API endpoint, UI, SSE, or background task;
the caller invokes the cycle manually and the database remains the durable
source of truth. No new migration or cycle table was introduced.

### Service integration boundary (Prompt 68)

`ScheduledMaintenanceCycleRunner` establishes the application-level service
integration boundary for manually invoking one bounded scheduled-maintenance
cycle. It wraps the canonical Prompt 67 `ScheduledMaintenanceCycleService`
and provides a narrow, production-oriented caller that future explicitly
authorized lifecycle callers can invoke without constructing metadata,
storage, and service dependencies ad hoc.

The runner holds a process-scoped `BackupScheduledMaintenanceWorkerId` that
is created once at construction and reused for all subsequent invocations.
The identity is not persisted to a dedicated table and is not exposed as
business/user-visible identity.

`run_one_scheduled_backup_maintenance_cycle(observed_at_utc,
lease_duration_seconds)` validates the lease duration against canonical
bounds (10–900 seconds) without silent clamping, then delegates to the
canonical Prompt 67 cycle. The caller supplies the observation timestamp;
no hidden `Utc::now()` appears in the orchestration layer.

The runner is composed into `ApiState` through the existing builder pattern:
`ApiState::with_scheduled_maintenance_cycle_runner(runner)`. When PostgreSQL
is configured via `with_postgres_auth`, the runner is automatically
initialized with a fresh worker identity.

All Prompt 67 invariants remain: exactly one scheduler tick, exactly one
worker step (at most one maintenance transition), scheduler failure prevents
worker execution, worker failure preserves scheduler durable commits, and
the database remains the durable source of truth. The runner adds no daemon,
polling loop, sleep, timer, heartbeat, lease renewal, retry/backoff, API
endpoint, UI, SSE, or background task.

### Internal one-shot scheduled-maintenance runtime (Prompt 71)

`synveil-scheduled-maintenance-once` (`crates/api/src/bin/synveil-scheduled-maintenance-once.rs`)
is the internal, explicitly operator-triggered one-shot process boundary for
scheduled backup maintenance. It is not a daemon, not a polling loop, not a
timer, not a cron, and not an HTTP API.

**Process:** `load runtime config → connect DatabasePool → MigrationRunner
→ ScheduledMaintenanceCycleRunner::new(pool) → Timestamp::now_utc() once
→ run_one_scheduled_backup_maintenance_cycle(observed_at_utc, lease_seconds)
→ structured log → close pool → exit`.

One process execution invokes exactly one canonical `ScheduledMaintenanceCycleRunner`
cycle, which itself performs at most one scheduler tick plus at most one
worker step and at most one semantic `BackupMaintenanceRun` transition
(`CREATED → SNAPSHOT_CAPTURED → EXPIRY_PLANNED → COMPLETED`). The process then
exits; it never loops.

**Clock:** The runtime obtains `observed_at_utc` exactly once at the outer edge
via `Timestamp::now_utc()` and injects it; scheduler/runner internals never
read the wall clock. This preserves Prompt 67–68 explicit-time injection.

**Lease:** `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS` (optional, default 120)
is validated against canonical bounds 10..=900 without clamping. Out-of-range
or non-numeric values are rejected before any cycle begins with a clear
diagnostic and non-zero exit; no partial worker execution occurs.

**Database:** Reuses canonical `DatabaseConfig` (`DATABASE_URL`) and
`MigrationRunner`. No second bootstrap path, no credential logging, no secret
exposure.

**Result:** The typed `ScheduledMaintenanceCycleResult` is preserved internally
(tick: `Idle`/`SkippedExpired`/`HandedOffExisting`/`MaterializedAndHandedOff`;
worker: `Idle`/`Stepped`). Structured tracing logs `tick`, `worker`,
`is_idle`, `observed_at_utc`, `lease_seconds` without leaking physical
identities (`ObjectId`, storage keys, etc.). Worker ID remains internal.

**Exit codes:** `0` for any successful cycle including `Idle` and
`SkippedExpired` (both canonical scheduler successes); non-zero for invalid
config, database/migration failure, scheduler failure (worker not executed),
or worker failure (including `LeaseLost`; scheduler writes remain committed).
Idle is not an error.

**Boundedness:** The production runtime contains no `loop`, `while`, `for`
repetition, `tokio::interval`, `sleep`, `spawn` for cycle execution,
`heartbeat`, `renewal`, `retry`, `backoff`, `cron`, `timer`, `run_forever`,
or `leader election` around the cycle. One invocation never performs
`CREATED → COMPLETED` in one call; four separate explicit invocations are
required to drain a run.

**Failure:** No automatic retry, no sleep-then-retry, no deadlock retry.
A bounded failure is logged, resources are closed, and the process exits
non-zero. A future external service (e.g., `systemd` timer) may invoke the
process again; recovery relies on Prompt 66 durable fencing and the database
as source of truth, not in-memory state. Manual Prompt 49 runs remain
invisible to the scheduled worker path; a committed handoff remains
execution authority after schedule disable or policy edit.

**Invocation:**

```sh
DATABASE_URL=postgresql://postgres@127.0.0.1:5432/synveil \
  cargo run -p synveil-api --bin synveil-scheduled-maintenance-once

# with explicit lease (optional)
SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS=120 \
  DATABASE_URL=... cargo run -p synveil-api --bin synveil-scheduled-maintenance-once
```

Suitable for manual operator invocation or future `systemd` timer/service
that invokes the process again externally. This binary is internal and
operator-facing only: no HTTP route, OpenAPI, SSE, WebSocket, frontend,
or client SDK changes.

### External lifecycle and systemd cadence (Prompt 72)

Prompt 72 wires the Prompt 71 one-shot runtime into an operating-system
service-manager model **without turning Synveil's runtime itself into a
daemon**. The architecture remains:

```text
systemd timer (recurrence)
      ↓
systemd oneshot service (Type=oneshot)
      ↓
synveil-scheduled-maintenance-once (one bounded cycle)
      ↓
exactly one ScheduledMaintenanceCycleRunner::run_one_scheduled_backup_maintenance_cycle
      ↓
structured result → pool.close() → exit
```

The timer owns recurrence; Synveil owns one bounded execution. A subsequent
timer activation is a new externally scheduled bounded attempt, not an
internal retry or lease renewal.

**Repository locations:**

- `deploy/systemd/synveil-scheduled-maintenance.service` — source unit
- `deploy/systemd/synveil-scheduled-maintenance.timer` — source unit
- Packaged installation (when produced): `/usr/lib/systemd/system/` or
  distribution-equivalent; repository source must not be copied directly to
  `/etc/systemd/system` during tests.

**Service unit (`synveil-scheduled-maintenance.service`):**

- `Type=oneshot`, `ExecStart=/usr/bin/synveil-scheduled-maintenance-once` —
  the canonical Prompt 71 binary, not shell-wrapped, no second maintenance
  executable.
- `EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env` —
  repository-standard file mechanism for optional non-secret
  `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS` tuning (default 120, bounded
  10..=900). The required `DATABASE_URL` is delivered through Prompt 77
  `LoadCredential`, not this file; no credentials are hardcoded in the unit.
- `Restart=no` — systemd records a non-zero exit; that invocation ends.
  Tight automatic restart loops are forbidden (`Restart=always` is not used).
  A later timer firing is a new lifecycle invocation.
- `TimeoutStartSec=300` — conservative 5-minute bound, deliberately longer
  than the default `TimeoutStartSec` (90 s) and the default lease (120 s)
  but below the maximum lease (900 s) and safely longer than the expected
  bounded cycle (snapshot capture / expiry planning / expiry execution each
  complete well under a minute in normal operation). Abrupt termination
  remains recoverable through Prompt 66 durable claim/lease fencing; the
  value may be raised per-site if measured bounded execution requires it,
  or the deferred-decision default may be used with documentation.
- Service account (Prompt 75): the one-shot executes as the persistent
  system identity `User=synveil` `Group=synveil` (auto-allocated UID/GID via
  `deploy/sysusers.d/synveil.conf`, shell `/usr/sbin/nologin`, home
  `/var/lib/synveil`, no supplementary groups). It requires only PostgreSQL
  connectivity via TCP or `AF_UNIX`; it MUST NOT run as `root`. Packaging
  creates the account with `systemd-sysusers` at install time — the one-shot
  binary itself never creates users, never needs `CAP_*`, and never requires
  UID 0 for its database-only path. `RuntimeDirectory=synveil` (0750) owns
  `/run/synveil` per-activation; see `DEPLOYMENT.md` for the full
  `DynamicUser=no` rationale, UID/GID policy, and filesystem ownership
  contract.
- Hardening (Prompt 78, evidence-driven, LOCKED Gen-1): `NoNewPrivileges=yes`,
  `RestrictSUIDSGID=yes`, empty `CapabilityBoundingSet=`/`AmbientCapabilities=`
  (zero Linux capabilities), `ProtectSystem=strict` with zero writable
  exceptions, `ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes` +
  `DevicePolicy=closed`, `InaccessiblePaths=/etc/synveil/credentials`,
  `ProtectKernelTunables=yes`, `ProtectKernelModules=yes`,
  `ProtectKernelLogs=yes`, `ProtectControlGroups=yes`,
  `ProtectProc=invisible` + `ProcSubset=pid`, `RestrictNamespaces=yes`,
  `RestrictRealtime=yes`, `LockPersonality=yes`,
  `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6` (Unix socket + TCP, no
  `PrivateNetwork`), `SystemCallArchitectures=native`,
  `MemoryDenyWriteExecute=yes`, `SystemCallFilter=@system-service` minus
  dangerous classes (`@mount`, `@raw-io`, `@reboot`, `@swap`, `@module`,
  `@debug`, `@privileged`, `@cpu-emulation`, `@obsolete`, `@resources`),
  `UMask=0077`, `WorkingDirectory=/`. Proven compatible by executing the real
  one-shot binary under the identical sandbox against PostgreSQL 17 (idle,
  due-work, and existing-work gates, all exit 0) plus negative probes; see
  `DEPLOYMENT.md` § “Linux systemd sandbox & runtime privilege hardening
  (Prompt 78)”, ADR-026, and
  `crates/metadata/tests/linux_sandbox_hardening_units.rs`.
  `systemd-analyze security` exposure improved 4.5 → 1.4 OK on systemd 261.2 (informational;
  functional correctness is authoritative).
- Logging to the journal (`StandardOutput=journal`,
  `StandardError=journal`); structured fields `tick`, `worker`, `is_idle`,
  `observed_at_utc`, `lease_seconds` are logged without leaking physical
  identities.

**Timer unit (`synveil-scheduled-maintenance.timer`):**

- Conservative default cadence: **approximately once per minute**
  (`OnCalendar=*:*:00`, `AccuracySec=1s`). One activation performs at most
  one scheduler tick + one worker step + one semantic transition
  (`CREATED → SNAPSHOT_CAPTURED → EXPIRY_PLANNED → COMPLETED`), so a full
  run drains in three separate activations (~3 min). The interval was chosen
  after inspecting `DEFAULT_BACKUP_SCHEDULE_MAX_LATENESS_SECONDS=604800`
  (7 days, minimum 60 s) and the one-transition-per-cycle bound: a 60-second
  poll detects a due occurrence within at most one minimum lateness window
  without tight looping, while remaining independent of both the lease
  duration and user backup schedule recurrence.
- `Persistent=true` — if the host was off during one or more activations,
  systemd invokes the service **once** after boot; the scheduler's durable
  misfire policy (`LATEST_ONLY` / `REPLAY_ONE_BY_ONE` / `SKIPPED_EXPIRED`)
  resolves backlog, not systemd replay. No hundreds of invocations are
  synthesized after extended downtime.
- `RandomizedDelaySec=10s` — bounded systemd-native jitter to avoid
  fleet-wide thundering herds at the same wall-clock second. 10 s is small
  relative to the 60-second minimum lateness and the 604800-second default,
  so correctness is preserved (due evaluation uses durable timestamps, not
  arrival second). No `sleep`/`random` was added inside the Rust binary.
- `WantedBy=timers.target`; `Unit=synveil-scheduled-maintenance.service`.

**Cadence vs. backup recurrence:**

The timer interval is **not** backup schedule frequency. Users configure
`BackupSchedule` recurrence (`DAILY`/`WEEKLY`, local `HH:MM`, `LATEST_ONLY`
/ `REPLAY_ONE_BY_ONE`, bounded lateness) independently. The timer merely
asks “is there bounded scheduled work to perform now?” every minute.

**Overlap, startup, failure, and lease relationship:**

- Overlap: two activations may overlap (previous still running, manual
  `systemctl start` plus timer firing, multiple callers). Correctness
  continues to rely on Prompt 66 durable claims / lease generation /
  fencing and Prompt 69 canonical lock ordering. No process-local `Mutex`,
  advisory lock, or `LOCK TABLE` is used. Systemd's natural same-unit
  activation serialization is **not** trusted for DB correctness; 12
  concurrent independent processes still converge with `SQLSTATE 40P01=0`.
- Startup: `timer` is enabled; `Persistent` evaluates missed activation;
  one bounded cycle runs when appropriate; normal cadence resumes. API
  server startup does **not** synchronously block on scheduled maintenance
  and does **not** invoke the cycle from `ApiState` construction.
- Failure: non-zero exit is recorded by systemd; that invocation ends; no
  internal retry/backoff/heartbeat/renewal. The next timer firing is a
  fresh attempt.
- Lease: default 120 s, 10..=900 s — independent of the 60 s timer
  interval; the timer is not derived from lease expiry.
- Long invocation: a healthy bounded cycle that exceeds one timer interval
  continues to completion; no in-process second cycle starts; the next
  lifecycle behavior follows service-manager semantics and durable fencing.
- API/UI: `HTTP route additions=0`, `OpenAPI=0`, `SSE=0`, `WebSocket=0`,
  frontend/mobile changes=0. Linux systemd integration is deployment
  infrastructure and stays outside `crates/core` and portable domain APIs;
  no `systemctl` invocation appears in business logic.

**Operator enablement (once packaged):**

```sh
# install units to /usr/lib/systemd/system (packaging does this)
# Prompt 77: secret delivery uses systemd LoadCredential=, not EnvironmentFile.
# The DATABASE_URL is NEVER in the EnvironmentFile in production.
# 1. (optional) non-secret runtime tuning
sudo install -m 0640 -o root -g synveil /dev/stdin /etc/synveil/synveil-scheduled-maintenance.env <<'EOF'
# non-secret lease tuning only; no DATABASE_URL
SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS=120
EOF
# 2. database credential via systemd-credentials (admin source, root-only)
sudo install -d -m 0700 -o root -g root /etc/synveil/credentials
sudo install -m 0600 -o root -g root /dev/stdin /etc/synveil/credentials/database-url <<'EOF'
postgresql://synveil:***@127.0.0.1:5432/synveil
EOF
sudo systemctl daemon-reload
sudo systemctl enable --now synveil-scheduled-maintenance.timer
systemctl status synveil-scheduled-maintenance.timer
journalctl -u synveil-scheduled-maintenance.service -f

# ad-hoc one-shot (without timer)
sudo systemctl start synveil-scheduled-maintenance.service
# or directly, using the dev fallback (no credential file configured)
DATABASE_URL=postgresql://dev@127.0.0.1:5432/synveil /usr/bin/synveil-scheduled-maintenance-once
# or with explicit credential file (no DATABASE_URL):
SYNVEIL_DATABASE_CREDENTIAL_FILE=/etc/synveil/credentials/database-url \
  /usr/bin/synveil-scheduled-maintenance-once

# disable
sudo systemctl disable --now synveil-scheduled-maintenance.timer
```

Tests validate unit syntax via `systemd-analyze verify` on temporary copies
(no host `/etc/systemd/system` mutation), static assertions for `Type`,
`ExecStart`, `User=synveil`/`Group=synveil`, `RuntimeDirectory`,
`NoNewPrivileges`/`ProtectSystem`, cadence, `Persistent`,
`RandomizedDelaySec`, `Restart`, `EnvironmentFile`, install targets, plus
`systemd-sysusers --dry-run` and `systemd-tmpfiles --dry-run` without mutating
`/etc/passwd`/`/etc/group`/`/var/lib/synveil`, and live PostgreSQL probes for
repeated activations, misfire policies after downtime, concurrency (12
callers), and restart recovery — all `40P01=0` without retry. Rootless
business execution has no UID 0 dependency. No GUI control, no cross-platform
service manager, no heartbeat, no daemon, no leader election, no
retry/backoff is claimed.

### Linux service identity and filesystem ownership foundation (Prompt 75)

Prompt 75 replaces the intentional Prompt 72 `User=` deferral with a locked
Gen-1 identity/ownership contract.

**Decision (LOCKED):** persistent system account `synveil` / group `synveil`
via `systemd-sysusers` (`deploy/sysusers.d/synveil.conf`), executed as
`User=synveil` `Group=synveil` in `synveil-scheduled-maintenance.service`
(and future first-party Synveil services). See `DEPLOYMENT.md`.

**Alternatives considered and rejected:**

| Alternative | Reason rejected |
|---|---|
| `root` runtime | Violates least privilege; service never needs to modify its own binary, units, or host. Package install remains `root`, runtime is unprivileged. |
| `DynamicUser=yes` | **Rejected** — transient UID cannot provide stable ownership of `/var/lib/synveil`, `/etc/synveil` read access, or future object-store/data paths; hides state under `/var/lib/private`/`/run/private` via id-mapped mounts, breaks admin-visible ownership, and prevents multiple services sharing one identity. Suitable only for stateless ephemeral services, not Synveil. |
| Per-service accounts (`synveil-api`, `synveil-maintenance`, `synveil-gc`) | Deferred — today all first-party services share a trusted backend/data boundary; separate identities would not materially improve least privilege. Documented as future option if capability separation emerges. |
| Interactive user account | Rejected — no login shell, no home `/home/*`, no `sudo`/`wheel`/`docker`/`disk`/`adm`/`root` groups, no browsing of host. |

**Account properties:** system account, auto-allocated UID/GID (`-` in
sysusers, no hardcoded numeric value), `GECOS="Synveil service account"`,
home `/var/lib/synveil` (stable state location, not interactive), shell
`/usr/sbin/nologin` (or default `nologin` equivalent), fully locked
(invalid password). No supplementary groups. Stable across reboots; created
by packaging (`systemd-sysusers`), never by application runtime; not persisted
to a `service_users` table — Linux identity is deployment identity, not
domain identity.

**UID/GID policy:** allow `sysusers`/packaging to allocate a system UID/GID.
No fixed numeric UID is assumed cross-machine. Future appliance images may
reserve a fixed identity via image construction if required; Gen-1 explicitly
documents the non-assumption.

**Filesystem ownership contract (authoritative — see also `deploy/README.md`):**

| Path | Ownership | Mode | Manager | Runtime access |
|---|---|---|---|---|
| `/usr/bin/synveil-scheduled-maintenance-once` (and future `synveil-*`) | `root:root` | `0755` | package (root) | `synveil` reads/execs, **cannot write** own binary |
| `/usr/lib/systemd/system/synveil-*.service`, `*.timer` | `root:root` | `0644` | package | not writable by `synveil` |
| `/usr/lib/sysusers.d/synveil.conf`, `/usr/lib/tmpfiles.d/synveil.conf` | `root:root` | `0644` | package | not writable |
| `/etc/synveil` | `root:synveil` | `0750` | package | `synveil` reads required config, traverses; cannot freely rewrite admin config |
| `/etc/synveil/synveil-scheduled-maintenance.env` | `root:synveil` | `0640` | admin/package | `synveil` reads optional non-secret lease tuning via `EnvironmentFile`; `DATABASE_URL` is delivered via Prompt 77 `LoadCredential` |
| `/var/lib/synveil` | `synveil:synveil` | `0750` | `tmpfiles.d` (`d` line) | persistent non-secret runtime state; empty today is acceptable, not in `/usr`/`/etc`/home |
| `/run/synveil` | `synveil:synveil` | `0750` | `RuntimeDirectory=synveil` | ephemeral per-activation, lifecycle-tied cleanup |
| `/var/log/synveil` | — | — | — | **not created** — journald is authoritative |

Object-store / user storage roots follow the storage architecture; Prompt 75
does **not** recursively `chown` arbitrary user pools to `synveil`.

**Creation mechanisms:**

- `deploy/sysusers.d/synveil.conf` — `systemd-sysusers` declarative source
  (installed to `/usr/lib/sysusers.d/`). Validated with `systemd-sysusers
  --dry-run --root=/tmp/root` and `systemd-sysusers --cat-config` in tests,
  never mutating the host account database.
- `deploy/tmpfiles.d/synveil.conf` — `d /var/lib/synveil 0750 synveil synveil`.
  Validated with `systemd-tmpfiles --dry-run --create --root=/tmp/root` or
  `--cat-config`; `/run/synveil` is **not** duplicated there.
- `RuntimeDirectory=synveil` in the service unit owns `/run/synveil`.
- `StateDirectory=` intentionally **not** used — `tmpfiles.d` remains the
  single authoritative host-level manager for `/var/lib/synveil` so that
  multiple future services can share it without per-service conflicting
  managers.

**Privilege boundary:** install/upgrade (`systemd-sysusers`, `systemd-tmpfiles
--create`, `install -m 0640` for non-secret env tuning, `install -m 0600` for the
root-owned credential source, `daemon-reload`, `systemctl
enable`) is `root`; one-shot runtime (`DynamicPool` → cycle) is `synveil` and
never creates users, `chown`s, modifies units, invokes `systemctl`, or
escalates. Database connectivity remains `DATABASE_URL`-configured (TCP or
`AF_UNIX`) — no `peer` authentication tied to the Unix username is assumed.

**Portability:** Linux username/UID/GID/systemd remain outside `crates/core`
and portable Synveil Server APIs. No domain type gains a Unix identity field.

### Canonical scheduled-maintenance lock-order invariant (Prompt 69)

All competing scheduled-maintenance transactions follow one canonical lock
hierarchy. This structurally removes the PostgreSQL `40P01` deadlock class
without retry, backoff, sleep, global mutex, advisory locks, table locks,
or reduced concurrency.

**Lock classes (top → bottom):**

1. `backup_sets` — BackupSet authority
2. `backup_schedules` + `backup_schedule_revisions` (revision read `FOR SHARE` where needed)
3. `backup_schedule_occurrences` — materialized firing identity
4. `backup_schedule_occurrence_handoffs` — exactly-once binding
5. `backup_schedule_misfire_skips` — expired-prefix progress (when applicable)
6. `backup_maintenance_runs` — canonical maintenance state
7. `backup_scheduled_maintenance_claims` — lease + fencing
8. canonical child-operation state — `backup_snapshots` / nodes / pins, `backup_snapshot_expiry_plans` / entries / executions (and their object locks when applicable)

**Invariant:** If transaction path A may acquire lock classes X and Y and
path B may also acquire X and Y, they acquire X and Y in the same relative
order. No path acquires `X → Y` while another acquires `Y → X`.

- Scheduler due discovery is lock-free; `resolve_misfire_skip` and
  `materialize` acquire `backup_sets → backup_schedules → (skips | occurrences)`.
- Handoff acquires `backup_sets → backup_schedules → occurrences → handoffs → maintenance_runs` (plus `FOR SHARE` policy where needed) before inserting the handoff.
- Worker claim acquisition (`claim_candidate_step`) first discovers parent
  identities without row locks, then acquires `backup_sets → backup_schedules → occurrences → handoffs → maintenance_runs → claims` in one short transaction, revalidates eligibility, and then inserts or takes over the claim.
- Lease takeover, recovery reconciliation, and fence verification all acquire
  `maintenance_runs → claims` (parent before child). The previous
  `claims → runs` order in `load_execution_plan` / `verify_fenced_run_state` /
  `fenced_mark_run_stale` was inverted and is now `runs → claims`.
- Fenced run-state transitions (`CREATED → SNAPSHOT_CAPTURED → EXPIRY_PLANNED → COMPLETED`) verify the fence inside the same `runs → claims` transaction and then `UPDATE` the run; child work (snapshot capture, expiry planning/execution) runs in its own transactions via the canonical `BackupService` identities and is not held across parent locks.
- When multiple rows of the same class are locked, deterministic ordering is used
  (`claim_id ASC`, `object.id ASC`, etc.). Business discovery ordering
  (`scheduled_for_utc, schedule_id, run_id`) is preserved for candidate
  selection; internal lock ordering uses a separate deterministic key only when
  needed and is documented separately.

**Discovery / mutation pattern:** discover candidate identity → start narrow
mutation transaction → acquire canonical locks → revalidate candidate →
mutate → commit. Stale discovery is never trusted.

**Lock lifetime:** parent scheduling locks are not held across expensive
canonical child operations unless required by an invariant. The atomic
correctness boundary (fenced `runs → claims` plus the final state `UPDATE`)
remains intact.

**Concurrency:** Independent maintenance runs touch different `BackupSet` /
`Schedule` / `Occurrence` / `Run` / `Claim` rows, so they do not serialize
on a global mutex or advisory lock. No automatic retry or `40P01` swallowing
is required for correctness; `SYNVEIL_BACKUP_SCHEDULED_MAINTENANCE_LOCK_ORDER_READY`
requires `SQLSTATE 40P01 = 0` under `12 × 25` concurrent-cycle contention.

## Backup invariants

1. Only a `COMMITTED` snapshot is listed as restorable. `BUILDING`,
   `VERIFYING`, `FAILED`, and `EXPIRED` are not complete recovery points.
2. Snapshot commit is one PostgreSQL transaction that makes an already staged,
   verified complete manifest authoritative. No partially submitted manifest
   can become restorable.
3. A committed snapshot is immutable. Correction creates another snapshot; it
   never edits historical entries or the root hash.
4. Every restorable file entry resolves to a `VERIFIED` object in the same
   owner/dedup domain, with canonical length and SHA-256.
5. Missing/unreadable/excluded source observations are explicit. They never
   delete entries from older retained snapshots.
6. Every initial snapshot is logically complete even when content objects are
   reused. A retained snapshot does not require its parent snapshot to remain.
7. Retention expires snapshot references first. Physical object GC occurs only
   after all live/version/Trash/snapshot/derivative/lease/hold references are
   absent and the storage safety window passes.
8. Restore is durable, idempotent, restartable, non-destructive by default, and
   verifies bytes. Overwrite, when explicitly selected, creates new file
   versions rather than rewriting historical objects.
9. The server never upgrades a client capture to a stronger consistency label
   than the client evidence supports.
10. Revoking/removing a source device stops future backup access; it does not
    silently erase retained snapshots.
11. Backup IDs, hashes, entry paths, and object IDs are not authorization.
12. Optional notifications, indexing, thumbnails, and anomaly detection cannot
    be prerequisites for snapshot commit or restore correctness.

## Backup set and source policy

A `BackupSet` belongs to one owner and source device. Its versioned policy
contains:

- client-stable opaque source IDs and user-facing source labels;
- selected roots, include/exclude rules, file-size/type policy, and schedule or
  continuous trigger;
- symlink/mount-boundary behavior and supported metadata profile;
- retention policy reference and quota domain;
- consistency requirements and whether a best-effort snapshot with explicit
  capture errors may commit;
- pause/retire state and last successful observation.

Source descriptors are untrusted device metadata. A string such as
`C:\Users\...` or `/home/...` is never interpreted as a server path and never
grants server filesystem access.

Initial recommended source behavior:

- do not follow symlinks during capture; record the link as bounded metadata if
  the client/platform profile supports it;
- do not cross a mount/volume boundary unless that source explicitly includes
  it;
- preserve original relative name components but also compute a portable
  comparison/safety projection for restore warnings;
- exclude sockets, device nodes, and other special files initially; report them
  explicitly rather than serializing unsafe semantics;
- treat hard-link preservation, ACLs, extended attributes, sparse extents, and
  platform-specific metadata as capability-versioned additions, not implicit
  promises.

Changing a policy affects later snapshots only. A snapshot stores the effective
policy revision used to capture it.

## Snapshot state machine

```mermaid
stateDiagram-v2
    [*] --> BUILDING: initiate snapshot
    BUILDING --> BUILDING: idempotent entry/content batches
    BUILDING --> VERIFYING: seal immutable manifest
    VERIFYING --> VERIFYING: lease retry/recovery
    VERIFYING --> COMMITTED: atomic manifest commit
    BUILDING --> FAILED: permanent invalid/incomplete input
    VERIFYING --> FAILED: integrity/policy verification fails
    BUILDING --> EXPIRED: abandoned build TTL
    COMMITTED --> EXPIRED: retention selection
```

`FAILED` and `EXPIRED` are not restorable. An expired committed snapshot may
retain entries temporarily while an idempotent purge job progresses, but the UI
must not present it as a recovery point after retention's audited point of no
return. A legal hold blocks `COMMITTED -> EXPIRED`.

### `BUILDING`

The snapshot has frozen set/device/parent/policy identity but accepts bounded,
idempotent manifest batches and backup-scoped content claims. Entries are not
authoritative restore references yet. An active snapshot build lease protects
its verified objects/staging from GC.

### `VERIFYING`

The manifest is sealed: no entry, error, parent, consistency claim, or content
binding can change. A generation-leased verifier validates topology,
completeness, all content receipts, canonical manifest serialization/root hash,
capture results, quotas, and policy. Retryable infrastructure errors keep this
state and schedule another attempt. Permanent mismatch becomes `FAILED`.

### `COMMITTED`

One transaction changes the frozen snapshot to `COMMITTED`, making all of its
entries authoritative references, finalizing accounting, storing idempotent
outcome, audit, and outbox/jobs. Queries for restorable entries always join a
committed snapshot state. The transaction does not update millions of entries;
their visibility changes through the single locked snapshot state after they
have already been verified and protected by the build lease.

The build lease is released only after commit. GC sees either that unexpired
lease before commit or the committed snapshot reference after commit, so there
is no unprotected interval.

## Manifest model

### Full logical snapshot

The initial implementation stores a complete logical manifest per snapshot.
An incremental parent is an ingest optimization and lineage hint, not a restore
dependency. Each new `BackupEntry` contains its own resulting metadata and
object reference. Unchanged files may share immutable objects, but expiring the
parent never makes a retained child snapshot incomplete.

Later structurally shared/delta manifests require a versioned format and a
retention proof showing every retained snapshot remains independently
resolvable. They are not an invisible schema optimization.

### Entry identity and topology

Each entry has a client-stable source entry identity where the platform can
provide one, plus a snapshot-local immutable entry ID and parent entry ID.
Relative path is represented as bounded components, not a trusted concatenated
server path.

Validation requires:

- exactly one manifest root for each declared source root;
- unique snapshot-local entry IDs and unique child comparison keys under a
  parent according to the manifest's name-policy version;
- no cycle, missing parent, child under non-directory, absolute path, `.`/`..`
  traversal component, NUL, excessive component length, excessive depth, or
  checked-arithmetic overflow;
- declared entry count and byte totals within set/user/instance bounds;
- type-specific fields only for registered types;
- every `PRESENT` file has exact length/hash and a verified object binding;
- capture errors and exclusions are classified and included in completeness
  summary rather than silently omitted.

Original names, timestamps, permissions, symlink targets, and local paths are
untrusted metadata. They can be shown to the owner but never drive a server
filesystem operation without safe restore validation.

### Canonical root hash

Every manifest format has an immutable version. The server computes a canonical
root hash from length-delimited, type-tagged entry fields and child hashes in a
defined byte order; JSON map order, database row order, locale collation, and
client path separators are never inputs. The snapshot records algorithm,
format version, root hash, entry count, logical bytes, and capture-error summary.

The client's expected root hash, if supplied, is cross-check evidence. The
server-computed value is authoritative. Any format change receives a new
version and golden cross-language fixtures before writers emit it.

## Snapshot creation protocol

### 1. Initiate

```http
POST /api/v1/backup-sets/{backup_set_id}/snapshots
Idempotency-Key: <backup-run-id>
```

The authenticated source device supplies optional parent snapshot, scan start,
client/platform/capability versions, and intended consistency mechanism. The
server locks/checks the backup set, device, policy, concurrent run limit, quota
reservation, parent eligibility, and idempotency fingerprint, then creates one
`BUILDING` snapshot with TTL and build lease.

An identical retry returns the same snapshot. A key reused with a different
set/parent/capture request returns `idempotency_conflict`.

### 2. Submit manifest batches

```http
POST /api/v1/backup-snapshots/{snapshot_id}/entry-batches
Idempotency-Key: <client-batch-id>
```

Batches contain bounded entries sorted/identified according to the manifest
protocol. The server validates structure incrementally, uses unique constraints
for entry and parent/name identities, and stores one batch fingerprint/outcome.
An identical retry returns it; a changed batch under the same identity fails.

Batch submission does not perform long object writes in its database
transaction. During `BUILDING`, the server may report the protocol disposition
`UPLOAD_REQUIRED` for a new/changed file; this is not an additional persisted
`BackupEntry.capture_result`. Final entry capture results remain exactly:

- `UNCHANGED`: the client names an entry from the selected previous committed
  snapshot; the server verifies same set/owner, retained verified object, and
  compatible expected length/hash, then copies the object binding into this
  snapshot entry;
- `PRESENT`: a completed backup content claim has server-verified length/hash;
- `UNREADABLE`, `EXCLUDED`, or `MISSING_OBSERVATION`: explicit non-content
  capture result with a safe error class.

The server does not expose arbitrary hash-existence queries. New/changed files
upload fully and dedup only after server verification. A future proof-of-
possession optimization requires a separate reviewed protocol.

### 3. Ingest required content

Backup content uses the same bounded streaming, part verification, whole-object
SHA-256, immutable finalization, retry, lease, and orphan behavior as
[UPLOADS.md](UPLOADS.md), but its destination is a backup-scoped content claim,
not a visible `Node` or `FileVersion`.

The internal claim freezes snapshot+entry ownership and expected size/hash. Its
terminal result can bind only that authorized manifest entry. It must not expose
raw `Object` creation or allow bytes verified for one owner/snapshot to be
retargeted. OpenAPI/domain implementation may either add a registered
`BACKUP_ENTRY` upload intent or expose a backup-specific wrapper over the same
application service; it must not duplicate the byte-integrity state machine.

### 4. Seal

```http
POST /api/v1/backup-snapshots/{snapshot_id}/complete
Idempotency-Key: <snapshot-completion-key>
```

The request supplies final entry count, scan end, expected root hash, consistency
evidence, and capture-error summary. A short transaction locks `BUILDING`,
validates every submitted batch is terminal, freezes a manifest fingerprint,
changes state to `VERIFYING`, and creates one verifier job/lease. Later calls
cannot add or replace entries.

The endpoint may return `202` and status URI. A semantically identical retry
returns the same verification/terminal outcome. A different sealed fingerprint
returns `manifest_conflict`.

### 5. Verify and commit

The verifier uses bounded/paginated database reads to check topology, content,
totals, canonical root hash, capture policy, and object states. It does not hold
one long transaction during the scan. It persists verification generation and
summary, renews the build lease, then performs one short final transaction:

1. lock snapshot, backup set, reservation/accounting, and verifier generation;
2. ensure no entry batch changed after seal and all verification evidence
   matches the frozen fingerprint;
3. ensure the build lease safely covers commit and no referenced object is
   invalid/quarantined/deleting;
4. apply policy: reject required-complete snapshots with capture failures, or
   commit an explicitly labeled best-effort snapshot;
5. set `COMMITTED`, server commit time, final counts/hash/consistency label;
6. convert reservation/accounting, persist terminal idempotent outcome,
   `AuditEvent`, `backup.snapshot.committed.v1` outbox, and retention/integrity
   follow-up jobs;
7. commit, then return success and release staging-only resources later.

If DB commit fails, the snapshot remains recoverable in `VERIFYING`; objects
stay protected by lease. If commit succeeds and response is lost, the same key
returns exactly the committed snapshot.

## Snapshot consistency classes

Every committed snapshot displays one of:

- `FILESYSTEM_CONSISTENT`: the client captured from a supported point-in-time
  filesystem/volume snapshot and supplies capability/evidence recognized by
  the protocol;
- `CRASH_CONSISTENT`: the client used a bounded scan with before/after stat,
  retry/stability checks, and no known unresolved content mutation, approximating
  what would survive an abrupt application stop;
- `BEST_EFFORT`: files may have changed during capture or explicit
  unreadable/unstable/unsupported entries remain.

These labels describe capture, not object-storage durability. The server stores
client method/version and verification summary and never calls an ordinary live
scan filesystem-consistent. A policy can fail the run instead of committing
`BEST_EFFORT`. A committed best-effort snapshot is restorable for the entries
it actually verified, with limitations prominently reported.

Client capture guidance:

- detect change during file read using stable file identity plus pre/post size,
  modification/change metadata where reliable; retry within a bounded count;
- hash the exact bytes uploaded, not only a later pathname;
- record rename/replacement discovered mid-scan explicitly;
- do not follow symlinks or mounts contrary to frozen set policy;
- enumerate in bounded batches and persist outbound progress so restart can
  resume the same snapshot/run identity.

## Unchanged files and deduplication

The safest no-upload fast path references a previous retained entry, not an
arbitrary hash:

1. client claims stable source identity unchanged from parent entry and sends
   expected metadata/hash;
2. server authorizes both snapshots in the same backup set and verifies the
   prior object remains `VERIFIED`;
3. new snapshot receives its own complete `BackupEntry` referencing that object;
4. snapshot commit makes the new reference authoritative.

If prior content is missing/quarantined/expired or identity evidence is
insufficient, server requests upload. Dedup of newly uploaded equal content
uses the same domain-scoped post-verification rule as storage. Repeated backups
therefore reuse bytes without coupling snapshot retention.

## Source deletion and missing input

If a source file was present in snapshot S1 and absent in later S2:

- S2 simply has no present entry for that source/path, or records a bounded
  `MISSING_OBSERVATION` when the scan discovered disappearance mid-run;
- S1 remains immutable/restorable until its own retention expires;
- no live `Node` is trashed and no S1 entry/reference is removed;
- retention policy, not the source device, decides when S1 can expire.

Unplugging a drive, permission loss, skipped root, incomplete enumeration, or
client bug must not look like a successful mass deletion. Root presence and
scan-completeness checks either fail the snapshot or visibly classify it
`BEST_EFFORT` according to policy.

## Retention

### Policy model

A versioned retention policy may combine:

- keep the latest successful `N` snapshots;
- keep snapshots younger than an age;
- daily/weekly/monthly representative buckets;
- minimum successful recovery points and protection after recent failure;
- manual pin/legal/incident hold;
- quota-pressure behavior that asks for user/admin action rather than silently
  weakening promised retention.

The effective policy revision and computed retention deadline are stored with
each committed snapshot. Policy changes are audited and prospectively
re-evaluated under an explicit rule; the UI previews what would expire.

Recommended safety rules are: never expire a held snapshot, never let a failed
run displace the minimum successful recovery floor, and require explicit
confirmation before retiring a set deletes its final recovery points.

### Expiry workflow

1. A retention planner computes candidates deterministically from committed
   snapshots, policy revision, holds, and server time. It records a dry-run
   explanation.
2. A short transaction locks the backup set and each bounded candidate,
   re-evaluates facts, changes winners from `COMMITTED` to `EXPIRED`, and appends
   audit/outbox plus idempotent purge jobs.
3. An expired snapshot immediately stops being advertised as restorable, but
   its entry rows/object references remain physically protected while purge
   batches run.
4. The purge worker deletes entry/reference rows in deterministic bounded
   batches with progress/generation. Crash/retry never exposes a partial
   snapshot as committed.
5. Completion tombstones snapshot metadata according to audit/product policy.
   Objects merely become candidates for the separate two-phase GC proof.

Retention never calls object delete directly, never uses a cached refcount as
proof, and never cascades into live files, Trash, another snapshot, or another
backup set.

## Restore domain

### Restore targets

Synveil supports/plans two explicit paths:

1. **Restore to a device/filesystem:** a newly authorized client receives the
   immutable manifest and bounded authorized content streams, writes to a
   selected local destination, and reports verified per-entry results.
2. **Restore into a Synveil library:** the server creates new `Node` and
   `FileVersion` state using existing immutable objects, then emits normal
   library `ChangeEvent` facts. It does not copy bytes unless storage policy
   requires another replica.

The default destination is non-destructive: a new user-selected directory, or
a new top-level directory named as a restore recovery point. Restoring directly
over an existing tree requires explicit collision policy and fresh authorization.

### Restore operation state machine

```mermaid
stateDiagram-v2
    [*] --> PLANNING
    PLANNING --> RUNNING: immutable plan accepted
    RUNNING --> RUNNING: idempotent bounded entry batches
    RUNNING --> VERIFYING: all required entries attempted
    VERIFYING --> COMPLETED: every required result verified or accepted skip
    VERIFYING --> PARTIAL: one or more visible unresolved failures
    PLANNING --> CANCELED
    RUNNING --> CANCELED: safe boundary
    PLANNING --> FAILED: invalid source/destination
    RUNNING --> FAILED: terminal operation-wide error
```

A restore record freezes source snapshot/version, selected entries, destination
identity, collision/symlink/metadata policy, actor/device, and idempotency key.
It owns a retention/object lease so the source cannot expire mid-operation.
Per-entry rows record planned action, attempts, output identity/path projection,
expected and observed length/hash, collision result, safe error, and terminal
verification state.

Workers claim bounded batches with generation leases and execute outside the
claim transaction. A retry inspects the per-entry result and destination
identity before writing. It never assumes "job ran once." Cancellation stops
new batches and reports already restored entries; it does not undo them by
destructive bulk deletion.

### Collision policy

- `RENAME` is the recommended non-destructive default: create a versioned,
  portable alternate name and report it.
- `FAIL` stops/reports collisions without modifying the occupant.
- `SKIP` is allowed only as an explicit accepted result recorded per entry.
- `OVERWRITE` requires explicit confirmation/scope. Into a Synveil library it
  creates a new current `FileVersion` with source `BACKUP_RESTORE` under the
  current base precondition, retaining old history. On a device, the client
  uses platform-safe temp+verify+atomic replacement and preserves/report policy
  as configured.

Directory and file type collisions never coerce one type into another silently.
Case/normalization collisions are detected before writing and surfaced under
the target platform/name-policy profile.

### Path and symlink safety

For filesystem restore, the client treats every manifest component as
untrusted: reject absolute paths, traversal, NUL, reserved/special names, depth
overflow, and any resolved path outside the selected destination. Create
directories/files without following attacker-controlled symlinks and re-check
parent identity across races. A manifest symlink is not followed while writing
children; initial policy skips/reports it unless the user explicitly enables a
safe platform-specific recreation mode.

Special files, ownership, ACLs, xattrs, sparse layout, and timestamps are
restored only when both manifest profile and client capability register the
semantics. Lack of metadata support is an explicit per-entry warning, not a
content verification success.

### Verification

Every restored file streams from an authorized snapshot entry, not a raw object
ID. It validates expected canonical length/SHA-256. A library restore references
the already verified object and can optionally scrub/read before commit based
on age/health policy. A filesystem client hashes the installed logical file
after write/flush and reports result.

`COMPLETED` means all required entries are verified or explicitly accepted
skips. `PARTIAL` lists exact unresolved entries and remains resumable. A server
cannot cryptographically prove an untrusted client wrote durable local media;
the status distinguishes server-delivery verification from client-reported
destination verification.

### Recovery after device loss

1. Revoke the lost device and its credentials; do not claim to erase bytes
   already on it.
2. Register/authenticate a replacement device with independently scoped
   credentials.
3. List authorized backup sets/snapshots with consistency class, capture
   errors, verification health, and retention deadline.
4. Choose a snapshot/entries and a non-destructive destination.
5. Create one durable restore operation and resume content by entry/range after
   interruption.
6. Verify results and export a human-readable restore report before optionally
   enabling normal sync/backup on the restored destination.

The lost device is not required to decrypt early server-trusted backups. A
future E2EE mode needs an independent key recovery/onboarding protocol and
cannot inherit this assumption.

## End-user recovery, uninstall, and machine migration

Recovery is a product surface, not only an operator runbook. Personal / Home
Mode presents understandable workflows for accidental deletion, an earlier
version, a lost laptop, failed or removable storage, a corrupt object, a broken
update, database recovery, and moving Synveil to a new computer or server.
Advanced / Server Mode exposes the same primitives through operator diagnostics.

The guided machine-migration package follows:

```text
prepare migration
    → inspect source and destination
    → validate release/schema, PostgreSQL, object identity, keys, capacity and host
    → copy/transfer with resumable progress
    → verify references, checksums, health and device re-registration
    → activate destination and preserve a rollback window
```

The operation uses `inspect → plan → validate → execute → verify`, a durable
operation identity, explicit source/destination ownership, and a non-destructive
destination by default. It must account for PostgreSQL metadata, canonical
objects/replicas, application master keys, device credentials, hostname/TLS,
remote-access configuration, backup policy, old-instance coexistence and
rollback. A missing key, incomplete object transfer, unsupported filesystem
capability, or failed health check is a visible blocker; it never causes a new
identity to be generated and presented as continuity.

Uninstall and reinstall are part of recovery validation. Removing application
binaries/services keeps PostgreSQL, objects, configuration, keys, and independent
backups according to the selected retention choice. Permanent data deletion is
a separate, confirmed operation with scope and recovery warning. Reinstall
discovers a retained storage identity and runs validation before any bootstrap;
it never treats a kept data root as empty merely because the application binary
was removed.

## Backup API blueprint

Expected routes include:

```text
POST   /api/v1/backup-sets
GET    /api/v1/backup-sets
GET    /api/v1/backup-sets/{backup_set_id}
PATCH  /api/v1/backup-sets/{backup_set_id}                 If-Match required
POST   /api/v1/backup-sets/{backup_set_id}/snapshots       idempotency required
GET    /api/v1/backup-snapshots                 keyset pagination
GET    /api/v1/backup-snapshots/{snapshot_id}
POST   /api/v1/backup-snapshots/{snapshot_id}/entry-batches
POST   /api/v1/backup-snapshots/{snapshot_id}/content-claims
POST   /api/v1/backup-snapshots/{snapshot_id}/complete
GET    /api/v1/backup-snapshots/{snapshot_id}/entries    opaque keyset cursor
POST   /api/v1/restores                         idempotency required
GET    /api/v1/restores/{restore_id}
GET    /api/v1/restores/{restore_id}/entries
POST   /api/v1/restores/{restore_id}/resume
POST   /api/v1/restores/{restore_id}/cancel
```

Exact route naming/schema belongs in reviewed OpenAPI. List queries scope
authorization before filters/counts and use stable keyset pagination. Snapshot
entry download authorizes through owner/set/snapshot/entry and committed state;
it never accepts an object ID alone. Mutations use `If-Match`/revision and
idempotency fingerprints. Large manifests are batched; no endpoint requires a
million-entry JSON body or holds all entries in memory.

Stable errors include `backup_set_paused`, `snapshot_not_restorable`,
`snapshot_incomplete`, `manifest_conflict`, `capture_inconsistent`,
`object_corrupt`, `restore_conflict`, `unsupported_entry_type`,
`quota_exceeded`, `storage_unavailable`, `device_revoked`,
`permission_denied`, and `internal_error`.

## Quota and accounting

Snapshot build reserves expected/staged capacity under bounded policy. Content
claims cannot exceed declared entry length or aggregate run limit. Commit
converts reservations to retained backup logical accounting and releases unused
staging reservation atomically; failure/expiry releases once.

Report separately:

- logical bytes represented by each snapshot;
- logical unique content for a backup set/reporting period;
- physical object bytes attributable only as an estimate under shared dedup;
- staging/reserved bytes;
- retained versus expiring/held snapshot bytes.

Dedup savings do not silently extend or reduce promised logical quota. A
retention policy and a quota policy cannot deadlock the user: if capacity is
insufficient, the system reports required action and protects the documented
minimum recovery floor rather than deleting it invisibly.

## Events and jobs

Snapshot and restore state changes append audit and internal work, not live
sync changes unless restore intentionally mutates a library.

Registered internal events/jobs should include versioned forms of:

- `backup.snapshot.verify`;
- `backup.snapshot.committed.v1`;
- `backup.snapshot.expire` and bounded purge;
- `backup.object.scrub`;
- `restore.plan`, `restore.batch`, and `restore.verify`;
- device backup health/notification updates.

Snapshot commit inserts required outbox/jobs in the same transaction. Handlers
are at-least-once and bind idempotency to snapshot/restore ID plus immutable
revision/entry batch. Job lease generation, retry classification, dead letters,
and manual replay use [STORAGE.md](STORAGE.md). Optional notification/anomaly
detection outage changes freshness/lag only.

A future ransomware/anomaly signal may place an audited retention hold or ask
for confirmation, but it must not automatically delete, rewrite, or declare a
snapshot safe. False positives cannot block ordinary restore indefinitely.

## Instance disaster recovery

### Recovery set

A restorable Synveil instance backup includes:

- PostgreSQL metadata/transaction state, including migrations, journal,
  snapshots, object locations, jobs, audit, and idempotency receipts;
- every object-store key referenced by that database recovery point;
- storage-backend identity/configuration, Compose/Caddy configuration, instance
  identity, and exact application/schema versions;
- authentication/storage encryption master material and referenced secrets,
  protected separately with an operator recovery procedure;
- a signed/hashed inventory and restore-runbook version.

Database-only backup loses file bytes. Object-volume-only backup loses names,
authorization, versions, journals, backup manifests, and key mappings. Missing
master secrets can make otherwise present data/auth state unrecoverable.

The recovery set is also the source of truth for a machine migration. It records
which instance identity, device-credential rotation, hostname/TLS, remote-access
configuration, platform/filesystem capability profile, and release/schema
compatibility are being moved. A migration may preserve an old instance during a
rollback window, but two writers must not be active against one logical identity
without an explicitly designed protocol.

### Initial recommended offline/maintenance procedure

The safest Compose baseline is a documented maintenance window:

1. enter maintenance/read-only mode and stop new sessions, upload completion,
   metadata mutation, retention, GC, and migrations;
2. drain/stop API and worker writers at a known schema/application version;
3. take a supported PostgreSQL logical/physical backup and a filesystem/object
   snapshot/copy while no writer or GC changes references/keys;
4. capture configuration, storage-identity marker, migration/application
   version, and protected required secrets;
5. compute/store inventory/checksums outside the protected data set;
6. restart services only after backup commands succeed or explicitly report
   failure;
7. regularly restore the set into an isolated deployment and verify every
   database-referenced object plus representative full hashes.

Copying a live PostgreSQL data directory is not a supported database backup.
Stopping only PostgreSQL while API/worker continue writing objects is not a
coordinated backup.

### Future online procedure

An online design can exploit durable-object-before-database-reference ordering,
but requires an explicit GC barrier:

1. create a durable disaster-recovery barrier that pins every object referenced
   at/through database recovery point `T` and blocks relevant physical deletion;
2. capture a PostgreSQL consistent backup at `T`;
3. copy/snapshot object storage at a point not earlier than `T` while the barrier
   prevents deletion of objects referenced by the DB snapshot;
4. capture configuration/secrets/version inventory and verify all references;
5. release the barrier only after the backup is complete or terminally failed.

Extra object keys created after `T` are safe orphans on restore. Missing an
object referenced at `T` is not safe. Taking object storage first and database
later without quiescence can include a later DB reference whose object was not
in the earlier object snapshot; that order is invalid.

S3 versioning, replication, RAID, ZFS/Btrfs snapshots, and PostgreSQL PITR are
useful mechanisms but none alone constitutes the coordinated recovery set.

### Instance restore procedure

1. Restore into an isolated network/paths, never over the only source copy.
2. Verify backup inventory, required secrets, storage identity, application
   version, and migration compatibility before starting writers.
3. Restore PostgreSQL using supported tooling and attach/copy object storage
   under the recorded backend identity.
4. Start in maintenance/read-only mode. Run a complete reference inventory:
   every protected object has a known key/replica; extras are reported but not
   immediately deleted.
5. Verify representation checksums and a policy-selected/full canonical sample;
   quarantine/report every mismatch.
6. Validate auth bootstrap/recovery, journal head/event constraints, snapshot
   manifest roots, jobs/leases, and storage accounting reconciliation.
7. Perform representative file, old-version, Trash, backup snapshot, and device
   restore drills.
8. Only then make the restored instance writable and establish a new backup
   baseline. Preserve the previous source through a rollback window.

Restore-time schema upgrade occurs only under the normal reviewed migration
path. A newer binary never silently rewrites an unverified old recovery set.

## Failure and recovery matrix

| Failure | Required outcome |
|---|---|
| Client disconnects during entry/content batch | `BUILDING` remains resumable; batch/content receipt replays by idempotency key. |
| Source file changes while read | Retry boundedly or record unstable/unreadable; never claim stronger consistency. |
| Source root disappears/unmounts | Fail snapshot or commit visibly `BEST_EFFORT` under policy; older snapshots remain. |
| Content object durable, entry/DB update fails | No committed snapshot reference; claim/lease enables retry, then orphan grace. |
| Snapshot seals with missing entry/object | Verification fails; no partial restorable snapshot. |
| Verifier crashes | Lease expires; successor recomputes/resumes from frozen manifest. |
| Snapshot DB commit succeeds, response lost | Completion key returns same committed snapshot/root hash; no duplicate. |
| Snapshot commit succeeds, notification worker is down | Snapshot remains restorable; durable outbox/job becomes late. |
| Retention races restore | Restore lease/transaction or expiry wins; a started authorized restore cannot lose its source silently. |
| Retention worker dies mid-purge | Snapshot stays non-restorable `EXPIRED`; deterministic batch progress resumes; object refs are not prematurely GCed. |
| Object is corrupt/missing during restore | Entry fails safely/uses another verified replica; operation becomes resumable `PARTIAL`, not false success. |
| Restore response/job completion is lost | Per-entry idempotency and destination check prevent duplicate destructive writes. |
| Quota fills during backup | No partial snapshot commits; preserve existing recovery floor, stop/retry run with explicit error. |
| Device is revoked mid-backup | New requests fail; current uncommitted build expires/cleans; committed snapshots remain by retention. |
| Database backup succeeds, object backup fails | Recovery set is failed/incomplete and not rotated in as the only backup. |
| Restored DB references missing object | Keep metadata, quarantine/report and seek another recovery set; never create empty bytes or delete the row. |

## Required tests

### Snapshot protocol

- empty source, one file, deep tree, multiple roots, maximum policy count, and
  large manifest with bounded memory/transactions;
- entry batches out of order, duplicate identical batch, changed payload under
  same key, lost response, missing batch, duplicate ID/name, missing parent,
  cycle, file-as-parent, traversal/absolute/special component, and depth/size
  overflow;
- zero-byte and very large file content claims, interrupted/resumed parts,
  checksum mismatch, durable object plus forced DB rollback;
- seal racing final entry/content claim is linearizable; post-seal mutation is
  rejected;
- verifier crash at every phase, lease takeover, root-hash mismatch, object
  becomes quarantined/deleting, DB serialization retry, commit-response loss;
- no `BUILDING`/`VERIFYING`/`FAILED`/`EXPIRED` snapshot appears restorable;
- repeated unchanged backup creates complete new entries and reuses object
  references without requiring parent retention;
- changed large file creates correct new object/version binding; equal newly
  uploaded bytes dedup only after verification and inside the domain.

### Deletion, completeness, and consistency

- file present in S1, locally deleted before S2: S1 remains restorable until
  retention; no live sync deletion emitted by backup;
- source root unplugged, permission denied, file vanishes mid-read, file changes
  repeatedly, excluded file, unsupported type, symlink loop, mount boundary;
- policy requiring complete capture fails appropriately; best-effort policy
  commits with exact error summary and never labels itself filesystem-consistent;
- filesystem snapshot evidence, crash-consistent scan, and ordinary live scan
  receive only their allowed labels;
- client lies/misreports count/hash/consistency and server verification catches
  every server-verifiable inconsistency.

### Retention and GC interaction

- keep-last/age/bucket policy golden timelines, clock boundary, legal/manual
  hold, policy revision, failed run, final recovery floor, and dry-run preview;
- retention versus new commit/restore/hold race has one locked decision;
- crash after `COMMITTED -> EXPIRED` and after every purge batch resumes without
  a partially restorable snapshot;
- an object shared by live version, Trash, S1, S2, derivative, and active
  restore is not GC-eligible until every relevant reference/lease expires;
- cached refcount corruption cannot delete a retained snapshot object;
- deleting/retiring a device or backup set never bypasses retention confirmation.

### Restore

- full snapshot, one file, directory subtree, historical file version, and
  restart after every entry;
- default restore creates a non-destructive destination; `RENAME`, `FAIL`,
  `SKIP`, and explicit `OVERWRITE` have exact per-entry outcomes;
- library overwrite creates a new `FileVersion` and journal event while old
  history remains;
- filesystem path traversal, absolute path, symlink-parent race, reserved/case
  collision, depth/path limit, special file, ACL/xattr unsupported warning;
- corrupt/truncated/download interruption, HTTP range resume, target disk full,
  post-write hash mismatch, client crash before/after atomic replace;
- restore job/response replay does not create duplicate nodes or overwrite
  twice; cancel reports already completed entries without deleting them;
- retention/GC/device revocation races and recovery on a newly registered device;
- `COMPLETED` only after required verification; exact unresolved results produce
  `PARTIAL` with safe resume.

### Instance disaster recovery

- automated isolated restore of PostgreSQL + local object store + config/secrets
  at the supported version;
- prove DB-only and object-only sets fail completeness checks;
- inject object-copy failure after DB backup and ensure the recovery set is not
  promoted/old backup not removed;
- online prototype: object committed before DB point, concurrent new objects,
  GC candidate at barrier, and prove every reference at `T` is copied while
  extras are harmless;
- missing storage marker, wrong bucket/root, wrong encryption key, migration
  mismatch, missing/corrupt object, extra orphan, stale jobs/leases;
- restore and verify live file, old version, Trash subtree, committed backup
  snapshot, sync cursor constraints, and authentication recovery;
- guided migration from old to clean destination, retained storage identity,
  interrupted copy/resume, key/device credential rotation, hostname/TLS and
  remote-access change, old-instance coexistence, rollback window, and explicit
  blockers for missing key/object/capability;
- application-only uninstall followed by reinstall discovery, permanent-data
  deletion confirmation, and proof that removing binaries never deletes the
  only database/object/key/backup copy;
- document recovery time/space and repeat drill on a schedule; a backup never
  earns `VERIFIED` status without a successful restore test.

### Property/fuzz/model tests

- randomized snapshot build/seal/verify/commit/expire/crash commands never make
  an incomplete manifest restorable;
- randomized retention/reference/lease transitions never delete an object used
  by any committed snapshot or active restore;
- canonical manifest golden fixtures match Rust and future client languages;
- fuzz manifest parser, length-delimited hashing, path components, symlink
  metadata, batch cursors, counts, and checked arithmetic;
- model repeated backup/delete/restore sequences and prove source deletion alone
  never removes an older retained recovery point.

## Observability and release gate

Metrics cover last successful backup/age by set/device, runs and snapshots by
state/consistency, scanned/uploaded/reused/logical/physical bytes, capture error
classes, build/verifier lease age, manifest verification time, root-hash
mismatch, retention candidates/holds/purge backlog, restore throughput/results,
object corruption, and instance recovery-set/drill age. Logs/traces use run,
snapshot, restore, device, and job correlation IDs but redact content, raw local
paths where sensitive, credentials, storage keys, and secrets.

Phase 5 requires end-to-end backup from a reference desktop client, every crash
boundary above, retention/GC proof, full and selective restore verification,
lost-device workflow, quota/full-disk behavior, and operator runbooks. Production
status additionally requires an automated coordinated instance backup and a
successful isolated restore drill; "files copied somewhere" is insufficient.

## Open decisions

OPEN DECISION OD-BACKUP-001: canonical manifest format v1
Owner: Backup / Clients / Storage
Needed by: Phase 5 protocol and stored-format gate
Options: normalized relational rows plus server Merkle root; immutable CBOR manifest object plus indexed rows; protobuf manifest with canonicalization profile
Recommendation: use normalized bounded PostgreSQL entry rows for initial query/commit and a precisely specified server-computed Merkle root with golden fixtures; optionally export a canonical portable manifest artifact without making it the only index
Decision evidence: million-entry memory/DB benchmark, cross-language canonical hash fixtures, corruption localization, and export/restore test

OPEN DECISION OD-BACKUP-002: default retention policy
Owner: Product / Backup / Operations
Needed by: Phase 5 UI and policy gate
Options: simple keep-last plus age; grandfather-father-son buckets; user-configured only with a safe minimum
Recommendation: ship a simple understandable keep-last-plus-age default with at least one protected latest successful recovery point, dry-run preview, and explicit holds; add bucket policies only after UX/test evidence
Decision evidence: representative household capacity simulations, accidental-deletion recovery expectations, abuse/quota review, and UI comprehension testing

OPEN DECISION OD-BACKUP-003: platform metadata profile
Owner: Backup / Desktop Clients / Security
Needed by: each platform client release gate
Options: portable content/names/times only; capability profiles for POSIX/Windows/macOS ACLs, xattrs, sparse files, hard links, and symlinks; opaque metadata blobs
Recommendation: start with a documented portable subset and explicit safe symlink policy; add typed versioned platform profiles individually, never opaque replay of privileged metadata
Decision evidence: cross-platform round-trip corpus, privilege/path threat review, and restore fidelity report

OPEN DECISION OD-BACKUP-004: online instance-backup service level
Owner: Operations / Database / Storage
Needed by: post-baseline online backup gate; not blocking maintenance-window backups
Options: maintenance-window coordinated backup; PostgreSQL point plus GC barrier and later object copy; infrastructure atomic snapshots of validated volumes
Recommendation: support and drill the maintenance-window procedure first; add the DB-point-plus-GC-barrier online protocol only after fault injection proves every referenced object is captured
Decision evidence: power/crash tests, concurrent commit/GC model, provider snapshot semantics, restore completeness, RPO/RTO measurements

# Deployment assets

## `systemd/` — Prompt 72 external lifecycle (Linux)

Prompt 72 introduces the first production-oriented external lifecycle for
scheduled backup maintenance. The one-shot runtime (`crates/api/src/bin/
synveil-scheduled-maintenance-once.rs`) remains the single bounded
execution boundary; `systemd` owns recurrence.

- `deploy/systemd/synveil-scheduled-maintenance.service` — `Type=oneshot`
  service that executes exactly the canonical one-shot binary
  (`ExecStart=/usr/bin/synveil-scheduled-maintenance-once`) once per
  activation. `EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env`
  supplies optional non-secret `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS`
  (10..=900, default 120); database credential is delivered via
  `LoadCredential=database-url:/etc/synveil/credentials/database-url` and
   `Environment=SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url` (Prompt 77,
   not `DATABASE_URL` in the env file). `Restart=no`, `TimeoutStartSec=300`,
   hardened sandbox (Prompt 78: `NoNewPrivileges`, `ProtectSystem=strict`,
   `PrivateTmp`, `PrivateDevices` + `DevicePolicy=closed`,
   `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6`,
   `SystemCallFilter=@system-service` minus dangerous classes, …) and journal
   logging.
  Since Prompt 75 it runs as the persistent service identity `User=synveil`
  `Group=synveil` with `RuntimeDirectory=synveil` (0750) and requires the
  `synveil` system account from `deploy/sysusers.d/synveil.conf`; earlier
  Prompt 72 deferred `User=` pending that packaging.

- `deploy/systemd/synveil-scheduled-maintenance.timer` — external cadence,
  **approximately once per minute** (`OnCalendar=*:*:00`, `AccuracySec=1s`),
  `Persistent=true` (single wake-up after downtime, not hundreds of
  replays), `RandomizedDelaySec=10s` (bounded fleet jitter, not a Rust
  sleep). User backup schedule recurrence (`DAILY`/`WEEKLY`, `LATEST_ONLY`/
  `REPLAY_ONE_BY_ONE`, lateness) remains independent; the timer merely
  asks “is there bounded work now?”

## `sysusers.d/` and `tmpfiles.d/` — Prompt 75 service identity (Linux)

Prompt 75 owns the least-privilege identity and ownership contract:

- `deploy/sysusers.d/synveil.conf` — declarative `systemd-sysusers` source
  that creates persistent system user+group `synveil` (`-/var/lib/synveil`
  `/usr/sbin/nologin`, auto-allocated UID/GID, no supplementary groups,
  no hardcoded numeric UID). Installed to `/usr/lib/sysusers.d/synveil.conf`;
  `systemd-sysusers --dry-run --root=/tmp/root` validates without host
  mutation. Rejected alternative: `DynamicUser=yes` (transient UID cannot
  own `/var/lib/synveil`, `/etc/synveil`, or future object-store paths and
  would hide state under `/var/lib/private`).

- `deploy/tmpfiles.d/synveil.conf` — declarative `systemd-tmpfiles` source
  that ensures `/var/lib/synveil 0750 synveil:synveil` exists at boot/package
  time. Runtime ephemeral state `/run/synveil` is **not** in tmpfiles; it is
  created per-activation via `RuntimeDirectory=synveil` in the service unit
  (correct ownership, lifecycle-tied cleanup, no stale host state). No
  `StateDirectory=` is used — one host-level tmpfiles declaration remains
  authoritative for all future Synveil services sharing `/var/lib/synveil`
  (avoids per-service conflicting managers).

### Filesystem ownership contract (Gen-1)

| Path | Owner | Mode | Creator | Access |
|---|---|---|---|---|
| `/usr/bin/synveil-*` | `root:root` | `0755` | package (root) | synveil can read/exec, not write |
| `/usr/lib/systemd/system/synveil-*.service` `/usr/lib/systemd/system/synveil-*.timer` | `root:root` | `0644` | package | synveil not writable |
| `/usr/lib/sysusers.d/synveil.conf` `/usr/lib/tmpfiles.d/synveil.conf` | `root:root` | `0644` | package | synveil not writable |
| `/etc/synveil` | `root:synveil` | `0750` | package | synveil can read non-secret configs, traverse; cannot rewrite admin config |
| `/etc/synveil/synveil-scheduled-maintenance.env` | `root:synveil` | `0640` | admin/package | synveil reads `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS` via `EnvironmentFile` (non-secret); **no DATABASE_URL** in production file — credentials via `LoadCredential` |
| `/etc/synveil/credentials` | `root:root` | `0700` | package (skeleton) | admin-only; synveil has **no direct read** — systemd reads and exposes per-service copy |
| `/etc/synveil/credentials/database-url` | `root:root` | `0600` | admin | secret source; systemd `LoadCredential=database-url:...` exposes as `$CREDENTIALS_DIRECTORY/database-url` read-only to `synveil` |
| `/var/lib/synveil` | `synveil:synveil` | `0750` | tmpfiles.d | persistent non-secret state, future appliance bookkeeping; empty today is acceptable |
| `/run/synveil` | `synveil:synveil` | `0750` | `RuntimeDirectory=` | ephemeral per-activation, cleaned on stop |
| `/run/credentials/synveil-scheduled-maintenance.service/database-url` | `root:synveil` (systemd) | `0400` | `LoadCredential` | per-service credential copy, unswapped, read-only; lives only for process lifetime |
| `/var/log/synveil` | — | — | — | **not created**; structured logging goes to journald |

Storage/object-store roots are governed by the storage architecture, not
recursively chowned to `synveil` in Prompt 75.

Installation (packaged layout): units to `/usr/lib/systemd/system/` (or
distro-equivalent); sysusers/tmpfiles fragments to `/usr/lib/sysusers.d/`
and `/usr/lib/tmpfiles.d/`; configuration to `/etc/synveil/synveil-scheduled-maintenance.env`
(non-secret); credentials to `/etc/synveil/credentials/database-url`;
persistent state to `/var/lib/synveil` (auto-created); binary to
`/usr/bin/synveil-scheduled-maintenance-once`. Repository source
must not be written to `/etc/systemd/system` during tests — validation uses
temporary copies and `systemd-analyze verify`.

```sh
# package install (root) creates the account and directories:
sudo systemd-sysusers  # reads /usr/lib/sysusers.d/synveil.conf
sudo systemd-tmpfiles --create  # reads /usr/lib/tmpfiles.d/synveil.conf
# (systemd package scripts invoke these automatically)

# configure non-secret env (lease tuning, no DATABASE_URL)
sudo install -d -m 0750 -o root -g synveil /etc/synveil
sudo install -m 0640 -o root -g synveil /dev/stdin /etc/synveil/synveil-scheduled-maintenance.env <<'EOF'
# SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS=120
EOF

# provision database credential via systemd-credentials (least privilege)
# admin source is root-only; synveil does not need direct read
sudo install -d -m 0700 -o root -g root /etc/synveil/credentials
sudo install -m 0600 -o root -g root /dev/stdin /etc/synveil/credentials/database-url <<'EOF'
postgresql://synveil:***@127.0.0.1:5432/synveil
EOF
# rotate atomically: write tmp file + chmod 0600 + mv into place
# next timer activation picks up new credential (no daemon restart)

sudo systemctl daemon-reload
sudo systemctl enable --now synveil-scheduled-maintenance.timer
systemctl is-active synveil-scheduled-maintenance.timer
journalctl -u synveil-scheduled-maintenance.service --since today
# disable
sudo systemctl disable --now synveil-scheduled-maintenance.timer
```

See `docs/en/BACKUP.md` § “External lifecycle and systemd cadence (Prompt 72)”
and `docs/en/DEPLOYMENT.md` § “Linux service identity and filesystem ownership
(Prompt 75)” for cadence rationale, overlap/failure/boot semantics,
DynamicUser evaluation, UID/GID policy, per-service-account trade-offs, and
the privilege boundary (package=root, runtime=synveil). No GUI control, no
Windows service, no launchd, no heartbeat/daemon/leader-election/retry is
claimed. Future Synveil API/worker/GC services may share the same `synveil`
account (see docs; per-service accounts deferred).

## `install/` — Prompt 76 package-neutral lifecycle (Linux)

Prompt 76 establishes the distribution-independent installation foundation that
later `.deb`/`.rpm`/PKGBUILD/OCI/Synveil OS packaging calls:

- `deploy/install/MANIFEST` — single authoritative mapping (`source destination
  mode owner group class`) for every installed path. No duplicated logic.
- `deploy/install/install.sh` — fresh install / idempotent reinstall / upgrade
  into any staged root (`--root=/tmp/root`, `DESTDIR`). Atomic `install → mv`,
  staged-root validation, `CONFIG_DIRECTORY` preservation, no host `systemctl`
  mutation in tests. See `deploy/install/README.md`.
- `deploy/install/uninstall.sh` — ordinary uninstall (removes only `PACKAGE`
  artifacts) vs explicit `--purge` (also removes `/etc/synveil` + `/var/lib/
  synveil` after safety checks, still never deletes external pools or
  PostgreSQL). Symlink-safe, parent-safe, path-escape-safe.
- `deploy/install/common.sh` — shared `set -euo pipefail`, quoting, path-
  containment, manifest parsing, and atomic helpers.
- `deploy/config/synveil-scheduled-maintenance.env.example` — template installed
  to `/usr/share/synveil/*.example` (`0644 root:root`); fresh install leaves
  `/etc/synveil/synveil-scheduled-maintenance.env` absent (`EnvironmentFile=-`
  handles it).

Installation contract (Gen-1):

| Path | Owner | Mode | Class | Manager |
|---|---|---|---|---|
| `/usr/bin/synveil-scheduled-maintenance-once` | `root:root` | `0755` | PACKAGE | `install.sh` (atomic) |
| `/usr/lib/systemd/system/synveil-*.service/.timer` | `root:root` | `0644` | PACKAGE | package |
| `/usr/lib/sysusers.d/synveil.conf` etc. | `root:root` | `0644` | PACKAGE | package |
| `/usr/share/synveil/*.env.example` | `root:root` | `0644` | PACKAGE | template (non-secret) |
| `/etc/synveil` | `root:synveil` | `0750` | CONFIG_DIRECTORY | `install.sh` (preserved) |
| `/etc/synveil/credentials` | `root:root` | `0700` | CREDENTIAL_DIRECTORY | `install.sh` (preserved, never seeds secret) |
| `/var/lib/synveil` | `synveil:synveil` | `0750` | STATE_DIRECTORY | `tmpfiles.d` (not seeded) |
| `/run/synveil` | `synveil:synveil` | `0750` | RUNTIME_MANAGED | `RuntimeDirectory=` |
| `/run/credentials/.../database-url` | `root:synveil` (systemd) | `0400` | — | `LoadCredential` (per-activation) |

Data safety: ordinary uninstall and failed upgrade never delete
`/etc/synveil`, `/var/lib/synveil`, external pools, or PostgreSQL.
`--purge` is explicit and still excludes external data. Rollback of
`PACKAGE` version after a failed upgrade is **DEFERRED** to the distro package
manager; this layer guarantees data preservation, not transactional revert.

See `docs/en/DEPLOYMENT.md` § “Linux package lifecycle and data-preserving
uninstall (Prompt 76)”, `docs/adr/ADR-024`, and `deploy/install/README.md` for
full ordering, purge policy, account retention, and `SYNVEIL_INSTALL_FAIL_AFTER`
failure-injection semantics. Staged tests validate via
`crates/metadata/tests/linux_install_lifecycle.rs` without host mutation.

## Hardening validation (Prompt 78 — evidence-driven systemd sandbox)

Prompt 78 hardens the service with the strongest practical sandbox proven
compatible with the real one-shot runtime (`ProtectSystem=strict` with zero
writable exceptions, empty `CapabilityBoundingSet`/`AmbientCapabilities`,
`RestrictSUIDSGID`, kernel tunables/modules/logs/cgroups protection,
`ProtectHome`, `PrivateTmp`, `PrivateDevices` + `DevicePolicy=closed`,
`InaccessiblePaths=/etc/synveil/credentials`, `ProtectProc=invisible` +
`ProcSubset=pid`, `RestrictNamespaces`, `RestrictRealtime`, `LockPersonality`,
`RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6`,
`SystemCallArchitectures=native`, `MemoryDenyWriteExecute`,
`SystemCallFilter=@system-service` minus dangerous classes, `UMask=0077`,
`WorkingDirectory=/`). Full policy, rejected/deferred directives
(`PrivateUsers`, `IPAddressDeny`, `ProtectClock`, `ProtectHostname`,
`RestrictFileSystems`), threat model, and live gate evidence are in
`docs/en/DEPLOYMENT.md` § “Linux systemd sandbox & runtime privilege
hardening (Prompt 78)” and `docs/adr/ADR-026` (LOCKED Gen-1).

Validation method (disposable, no host mutation except the user-manager test
units): `systemd-analyze verify` (service + timer), `systemd-analyze
security` (exposure 4.5 → 1.4 OK on systemd 261.2), and real systemd-managed execution of the
production one-shot binary under the identical sandbox directives against
disposable PostgreSQL 17 — idle (`tick Idle worker Idle`, exit 0), due work
(one `MaterializedAndHandedOff` + one `Stepped` transition), existing work
(`tick Idle` + one worker step) — plus negative probes (writes to
`/usr/bin`/`/etc` denied, home read denied, `/dev/kmsg` denied,
`InaccessiblePaths` mechanism denied, minimal private `/dev`, effective
capabilities all zero with `NoNewPrivs=1`).

Operational notes discovered by the gate and encoded in the unit/ADR:

- `InaccessiblePaths=` requires the path to exist at activation; a missing
  `/etc/synveil/credentials` skeleton fails closed (`226/NAMESPACE`). The
  package always creates that skeleton (`CREDENTIAL_DIRECTORY` in
  `deploy/install/MANIFEST`), so production is unaffected.
- Static tests covering the sandbox live in
  `crates/metadata/tests/linux_sandbox_hardening_units.rs`; install-lifecycle
  tests prove the installed unit is byte-identical to this source.

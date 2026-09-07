# Package-neutral Linux install lifecycle (Prompt 76)

This layer is **distribution-independent**. It stages package-owned files into any
`DESTDIR` / `--root` tree without mutating the developer host. Future `.deb`,
`.rpm`, Arch PKGBUILD, OCI or Synveil OS images call this layer; they do NOT
reimplement destination logic.

## Authoritative manifest

`deploy/install/MANIFEST` is the single source of truth. It enumerates every
installed path with `source  destination  mode  owner  group  class`:

```
BINARY  /usr/bin/synveil-scheduled-maintenance-once  0755  root  root  PACKAGE
deploy/systemd/*.service  /usr/lib/systemd/system/*.service  0644  root  root  PACKAGE
...
-  /etc/synveil  0750  root  synveil  CONFIG_DIRECTORY
-  /etc/synveil/credentials  0700  root  root  CREDENTIAL_DIRECTORY
-  /var/lib/synveil  0750  synveil  synveil  STATE_DIRECTORY
-  /run/synveil  0750  synveil  synveil  RUNTIME_MANAGED
```

`common.sh` parses this file; `install.sh` and `uninstall.sh` never hardcode
destinations. This satisfies PKG-01 and prevents drift between packaging and
tests.

## Filesystem contract (Gen-1)

| Path | Owner | Mode | Class | Manager |
|---|---|---|---|---|
| `/usr/bin/synveil-scheduled-maintenance-once` | `root:root` | `0755` | PACKAGE | `install.sh` (atomic rename) |
| `/usr/lib/systemd/system/synveil-*.service` / `*.timer` | `root:root` | `0644` | PACKAGE | package |
| `/usr/lib/sysusers.d/synveil.conf` / `/usr/lib/tmpfiles.d/synveil.conf` | `root:root` | `0644` | PACKAGE | package |
| `/usr/share/synveil/synveil-scheduled-maintenance.env.example` | `root:root` | `0644` | PACKAGE | template (never overwrites `/etc`) |
| `/etc/synveil` | `root:synveil` | `0750` | CONFIG_DIRECTORY | `install.sh` (mkdir, preserved) |
| `/etc/synveil/synveil-scheduled-maintenance.env` | `root:synveil` | `0640` | admin-owned (not package payload) | admin (`install -m 0640`) — **not created by fresh install** (`EnvironmentFile=-` handles absence, non-secret only) |
| `/etc/synveil/credentials` | `root:root` | `0700` | CREDENTIAL_DIRECTORY | `install.sh` (mkdir 0700, preserved; never seeds secret) |
| `/etc/synveil/credentials/database-url` | `root:root` | `0600` | admin secret | admin (`install -m 0600`) — **not created by fresh install**; systemd `LoadCredential` exposes per-service copy |
| `/var/lib/synveil` | `synveil:synveil` | `0750` | STATE_DIRECTORY | `tmpfiles.d` (not seeded by payload) |
| `/run/synveil` | `synveil:synveil` | `0750` | RUNTIME_MANAGED | `RuntimeDirectory=` |
| `/run/credentials/.../database-url` | `root:synveil` (systemd) | `0400` | — | `LoadCredential` per-activation, unswapped |
| `/var/log/synveil` | — | — | — | **not created** (journald) |

User-data/storage pools, PostgreSQL data, backup destinations, object-store
roots, home data are **never package-owned** and never deleted.

## Scripts

### `install.sh` — fresh install / idempotent reinstall / upgrade

```sh
# Staged (rootless, no host mutation):
./deploy/install/install.sh --root=/tmp/synveil-root --binary=/path/to/synveil-scheduled-maintenance-once

# With DESTDIR env (make-style):
DESTDIR=/tmp/synveil-root ./deploy/install/install.sh --binary=...

# Real host (requires root, explicit allow):
sudo SYNVEIL_ALLOW_HOST_ROOT=1 ./deploy/install/install.sh --root=/ --binary=...
```

Properties:

- `set -euo pipefail`, all paths quoted, no `eval`, no `curl|sh`.
- Validates `STAGED_ROOT` is absolute, no `..` component, not `/` without allow,
  not a symlink.
- Validates each destination stays under `STAGED_ROOT` via `realpath -m -s`
  (lexical) plus parent-dir symlink check — prevents path escape.
- Installs PACKAGE files atomically: `cp` → `chmod` → `chown` (if root) → `mv`
  (atomic rename). No truncate-in-place streaming.
- Creates `/etc/synveil` (`0750 root:synveil`) and `/etc/synveil/credentials`
  (`0700 root:root`) but **does not create or overwrite**
  `synveil-scheduled-maintenance.env` (non-secret) nor `credentials/database-url`
  (secret). Admin creates non-secret via `install -m 0640 -o root -g synveil` and
  secret via `install -m 0600 -o root -g root` under `credentials`. Template at
  `/usr/share/synveil/*.example` is non-secret only. Systemd `LoadCredential`
  exposes secret as `$CREDENTIALS_DIRECTORY/database-url`.
- Does NOT create `/var/lib/synveil` or `/run/synveil`; those are managed by
  `tmpfiles.d` / `RuntimeDirectory=` (Prompt 75). Empty `/var/lib/synveil`
  after fresh install is correct.
- Idempotent: second invocation yields identical PACKAGE checksums/modes.
- Upgrade: replaces PACKAGE files (new binary/units/sysusers/tmpfiles) while
  preserving `/etc/synveil`, `/etc/synveil/credentials/database-url` and
  `/var/lib/synveil` byte-identical.
- Failed upgrade: `SYNVEIL_INSTALL_FAIL_AFTER=N` injects failure after N PACKAGE
  artifacts (test-only). Already-replaced artifacts remain at new version;
  config/state/user-data remain untouched. Full version rollback is **DEFERRED**
  to distro package manager; this layer guarantees **data preservation**, not
  transactional version revert.
- Does NOT run `systemctl`, `systemd-sysusers`, `systemd-tmpfiles`, or
  `daemon-reload` on staged root; it logs the expected host ordering for
  packaging.

### `uninstall.sh` — ordinary uninstall vs explicit purge

```sh
# Ordinary uninstall (preserves config/state):
./deploy/install/uninstall.sh --root=/tmp/synveil-root

# Explicit purge (destructive — requires flag):
./deploy/install/uninstall.sh --root=/tmp/synveil-root --purge
```

- Validates staged root like install.
- Removes only PACKAGE artifacts by unlinking known paths (lexical check,
  `rm -f` without following symlink target). Never `rm -rf` parent dirs.
- **Symlink safety**: if a PACKAGE path is a symlink to outside, only the link
  is unlinked.
- **Parent safety**: never removes `/usr/bin`, `/usr/lib/...`, `/etc`,
  `/var/lib` themselves.
- **Ordinary uninstall** preserves `/etc/synveil` (including
  `/etc/synveil/credentials/database-url`), `/var/lib/synveil`, external pools,
  PostgreSQL, and retains the `synveil` account (avoids orphaned UID). This is
  the correct reinstall-after-uninstall path.
- **`--purge`** (explicit) additionally removes `/etc/synveil` (including
  credentials) and `/var/lib/synveil` after safety checks (target must be under
  root, must be in allowlist, symlink targets are not followed). Still **never**
  deletes external user-data/storage pools, backup destinations, object-store
  roots, or PostgreSQL data.
- Account removal is **not** automated; purge logs that `userdel synveil` is
  safe only after confirming no orphaned files remain.
- Never uses `sudo` internally; privilege is external.

## Lifecycle examples

```sh
# Fresh staged install
BINARY=/tmp/fixture-bin ./deploy/install/install.sh --root=/tmp/root
# Idempotent
./deploy/install/install.sh --root=/tmp/root --binary=/tmp/fixture-bin
# Upgrade (new binary)
./deploy/install/install.sh --root=/tmp/root --binary=/tmp/fixture-bin-v2
# Failed upgrade simulation (partial):
SYNVEIL_INSTALL_FAIL_AFTER=2 ./deploy/install/install.sh --root=/tmp/root --binary=/tmp/fixture-bin-v3 || echo "partial, data preserved"
# Ordinary uninstall (config/state preserved)
./deploy/install/uninstall.sh --root=/tmp/root
# Reinstall after uninstall (reuses preserved config/state)
./deploy/install/install.sh --root=/tmp/root --binary=/tmp/fixture-bin
# Purge (explicit)
./deploy/install/uninstall.sh --root=/tmp/root --purge   # removes /etc/synveil, /var/lib/synveil
# Purge still preserves external pools under /srv, /mnt, etc.
```

## Host ordering (real machine, not staged test)

```
install package-owned files
  → systemd-sysusers --root=/  (reads /usr/lib/sysusers.d/synveil.conf)
  → systemd-tmpfiles --create --root=/  (creates /var/lib/synveil)
  → systemctl daemon-reload
  → systemctl enable synveil-scheduled-maintenance.timer  # explicit, not automatic
  # service is NOT auto-started by package-neutral layer
```

Uninstall ordering (real host):

```
systemctl disable --now synveil-scheduled-maintenance.timer
systemctl stop synveil-scheduled-maintenance.service  # allow bounded completion, don't kill healthy cycle
<remove PACKAGE artifacts via package manager or uninstall.sh --root=/ >
systemctl daemon-reload
# purge only with explicit admin confirmation
```

## Upgrade while service is active

Unix semantics allow the already-running `synveil-scheduled-maintenance-once`
process image to continue while `/usr/bin/synveil-scheduled-maintenance-once`
is atomically replaced. No kill is issued merely to replace the executable.
Next timer activation uses the new binary. This mirrors package-manager
behavior.

## Rollback contract

- **Application-data safety**: LOCKED — `/etc/synveil` (including
  `/etc/synveil/credentials/database-url`), `/var/lib/synveil`, external pools,
  PostgreSQL are never deleted by upgrade/uninstall; failed upgrade leaves them
  intact.
- **Package-version rollback**: DEFERRED to distribution packaging / Synveil OS
  image lifecycle. This layer does not implement a custom package database or
  version revert; a partially upgraded PACKAGE set may remain after injected
  failure, and the package manager must provide transactional guarantees.

## Tests

`crates/metadata/tests/linux_install_lifecycle.rs` exercises the 18 required
behaviors via temporary roots and `bash` invocation (no root, no host mutation,
no PostgreSQL). See `docs/en/DEPLOYMENT.md` for the full manifest and policy.

# ADR-026: Systemd sandbox hardening for Gen-1 scheduled-maintenance service / ADR-026: Vỏ bọc systemd cho dịch vụ bảo trì lên lịch Gen-1

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED for Gen-1 Linux scheduled-maintenance deployment**
- Date / Ngày: 2026-09-05
- Owners / Chủ sở hữu: Platform / Distribution, Security, Release, Architecture

## Context (English)

Prompt 78 mandates evidence-driven systemd sandboxing for the Gen-1 Linux scheduled-maintenance one-shot runtime. The service (`/usr/bin/synveil-scheduled-maintenance-once`) executes with `User=synveil`, requires PostgreSQL connectivity (TCP, Unix socket), uses systemd credential delivery, and must be hardened against privilege escalation, unauthorized filesystem access, device access, and kernel modification.

Key requirements from Prompt 78:
- Runtime capability inventory shows 0 required capabilities
- Runtime writes should be 0 (no filesystem changes)
- Network access limited to AF_UNIX, AF_INET, AF_INET6 for PostgreSQL
- No device access required
- Minimal /proc access needed
- No kernel modification, mount operations, or namespace creation required

## Decision (English)

The `synveil-scheduled-maintenance.service` unit implements a comprehensive systemd sandbox with the following hardening directives:

### Privilege Controls
- `NoNewPrivileges=yes` — prevents setuid/setgid privilege escalation
- `RestrictSUIDSGID=yes` — blocks setuid/setgid binary execution
- `CapabilityBoundingSet=` (empty) — no Linux capabilities granted
- `AmbientCapabilities=` (empty) — no ambient capabilities

### Filesystem Hardening
- `ProtectSystem=strict` — makes /usr, /boot, /efi, /etc, /var read-only; hides /home, /root
- `ProtectHome=yes` — hides all user home directories
- `PrivateTmp=yes` — isolates /tmp for the service
- `PrivateDevices=yes` combined with `DevicePolicy=closed` — denies raw device access
- `InaccessiblePaths=/etc/synveil/credentials` — explicitly protects credential source
- `UMask=0077` — restrictive file creation mask

### Kernel Protection
- `ProtectKernelTunables=yes` — prevents modification of /proc/sys
- `ProtectKernelModules=yes` — prevents module loading/unloading
- `ProtectKernelLogs=yes` — prevents kernel log access
- `ProtectControlGroups=yes` — prevents cgroup manipulation

### Process Restrictions
- `ProtectProc=invisible` — hides processes not owned by the service
- `ProcSubset=pid` — limits /proc to PID information only
- `RestrictNamespaces=yes` — blocks new namespace creation
- `RestrictRealtime=yes` — prevents realtime scheduling
- `LockPersonality=yes` — prevents personality changes

### Network Restrictions
- `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6` — allows only PostgreSQL network access

### Memory Protection
- `MemoryDenyWriteExecute=yes` — prevents W^X memory mappings

### Syscall Filtering
- `SystemCallFilter=@system-service` — base allowlist
- Denied classes: `@clock`, `@cpu-emulation`, `@debug`, `@module`, `@mount`, `@raw-io`, `@reboot`, `@swap`, `@privileged`, `@obsolete`, `@resources`
- `SystemCallErrorNumber=EPERM` — permission denied instead of crashing

### Additional Settings
- `WorkingDirectory=/` — no writable working directory
- `RuntimeDirectory=synveil` — ephemeral state directory (unused by current runtime)
- `SystemCallArchitectures=native` — only native syscalls

### Credential Isolation
- Service uses `LoadCredential=` for database URL (systemd-mediated)
- Source `/etc/synveil/credentials/database-url` is `root:root 0600`
- Runtime never reads source directly; uses systemd-delivered credential

## Consequences / Hệ quả (English)

### Positive
- Attack surface significantly reduced for any compromised runtime process
- No capability for system modification, kernel manipulation, or privilege escalation
- Filesystem writes prohibited (ProtectSystem=strict with no ReadWritePaths)
- Device access denied prevents any hardware-level attacks
- Syscall filtering prevents dangerous operations

### Monitoring
- Hardening is validated by `systemd-analyze verify` (syntax), `systemd-analyze security` (score)
- Runtime tests (one-shot, stress, lifecycle) verify functional correctness

### Known Limitations
- Root remains trusted administrator (systemd credentials visible to PID 1)
- No protection against host-level compromise
- `RestrictNamespaces=yes` may interact poorly with some container runtimes
- `ProcSubset=pid` may hide legitimate debugging info in some edge cases

## Rejected / Deferred Alternatives / Các phương án bị từ chối / hoãn

- **Broader RuntimeDirectory ownership** — rejected; current runtime doesn't need /run/synveil
- **ReadWritePaths for /run/synveil or /var/lib/synveil** — rejected; 0 filesystem writes demonstrated, no hypothetical permissions granted early
- **ProtectHost=yes** — not applicable; service doesn't need host sysctls
- **PrivateUsers=** — deferred; persistent `synveil` ownership interacts poorly with user namespaces
- **IPAddressDeny=any + IPAddressAllow=** — deferred; DB endpoint is a packaging choice, no localhost assumption baked into the unit
- **ProtectClock= / ProtectHostname=** — deferred; outside the mandatory baseline, exposure already 1.4 OK on systemd 261.2, no score-only churn
- **RestrictFileSystems=** — deferred; brittle whitelist with no demonstrated benefit for this workload
- **PrivateNetwork=yes** — rejected; breaks PostgreSQL TCP
- **Any Linux capability** — rejected; measured zero requirement
- **LoadCredentialEncrypted=** — deferred future; plain LoadCredential is LOCKED Gen-1 (see ADR-025)

## Implementation Evidence / Bằng chứng thực thi

### Runtime Requirements Inventory
| Requirement | Status | Evidence |
|---|---|---|
| Filesystem read | ✅ | Executable, libs, migrations, credentials loaded via systemd |
| Filesystem write | ❌ | None |
| Network AF_UNIX | ✅ | PostgreSQL local socket support |
| Network AF_INET | ✅ | PostgreSQL TCP support |
| Network AF_INET6 | ✅ | PostgreSQL TCP over IPv6 |
| Device access | ❌ | None needed |
| /proc access | ⚠️ | Minimal (sqlx/tokio reads /proc/self/maps) |
| Capabilities | ❌ | Zero required for PostgreSQL |
| Namespace creation | ❌ | None needed |

### Test Validation
- `systemd-analyze verify` (service + timer) — PASS
- `systemd-analyze security` exposure — 4.5 (Prompt-77-era) → 1.4 OK on systemd 261.2 (hardened); informational, correctness authoritative
- Real systemd-managed execution of the production one-shot binary under the
  identical sandbox directives against disposable PostgreSQL 17.6 — PASS:
  idle exit 0 (`tick Idle worker Idle`); due work exit 0 (one
  `MaterializedAndHandedOff` + one `Stepped` `CREATED→SNAPSHOT_CAPTURED`);
  existing work exit 0 (`tick Idle` + one worker step)
- Negative probes under the identical sandbox — PASS: writes to `/usr/bin`,
  `/etc`, `/run/user` denied; home-sentinel read denied; `/dev/kmsg` denied;
  `InaccessiblePaths` mechanism denied; private `/dev` minimal;
  `CapInh/Prm/Eff/Bnd/Amb` all zero, `NoNewPrivs=1`, unprivileged UID
- `service_has_no_privilege_escalation_directives` (Prompt 75) — PASS
- `service_hardening_is_conservative_and_allows_postgres` — PASS
- `systemd_service_contains_credential_delivery_and_no_secret_env` — PASS
- Static sandbox contract — `crates/metadata/tests/linux_sandbox_hardening_units.rs`

### Operational notes
- `InaccessiblePaths=` requires its target to exist at activation: a missing
  `/etc/synveil/credentials` skeleton fails closed (`226/NAMESPACE` before
  `ExecStart`). Packaging always creates it (`CREDENTIAL_DIRECTORY` in
  `deploy/install/MANIFEST`), so production is unaffected; the fail-closed
  behavior was observed directly during gate validation.
- `@clock` in the syscall policy covers only time-setting syscalls;
  `clock_gettime`/`nanosleep` used by tokio stay allowed via `@system-service`.
- The `~@clock` / `~@resources` denies were retained only after the real
  binary passed idle + due + existing-work gates under the exact policy.

## Migration and Review Trigger / Điều kiện di chuyển và xem xét lại

- If future runtime changes require filesystem writes, add `ReadWritePaths=` for specific paths under `ProtectSystem=strict`
- If namespace creation is needed, consider `RestrictNamespaces=` with specific namespaces blocked
- If additional syscalls are required, update `SystemCallFilter` with documented exceptions
- If device access becomes necessary, revisit `PrivateDevices` and `DevicePolicy`

## Quyết định (Tiếng Việt)

Dịch vụ `synveil-scheduled-maintenance.service` triển khai vỏ bọc systemd toàn diện với các directive bảo mật:

### Kiểm soát quyền
- `NoNewPrivileges=yes` — ngăn escalation qua setuid/setgid
- `RestrictSUIDSGID=yes` — chặn thực thi binary setuid/setgid
- `CapabilityBoundingSet=` (rỗng) — không cấp quyền Linux
- `AmbientCapabilities=` (rỗng) — không có ambient capabilities

### Bảo mật filesystem
- `ProtectSystem=strict` — ghi đèn /usr, /boot, /efi, /etc, /var; ẩn /home, /root
- `ProtectHome=yes` — ẩn tất cả thư mục home người dùng
- `PrivateTmp=yes` — cách ly /tmp
- `PrivateDevices=yes` + `DevicePolicy=closed` — từ chối truy cập thiết bị raw
- `InaccessiblePaths=/etc/synveil/credentials` — bảo vệ source credential
- `UMask=0077` — file creation mask hạn chế

### Bảo vệ kernel
- Các directive bảo vệ kernel (Tunables, Modules, Logs, ControlGroups)

### Hạn chế quy trình
- Các directive hạn chế process (ProtectProc, ProcSubset, RestrictNamespaces, RestrictRealtime, LockPersonality)

### Hạn chế mạng
- `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6` — chỉ PostgreSQL

### Bảo vệ bộ nhớ
- `MemoryDenyWriteExecute=yes` — không cho W^X

### Bộ lọc syscall
- `SystemCallFilter=@system-service` + các lớp bị từ chối

### Isolate credential
- Runtime dùng `LoadCredential=` với source `root:root 0600`

## Hạn chế / Hạn chế đã biết

- Root vẫn là người quản trì tin cậy (systemd credentials nhìn thấy bởi PID 1)
- Không bảo vệ chống compromise host level
- `RestrictNamespaces=yes` có thể xung đột với container runtime

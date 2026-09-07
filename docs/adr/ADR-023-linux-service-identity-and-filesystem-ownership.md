# ADR-023: Linux service identity and filesystem ownership / Identity dịch vụ Linux và ownership filesystem

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED for current Linux Gen-1 deployment architecture**
- Date / Ngày: 2026-09-05
- Owners / Chủ sở hữu: Platform / Distribution, Security, Release, Architecture

## Context (English)

Prompts 71–74 established a bounded `synveil-scheduled-maintenance-once → DatabasePool → ScheduledMaintenanceCycleRunner` one-shot and an
external `systemd` `Type=oneshot` service + `OnCalendar=*:*:00` timer lifecycle. Prompt 72 intentionally deferred `User=`/`Group=` pending a
dedicated service-account architecture. Production Linux deployment now requires a least-privilege, stable, non-interactive service identity and
an explicit ownership contract for binaries, units, non-secret configuration,
the administrator-controlled credential source, persistent state, and ephemeral
runtime state — without yet implementing full package install/uninstall mechanics (Prompt 76) or finalizing secret delivery (Prompt 77).

Key constraints:
- Runtime must not be `root` and must not modify its own executable or unit files.
- Future deployment will require stable ownership of persistent state, config access, and possibly object-store/data paths.
- `systemd` offers `DynamicUser=yes` (transient UID) and declarative `sysusers.d`/`tmpfiles.d` plus `RuntimeDirectory=`/`StateDirectory=`.
- The portable `crates/core` domain must remain free of Linux identity concepts.

## Decision (English)

**Persistent `synveil` Linux service account is the authoritative Gen-1 first-party runtime identity.**

- **Account:** name `synveil`, group `synveil`, system account, created by packaging via `systemd-sysusers` from `deploy/sysusers.d/synveil.conf`.
- **Properties:** non-login (invalid password), shell `/usr/sbin/nologin` (or distribution `nologin` equivalent), home `/var/lib/synveil`
  (stable state location, not a conventional `/home/*` interactive home), auto-allocated UID/GID (`-` in sysusers, no hardcoded numeric UID),
  no supplementary groups (`sudo`/`wheel`/`docker`/`disk`/`adm`/`root`/`adm` not granted), stable across reboots, suitable for persistent
  state ownership.
- **Creation mechanism:** declarative `systemd-sysusers` (preferred over custom `useradd`/`groupadd` shell logic). Validate with
  `systemd-sysusers --dry-run --root=/tmp/root` without host mutation. Packaging installs to `/usr/lib/sysusers.d/synveil.conf`.
- **UID/GID policy:** allow system allocation; do not assume a particular numeric UID cross-machine. Future appliance images may reserve a
  fixed UID/GID if image construction requires it; Gen-1 documents the non-assumption.
- **Filesystem ownership contract** (also in `deploy/README.md` and `DEPLOYMENT.md`):

| Path | Owner | Mode | Manager | Runtime access |
|---|---|---|---|---|
| `/usr/bin/synveil-*` | `root:root` | `0755` | package | `synveil` can read/exec, not write |
| `/usr/lib/systemd/system/synveil-*.service` / `*.timer` | `root:root` | `0644` | package | not writable |
| `/usr/lib/sysusers.d/synveil.conf` / `/usr/lib/tmpfiles.d/synveil.conf` | `root:root` | `0644` | package | not writable |
| `/etc/synveil` | `root:synveil` | `0750` | package | `synveil` can read required config, not freely rewrite |
| `/etc/synveil/synveil-scheduled-maintenance.env` | `root:synveil` | `0640` | admin/package | non-secret lease tuning via `EnvironmentFile`; database credential is delivered by Prompt 77 `LoadCredential` |
| `/var/lib/synveil` | `synveil:synveil` | `0750` | `tmpfiles.d` (`d` line) | persistent non-secret state; empty today is acceptable |
| `/run/synveil` | `synveil:synveil` | `0750` | `RuntimeDirectory=synveil` | ephemeral per-activation, cleaned on stop |
| `/var/log/synveil` | — | — | — | intentionally not created; journald is authoritative |

- **State/runtime management:**
  - `/var/lib/synveil` → `deploy/tmpfiles.d/synveil.conf` (`d /var/lib/synveil 0750 synveil synveil`). One host-level declaration, shared by
    future services. `StateDirectory=` intentionally not used to avoid per-service conflicting managers for the same path.
  - `/run/synveil` → `RuntimeDirectory=synveil` in the service unit (`RuntimeDirectoryMode=0750`). Lifecycle-tied, not duplicated in
    `tmpfiles.d`.
  - No `tmpfiles.d` entry creates secret-bearing files; storage/object-store roots follow storage architecture, not recursively `chown`'d
    to `synveil`.
- **Service unit:** `deploy/systemd/synveil-scheduled-maintenance.service` now contains `User=synveil`, `Group=synveil`,
  `RuntimeDirectory=synveil`, `RuntimeDirectoryMode=0750`. It never creates users, `chmod`/`chown`s directories, modifies units, invokes
  `systemctl`, heartbeats, renews leases, or escalates.
- **Privilege boundary:** package/install operations are `root`-owned; runtime operates as `synveil` with only `AF_UNIX` + `AF_INET`/`AF_INET6`.
- **Per-service account evaluation:** identities such as `synveil-api`, `synveil-maintenance`, `synveil-gc` were evaluated. All current
  first-party services share a trusted backend/data boundary; separate accounts would add packaging complexity without material least-privilege
  gain. **Gen-1 retains one `synveil` account**; a future capability separation may justify per-service accounts.
- **Compatibility:** `DATABASE_URL` remains `postgres://`/`postgresql://` TCP or `AF_UNIX`; no `peer` authentication tied to the Unix username is
  assumed. GC worker (`synveil-worker`) and future API server may share the same account; no GC/API refactor is performed.
- **Portability:** Linux identity concepts stay outside `crates/core` and portable server APIs.

## Alternatives rejected / Phương án loại bỏ (English + Tiếng Việt)

- **Root runtime / Runtime `root`:** violates least privilege; package install is `root`, runtime must be unprivileged.
  `Runtime `root` vi phạm least privilege; cài đặt là `root`, runtime phải không có quyền.`

- **DynamicUser=yes / `DynamicUser=yes`:** **REJECTED / LOẠI** — transient UID cannot provide stable ownership of `/var/lib/synveil`,
  `/etc/synveil`, or future object-store paths; hides state under `/var/lib/private`/`/run/private` via id-mapped mounts, breaks
  administrator-visible ownership, prevents multiple services sharing one identity. Suitable only for fully ephemeral stateless services.
  `UID tạm thời không thể sở hữu bền `/var/lib/synveil`, `/etc/synveil` hay path object-store tương lai; giấu state ở `/var/lib/private`,
  gãy ownership admin, cản nhiều service chia sẻ identity. Chỉ phù hợp service stateless tạm thời hoàn toàn.`

- **Per-service accounts (`synveil-api`, `synveil-maintenance`, `synveil-gc`) / Account riêng từng service:** deferred — shared trusted
  boundary today; packaging overhead without capability separation.
  `Hoãn — hôm nay mọi service chia sẻ boundary tin cậy; thêm account riêng gây phức tạp đóng gói mà không tách capability thực chất.`

- **Interactive user account / Account user tương tác:** rejected — no login, no `/home/*`, no `sudo`/`wheel`/`docker` etc.
  `Loại — không login, không `/home/*`, không `sudo`/`wheel`/`docker` v.v.`

## Consequences / Hệ quả (English)

- Packaging must invoke `systemd-sysusers` and `systemd-tmpfiles --create` at install (no host `/etc/passwd` mutation in tests; validation uses
  `--dry-run` + `--root=/tmp/root`).
- Any Synveil first-party service that needs persistent state uses `/var/lib/synveil` with `synveil:synveil 0750`; any ephemeral state uses
  `RuntimeDirectory=synveil` in its unit.
- `deploy/sysusers.d/synveil.conf` and `deploy/tmpfiles.d/synveil.conf` are the single declarative sources; custom shell scripts for `useradd`
  are forbidden unless `sysusers` is demonstrably unavailable.
- Security audits assert: runtime ≠ `root`, cannot modify own binary or units, non-secret env is `0640`, database source is `0600 root:root`, no interactive login,
  no broad groups, no arbitrary object-store `chown`, no new privilege escalation path.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

- If a future storage architecture proves durable filesystem ownership is genuinely unnecessary (pure PostgreSQL + external object store with
  no host state), STOP and re-evaluate rather than forcing the persistent account (per prompt stop condition).
- If a measured capability separation between API, maintenance, GC, etc. emerges, supersede this ADR with per-service identities and a migration
  plan for existing `/var/lib/synveil` ownership.
- If `systemd-sysusers`/`tmpfiles` are unavailable on a claimed platform (e.g. non-systemd), produce a platform-specific adapter ADR.
- Do not introduce a database `service_users`/`worker_host` table or inject Linux identity into `crates/core`; that would trigger a superseding ADR.

---

## Quyết định (Tiếng Việt)

**Tài khoản dịch vụ Linux bền `synveil` là identity runtime Gen-1 chính thức cho mọi service first-party.**

- **Account:** tên `synveil`, group `synveil`, system account, tạo bởi packaging qua `systemd-sysusers` từ `deploy/sysusers.d/synveil.conf`.
- **Thuộc tính:** không login (password invalid), shell `/usr/sbin/nologin` (hoặc `nologin` tương đương), home `/var/lib/synveil`
  (vị trí state bền, không phải `/home/*` tương tác), UID/GID cấp phát tự động (`-` trong sysusers, không hardcode số), không nhóm phụ
  (`sudo`/`wheel`/`docker`/`disk`/`adm`/`root` không cấp), bền qua reboot, đủ để sở hữu state bền.
- **Cơ chế tạo:** khai báo `systemd-sysusers` (ưu tiên hơn logic shell `useradd`/`groupadd` tự viết). Kiểm tra bằng
  `systemd-sysusers --dry-run --root=/tmp/root` không đụng host. Packaging cài vào `/usr/lib/sysusers.d/synveil.conf`.
- **Chính sách UID/GID:** để hệ thống cấp phát; không giả định số cụ thể cross-machine. Image appliance tương lai có thể reserve UID/GID cố
  định nếu cần đóng image; Gen-1 ghi rõ không giả định.
- **Contract ownership filesystem** (cũng trong `deploy/README.md` và `DEPLOYMENT.md`):

| Path | Ownership | Mode | Quản lý | Truy cập runtime |
|---|---|---|---|---|
| `/usr/bin/synveil-*` | `root:root` | `0755` | package | `synveil` đọc/exec, không ghi |
| `/usr/lib/systemd/system/synveil-*` | `root:root` | `0644` | package | không ghi |
| `/usr/lib/sysusers.d` / `tmpfiles.d` | `root:root` | `0644` | package | không ghi |
| `/etc/synveil` | `root:synveil` | `0750` | package | `synveil` đọc config cần thiết, không ghi tùy ý |
| `/etc/synveil/*.env` | `root:synveil` | `0640` | admin/package | tuning lease không bí mật; database credential do Prompt 77 phân phối qua `LoadCredential` |
| `/var/lib/synveil` | `synveil:synveil` | `0750` | `tmpfiles.d` | state bền không-bí-mật; hôm nay rỗng OK |
| `/run/synveil` | `synveil:synveil` | `0750` | `RuntimeDirectory=` | tạm thời mỗi lần kích hoạt, xóa khi stop |
| `/var/log/synveil` | — | — | — | cố ý không tạo; dùng journald |

- **Quản lý state/runtime:**
  - `/var/lib/synveil` → `deploy/tmpfiles.d/synveil.conf` (`d` line). Một khai báo cấp host, chia sẻ cho service tương lai. **Không dùng**
    `StateDirectory=` để tránh manager xung đột cho cùng path.
  - `/run/synveil` → `RuntimeDirectory=synveil` trong unit (`RuntimeDirectoryMode=0750`). Gắn lifecycle, không lặp ở `tmpfiles.d`.
  - Không tạo file chứa secret qua `tmpfiles.d`; gốc storage/object-store tuân storage architecture, không `chown` đệ quy sang `synveil`.
- **Unit service:** `deploy/systemd/synveil-scheduled-maintenance.service` nay có `User=synveil`, `Group=synveil`,
  `RuntimeDirectory=synveil`, `RuntimeDirectoryMode=0750`. Không tạo user, không `chmod`/`chown` thư mục hệ thống, không sửa unit, không gọi
  `systemctl`, không heartbeat/renew/leo thang.
- **Ranh giới đặc quyền:** thao tác package/cài đặt là `root`; runtime là `synveil` chỉ với `AF_UNIX` + `AF_INET`/`AF_INET6`.
- **Đánh giá per-service account:** các identity `synveil-api`, `synveil-maintenance`, `synveil-gc` đã được xem xét. Hiện mọi service
  first-party chia sẻ boundary backend/data tin cậy; tách account riêng chỉ thêm phức tạp đóng gói mà không cải thiện least privilege
  thực chất. **Gen-1 giữ một account `synveil`;** tách capability sau này có thể biện minh account riêng.
- **Tương thích:** `DATABASE_URL` vẫn `postgres://`/`postgresql://` TCP hoặc `AF_UNIX`; không giả định xác thực `peer` gắn với Unix username.
  GC worker (`synveil-worker`) và API server tương lai có thể chia sẻ cùng account; không refactor GC/API.
- **Tính di động:** concept identity Linux nằm ngoài `crates/core` và API server portable.

# ADR-024: Linux package lifecycle and data-preserving uninstall / Vòng đời package Linux và uninstall bảo toàn dữ liệu

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED for Gen-1 Linux deployment**
- Date / Ngày: 2026-09-05
- Owners / Chủ sở hữu: Platform / Distribution, Security, Release, Architecture

## Context (English)

Prompts 71–75 delivered the bounded one-shot (`synveil-scheduled-maintenance-once`),
external `systemd` service+timer, and persistent `synveil:synveil` identity with
the ownership contract (`root:root` binaries/units, `root:synveil 0750/0640` config,
`synveil:synveil 0750` state, `RuntimeDirectory=`). No package-neutral lifecycle
existed; distribution packaging (`.deb`/`.rpm`/PKGBUILD/OCI/Synveil OS image) needs
one deterministic staging/install layer that:

- supports `DESTDIR` / staged-root testing without host mutation;
- is idempotent and safely upgrades PACKAGE files;
- never deletes administrator config, persistent state, user pools, or PostgreSQL on
  ordinary uninstall;
- separates explicit purge from ordinary uninstall;
- defends against path escape, symlink traps, and parent-directory removal;
- remains rootless-testable (ownership validated via manifest + mode, not `chown`).

Prompt 77 will own final secret delivery; Prompt 76 must not finalize credentials.

## Decision (English)

**Package-neutral Linux installation lifecycle is the authoritative Gen-1 deployment
foundation for scheduled maintenance; system/package artifacts are replaceable,
config/state/data have separate lifecycles.**

- **Authoritative manifest**: `deploy/install/MANIFEST` enumerates every installed
  path with `source  destination  mode  owner  group  class`. Distribution
  packaging MUST call the package-neutral layer that reads this single manifest;
  duplicated destination logic is forbidden. Classes are `PACKAGE`,
  `CONFIG_DIRECTORY`, `STATE_DIRECTORY`, `RUNTIME_MANAGED` (template is `PACKAGE`
  under `/usr/share`).

- **PACKAGE-owned (root:root, replaced on upgrade, removed on uninstall)**:

  | Destination | Mode | Source |
  |---|---|---|
  | `/usr/bin/synveil-scheduled-maintenance-once` | `0755` | `BINARY` (built from `crates/api/src/bin/synveil-scheduled-maintenance-once.rs`) |
  | `/usr/lib/systemd/system/synveil-scheduled-maintenance.service` | `0644` | `deploy/systemd/*.service` |
  | `/usr/lib/systemd/system/synveil-scheduled-maintenance.timer` | `0644` | `deploy/systemd/*.timer` |
  | `/usr/lib/sysusers.d/synveil.conf` | `0644` | `deploy/sysusers.d/synveil.conf` |
  | `/usr/lib/tmpfiles.d/synveil.conf` | `0644` | `deploy/tmpfiles.d/synveil.conf` |
  | `/usr/share/synveil/synveil-scheduled-maintenance.env.example` | `0644` | `deploy/config/*.env.example` (template) |

- **CONFIG_DIRECTORY**: `/etc/synveil` `0750 root:synveil` created as skeleton
  (mkdir + chown if root). Fresh install does **not** create
  `/etc/synveil/synveil-scheduled-maintenance.env` (policy **A: no file**);
  `EnvironmentFile=-` in the unit handles absence. Admin creates it via
  `install -m 0640 -o root -g synveil` from the template. Reinstall/upgrade
  **preserves** existing file byte-identical; never overwrites working env.

- **STATE_DIRECTORY**: `/var/lib/synveil` `0750 synveil:synveil` is **not**
  created/seeded by the package payload; it is created by `tmpfiles.d` at
  boot/package time (Prompt 75). Package lifecycle never populates fake state.

- **RUNTIME_MANAGED**: `/run/synveil` `0750 synveil:synveil` via
  `RuntimeDirectory=synveil`; not in `tmpfiles.d`.

- **Not package-owned (never deleted)**:
  object/storage pools, backup destinations, mounted volumes, home data,
  PostgreSQL data, `/var/log/synveil` (not created; journald).

- **Package-neutral interface**: `deploy/install/install.sh --root=<staged-root>
  --binary=<path>` and `deploy/install/uninstall.sh --root=<staged-root>
  [--purge]`, plus `DESTIR` env, `common.sh` helpers. Requirements: absolute
  staged root, no `..` component, not `/` without `SYNVEIL_ALLOW_HOST_ROOT=1`,
  lexical + parent-realpath containment (`realpath -m -s`), `set -euo pipefail`,
  quoted paths, no `eval`/`curl|sh`, atomic `cp+chmod+chown→mv` (no truncate
  in place), non-interactive, no `sudo` internally, no `systemctl` on staged
  root (logs future host ordering).

- **Staged-root model**: all install/remove mechanics support alternate root
  (`DESTDIR=/tmp/synveil-root`). Tests operate on disposable `/tmp/.../root`
  trees, never on `/usr`/`/etc`/`/var`/`/run` of the developer host.

- **Idempotent install**: running install twice succeeds, does not duplicate,
  preserves `PACKAGE` checksums/modes; `CONFIG_DIRECTORY` not overwritten.

- **Upgrade**: re-invokes install with newer PACKAGE content; atomically replaces
  PACKAGE files while preserving `/etc/synveil` and `/var/lib/synveil`. Unix
  executable semantics allow a running bounded one-shot to finish while the
  binary path is replaced; next timer activation uses new binary.

- **Failed upgrade**: testable via `SYNVEIL_INSTALL_FAIL_AFTER=N` (injects
  failure after N PACKAGE artifacts). Data invariant: admin config, persistent
  state, external user-data remain untouched. Package-owned version may be
  **partially updated**; full transactional version rollback is **DEFERRED** to
  distro packaging / Synveil OS image lifecycle. This is documented honestly.

- **Rollback contract**:

  | Aspect | Guarantee |
  |---|---|
  | Application-data safety | **LOCKED** — config/state/external DB never deleted |
  | Package-version rollback | **DEFERRED** — no custom package DB; future packaging provides transactions |

- **Ordinary uninstall** removes only PACKAGE artifacts (unlink known paths,
  symlink-safe, no recursive parent removal). Preserves `/etc/synveil`,
  `/var/lib/synveil`, external pools, PostgreSQL, parent dirs, and retains
  `synveil` account (avoids orphaned UID). Reinstall after ordinary uninstall is
  a first-class recovery path (restores PACKAGE, reuses preserved state).

- **Purge**: destructive, **explicit** `--purge` flag only. Removes
  `/etc/synveil` and `/var/lib/synveil` after allowlist + lexical containment +
  symlink-unlink checks. Still **never** deletes external pools, backup
  destinations, object-store roots, or PostgreSQL. Account deletion is not
  automated; purge logs that `userdel synveil` is safe only after confirming no
  orphaned files remain.

- **Safety**:

  - Path escape: validate staged root and each destination before write/remove.
  - Symlink: unlink PACKAGE links without following; purge unlinks symlink
    config/state dirs without traversing target.
  - Parent: never `rm -rf` shared parents (`/usr/bin`, `/usr/lib/...`, `/etc`,
    `/var/lib`).

- **Ordering (real host, documented not executed in staged test)**:
  `install files → systemd-sysusers → systemd-tmpfiles --create → daemon-reload
  → enable timer (explicit)`; uninstall: `disable timer → stop oneshot (allow
  bounded completion) → remove PACKAGE → daemon-reload`.

- **Portability**: install concepts (`DESTDIR`, systemd, `UID`/`GID`, `/usr`)
  remain outside `crates/core`.

## Consequences / Hệ quả (English)

- One MANIFEST drives all packaging and tests; drift is impossible without editing
  the manifest.
- Data loss on uninstall is structurally prevented: ordinary `uninstall` keeps
  config/state; only `uninstall --purge` deletes them, and never external data.
- Staged-root testing is rootless and host-safe; CI can validate without `sudo`.
- Upgrade is safe for bounded one-shot semantics; long-running cycles are not
  killed merely to replace the binary.
- Packaging can add transactional guarantees later without changing application
  semantics; no custom package DB is introduced.
- Secrets are not handled here: fresh install leaves env absent; template is
  `0644` under `/usr/share`, not a placeholder `DATABASE_URL`.

## Alternatives rejected / Phương án loại bỏ (English + Tiếng Việt)

- **Uninstall deletes everything / Uninstall xóa mọi thứ**: **REJECTED / LOẠI** — would violate ADR-021 data-preserving lifecycle; recovery would require full restore for a simple package replace.
  `LOẠI — vi phạm lifecycle bảo toàn dữ liệu của ADR-021.`
- **Custom package manager / Tự viết package manager riêng**: **REJECTED / LOẠI** — dependency resolution, version solver, signature format are deferred to distro tooling.
  `LOẠI — để distro lo.`
- **State stored under /usr or with binary / State để dưới /usr**: **REJECTED / LOẠI** — violates FHS and Prompt 75 ownership; state must be under `/var/lib` via `tmpfiles.d`.
  `LOẠI — vi phạm FHS và Prompt 75.`
- **Install auto-creates working env with placeholder DATABASE_URL / Tự tạo env với DATABASE_URL giả**: **REJECTED / LOẠI** — insecure default or production pointer; fresh install leaves file absent, template under `/usr/share`.
  `LOẠI — tạo default không an toàn.`
- **Transactional rollback in shell layer / Rollback transaction trong shell**: **REJECTED / LOẠI** — would require a custom DB; honest partial-update + data preservation is correct; package manager owns transactions.
  `LOẠI — cần DB riêng; để package manager lo.`

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

- If a future service adds PACKAGE artifacts (new binary/unit), update `MANIFEST`
  and `crates/metadata/tests/linux_install_lifecycle.rs`; do not duplicate logic.
- If secret delivery (Prompt 77) changes env file handling, supersede the
  CONFIG_DIRECTORY policy without weakening data preservation.
- If storage architecture requires package-owned state under new path, add an ADR
  that keeps external pools outside lifecycle and proves no recursive delete.
- If per-service accounts supersede single `synveil` account, update ownership
  columns and purge/account policy.

---

## Quyết định (Tiếng Việt)

**Lifecycle cài đặt Linux package-neutral là nền tảng triển khai Gen-1 cho
scheduled maintenance; artifact hệ thống/package có thể thay thế, config/state/
data có lifecycle riêng.**

- **Manifest chuẩn**: `deploy/install/MANIFEST` liệt kê mọi path với
  `source destination mode owner group class`. Packaging phân phối **phải** gọi
  layer package-neutral đọc manifest duy nhất này; cấm logic destination trùng lặp.
  Classes: `PACKAGE`, `CONFIG_DIRECTORY`, `STATE_DIRECTORY`, `RUNTIME_MANAGED`
  (template là `PACKAGE` dưới `/usr/share`).

- **PACKAGE-owned (root:root, thay khi upgrade, xóa khi uninstall)**:

  | Destination | Mode | Nguồn |
  |---|---|---|
  | `/usr/bin/synveil-scheduled-maintenance-once` | `0755` | `BINARY` (build từ `crates/api/src/bin/...`) |
  | `/usr/lib/systemd/system/*.service` / `*.timer` | `0644` | `deploy/systemd/*` |
  | `/usr/lib/sysusers.d/synveil.conf` | `0644` | `deploy/sysusers.d/*` |
  | `/usr/lib/tmpfiles.d/synveil.conf` | `0644` | `deploy/tmpfiles.d/*` |
  | `/usr/share/synveil/*.env.example` | `0644` | `deploy/config/*.example` (template) |

- **CONFIG_DIRECTORY**: `/etc/synveil` `0750 root:synveil` tạo skeleton.
  Cài mới **không** tạo `synveil-scheduled-maintenance.env` (chính sách **A**);
  `EnvironmentFile=-` xử lý thiếu file. Admin tạo bằng `install -m 0640`. Reinstall/
  upgrade giữ nguyên file byte-identical.

- **STATE_DIRECTORY**: `/var/lib/synveil` `0750 synveil:synveil` **không** do
  payload package tạo/seed; do `tmpfiles.d` tạo khi boot. Không seed fake state.

- **RUNTIME_MANAGED**: `/run/synveil` `0750 synveil:synveil` qua
  `RuntimeDirectory=`; không trong `tmpfiles.d`.

- **Không thuộc package (không bao giờ xóa)**: pool storage/object, nơi backup,
  volume mount, home data, PostgreSQL, `/var/log/synveil` (không tạo; dùng journald).

- **Interface package-neutral**: `deploy/install/install.sh --root=<root>
  --binary=<path>` và `uninstall.sh --root=<root> [--purge]` + `DESTDIR`,
  `common.sh`. Yêu cầu: root tuyệt đối, không `..`, không `/` nếu thiếu
  `SYNVEIL_ALLOW_HOST_ROOT=1`, containment `realpath -m -s` + check parent
  symlink, `set -euo pipefail`, quote path, không `eval`, atomic `mv`.

- **Model staged-root**: mọi thao tác hỗ trợ root thay thế
  (`DESTDIR=/tmp/...`). Test chạy trên cây tạm `/tmp/.../root`, không đụng
  `/usr`/`/etc`/`/var` host.

- **Cài lại idempotent**: chạy hai lần thành công, không duplicate, checksum/mode
  `PACKAGE` không đổi.

- **Upgrade**: gọi lại install với nội dung PACKAGE mới; thay PACKAGE atomically
  trong khi giữ `/etc/synveil` và `/var/lib/synveil`. Process one-shot đang chạy
  được phép kết thúc trong khi binary path bị thay.

- **Upgrade lỗi**: kiểm tra qua `SYNVEIL_INSTALL_FAIL_AFTER=N`. Bất biến dữ liệu:
  config/state/user-data còn nguyên. Version PACKAGE có thể **cập nhật một phần**;
  rollback transaction hoàn chỉnh **HOÃN** cho packaging distro.

- **Contract rollback**: data safety **LOCKED**, rollback version **DEFERRED**.

- **Uninstall thường** chỉ xóa PACKAGE (unlink, an toàn symlink, không xóa parent
  đệ quy). Giữ `/etc/synveil`, `/var/lib/synveil`, pool ngoài, PostgreSQL, giữ
  account `synveil`. Reinstall sau uninstall là path khôi phục hạng nhất.

- **Purge**: phá hủy, chỉ với flag `--purge` tường minh. Xóa `/etc/synveil` và
  `/var/lib/synveil` sau allowlist + containment + xử lý symlink. Vẫn **không**
  xóa pool ngoài, backup, object-store, PostgreSQL. Không tự xóa account.

- **An toàn**: escape path, symlink, parent directory như mô tả.

- **Thứ tự (host thực, không chạy trong staged test)**: install → sysusers →
  tmpfiles → daemon-reload → enable timer; uninstall: disable → stop → remove → reload.

- **Portability**: concept install nằm ngoài `crates/core`.

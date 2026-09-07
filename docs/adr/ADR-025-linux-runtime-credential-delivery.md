# ADR-025: Linux runtime credential delivery via systemd LoadCredential / Phân phối credential runtime Linux qua systemd LoadCredential

- Status / Trạng thái: **Accepted / Chấp thuận — LOCKED for Gen-1 Linux systemd deployment**
- Date / Ngày: 2026-09-05
- Owners / Chủ sở hữu: Platform / Distribution, Security, Release, Architecture

## Context (English)

Prompt 75 locked a persistent `synveil:synveil` least-privilege service identity.
Prompt 76 established a package-neutral install lifecycle with `root:root` PACKAGE,
`root:synveil` CONFIG, `synveil:synveil` STATE, and `RuntimeDirectory=`. The
transitional secret model used `EnvironmentFile=-/etc/synveil/synveil-scheduled-
maintenance.env` (`0640 root:synveil`) containing `DATABASE_URL`. This has two
weaknesses:

1. The `synveil` runtime must read the administrator secret source directly
   (`root:synveil` group), widening the read surface.
2. Secrets in `EnvironmentFile` may become visible via process environment
   debugging surfaces (`/proc`, core dumps, service debugging) depending on
   privileges/OS.

Gen-1 needs a production credential model that removes the need for `synveil`
to read the admin source and avoids `DATABASE_URL` in process environment,
without adding an external secret manager or new daemon/watcher, while retaining
a safe development/manual fallback.

## Decision (English)

**Gen-1 Linux production runtime receives the database secret through systemd
credential delivery (`LoadCredential=`), not through `EnvironmentFile`.**

- **Administrator source** (host): `/etc/synveil/credentials/database-url`
  `0600 root:root`, directory `/etc/synveil/credentials` `0700 root:root`.
  Created as skeleton by package-neutral install (`CREDENTIAL_DIRECTORY` in
  `MANIFEST`); fresh install never invents a secret. Rotated atomically
  (write adjacent `0600` temp file + `chmod` + `rename`).

- **Systemd delivery** (PID 1): `deploy/systemd/synveil-scheduled-maintenance.
  service` contains:

  ```
  LoadCredential=database-url:/etc/synveil/credentials/database-url
  Environment=SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url
  EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env  # non-secret only
  ```

  `%d` is the systemd credentials-directory specifier (systemd 261,
  `man systemd.exec` `CREDENTIALS` / `man systemd.unit` `%d`). At runtime it
  expands to `$CREDENTIALS_DIRECTORY` (e.g. `/run/credentials/synveil-
  scheduled-maintenance.service`). The credential file appears as
  `$CREDENTIALS_DIRECTORY/database-url` read-only `0400`, placed in unswapped
  memory, accessible only to `synveil` (and root). The non-secret env
  `SYNVEIL_DATABASE_CREDENTIAL_FILE` carries the **path**, not the secret.

- **Runtime helper** (`crates/api/src/runtime_database_credential.rs`,
  runtime edge, not `crates/core`):

  - `CREDENTIAL_FILE_ENV = SYNVEIL_DATABASE_CREDENTIAL_FILE`
  - `CREDENTIALS_DIRECTORY_ENV = CREDENTIALS_DIRECTORY`
  - `CREDENTIAL_ID = database-url`
  - `MAX_CREDENTIAL_FILE_SIZE = 8192` (8 KiB, bounded read)

  Precedence:
  1. Explicit credential file (`SYNVEIL_DATABASE_CREDENTIAL_FILE` non-empty or
     `$CREDENTIALS_DIRECTORY/database-url` if `CREDENTIALS_DIRECTORY` is set)
  2. Development fallback `DATABASE_URL` (only if no credential file is
     configured)

  **Dual-source policy B (chosen):** if both a credential file path and
  `DATABASE_URL` are set, fail closed with `database credential is ambiguous:
  both credential file and DATABASE_URL are set; use only one` (no secret in
  message). Do not silently combine or prefer one silently; operational clarity
  outweighs quiet fallback. Empty `SYNVEIL_DATABASE_CREDENTIAL_FILE` after trim
  is treated as not set.

  File handling:
  - `metadata.len` fast oversized check + post-read `len > 8 KiB` check.
  - `fs::read` bounded; directory is `UnreadableCredentialFile`.
  - Empty or whitespace-only (after trimming single trailing `\n`/`\r\n`) →
    `EmptyCredential`.
  - UTF-8 required; otherwise `InvalidDatabaseUrl`.
  - Single trailing `\n` or `\r\n` is stripped (common admin file); interior
    content is never rewritten; arbitrary spaces are not trimmed.
  - Validation via canonical `DatabaseConfig::from_url`; `InvalidDatabaseUrl`
    maps to `database configuration is invalid: ...` without secret.

  Errors are generic and never contain the credential:

  ```
  database credential is missing
  database credential is ambiguous: both credential file and DATABASE_URL are set; use only one
  database credential file is unreadable
  database credential is empty
  database credential exceeds maximum size (8192 bytes)
  database configuration is invalid: database URL must use the PostgreSQL scheme
  ```

  The helper provides `load_database_url_from_runtime_source()` and
  `database_config_from_runtime()` (which calls `DatabaseConfig::from_url`
  once). No secret is logged, cached, or persisted.

- **One-shot integration**: `crates/api/src/bin/synveil-scheduled-maintenance-
  once.rs` now calls `database_config_from_runtime()` **before**
  `Timestamp::now_utc()` (still exactly once) and before
  `MigrationRunner::run` / `run_one_scheduled_backup_maintenance_cycle`
  (still exactly once per process). Missing credential → `configuration error:
  database credential invalid: ...` → `process::exit(1)` before any tick/worker.

- **EnvironmentFile role**: retained for non-secret
  `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS=120` (and future non-secret
  tuning). Production `synveil-scheduled-maintenance.env.example` contains no
  `DATABASE_URL=postgresql://` functional line; it documents `LoadCredential`.

- **Installer manifest**: `deploy/install/MANIFEST` adds:

  ```
  -  /etc/synveil/credentials  0700  root  root  CREDENTIAL_DIRECTORY
  ```

  `install.sh` creates both `/etc/synveil` (`0750 root:synveil`) and
  `/etc/synveil/credentials` (`0700 root:root`) as skeletons, never invents
  `database-url`. `uninstall.sh` ordinary preserves credentials; `--purge`
  removes `/etc/synveil` including `credentials/*` after `realpath -m -s`
  + allowlist + symlink-unlink checks (still never deletes external pools/DB).

- **Rotation**: admin does `install -m 0600 -o root -g root /tmp/new-url
  /etc/synveil/credentials/database-url` (or tmpfile + `chmod` + `mv`);
  next timer activation receives new credential. One-shot is stateless, no daemon
  restart, no watcher, no polling.

- **systemd-creds evaluation**: `LoadCredentialEncrypted=` / `systemd-creds`
  is a **stronger optional future**; Gen-1 **plain `LoadCredential=` is LOCKED**
  as baseline. Encrypted credentials are **SUPPORTED FUTURE / DEFERRED** because
  they require TPM2 or provisioned host key and `systemd-creds` tooling not yet
  in minimal deployment target. Documented as future option; no encrypted
  credential is required for Gen-1.

- **Portability**: `LoadCredential`, `CREDENTIALS_DIRECTORY`, `/etc/synveil`
  remain outside `crates/core` and portable `crates/metadata` domain types.

## Consequences / Hệ quả (English)

- `synveil` no longer needs read on `/etc/synveil/credentials`; attack surface
  is reduced from `root:synveil 0640` to `root:root 0600/0700` with PID 1
  mediating a `0400` per-service copy.
- Secrets are not placed in `EnvironmentFile` or process environment variable
  value; only the non-secret path `%d/...` is in `Environment=`. This mitigates
  `/proc` environment visibility (systemd credentials are unswapped, read-only).
- Single trailing newline handling matches admin-managed secret file conventions
  without interior rewrite.
- Bounded 8 KiB prevents unbounded read; oversized fails closed.
- Dual-source ambiguity fails closed, preventing silent fallback to development
  `DATABASE_URL` when a credential file was intended.
- Fresh install declares credential directory but never invents a secret;
  `database credential is missing` before DB/migration is the safe default.
- Upgrade/reinstall/ordinary-uninstall preserve credential byte-identical;
  purge is explicit.
- Development remains ergonomic via `DATABASE_URL` fallback when no credential
  file is configured.
- No new daemon, watcher, poll loop, heartbeat, or cache is introduced.
- `crates/core` stays systemd-free; `crates/metadata::DatabaseConfig::from_url`
  remains the single validator.

## Alternatives rejected / Phương án loại bỏ (English + Tiếng Việt)

- **EnvironmentFile with DATABASE_URL / EnvironmentFile chứa DATABASE_URL**: **REJECTED / LOẠI** — requires `synveil` to read admin source (`root:synveil 0640`), secret may leak via `proc` or debugging, not least privilege.
  `LOẠI — cần synveil đọc source admin, có thể lộ qua proc.`

- **Direct root:synveil 0640 secret file read by service / File secret root:synveil 0640 do service đọc trực tiếp**: **REJECTED / LOẠI** — stronger than world-readable but still widens read surface; PID 1 mediation is preferred where systemd is available.
  `LOẠI — vẫn mở rộng read surface; PID 1 trung gian tốt hơn.`

- **systemd LoadCredentialEncrypted / systemd LoadCredentialEncrypted**: **DEFERRED / HOÃN** — stronger (authenticated encryption) but requires host key/TPM2 and `systemd-creds` provisioning not justified for Gen-1 minimal target. Supported future.
  `HOÃN — mạnh hơn nhưng cần host key/TPM2, để tương lai.`

- **External secret manager (Vault/AWS/1Password/KS) / Secret manager ngoài**: **REJECTED / LOẠI** — introduces required external service dependency; Gen-1 remains self-contained.
  `LOẠI — tạo phụ thuộc ngoài bắt buộc.`

- **Custom file watcher / polling for rotation / Watcher file tùy chỉnh**: **REJECTED / LOẠI** — one-shot is stateless; next activation naturally picks new file; no daemon needed.
  `LOẠI — one-shot vô trạng thái, lần kích hoạt sau tự lấy file mới.`

## Threat model / Mô hình đe dọa

See `docs/en/DEPLOYMENT.md` § “Linux runtime configuration & secure credential delivery (Prompt 77)” for the full table mapping world-readable, direct synveil read, environment leakage, log exposure, upgrade overwrite, uninstall deletion, symlink, oversized, empty, ambiguous dual, and rotation to mitigations and tests.

## Migration and review trigger / Điều kiện di chuyển và xem xét lại

- If systemd version on deployment target does not support `LoadCredential`/`%d` or `CREDENTIALS_DIRECTORY`, STOP and report; do not silently fall back to insecure `EnvironmentFile` for production.
- If a future encrypted-credentials rollout is justified (host key/TPM2 available, `systemd-creds` in image), supersede this ADR with `LoadCredentialEncrypted=` policy and provisioning steps.
- If a non-systemd production deployment is claimed, produce a platform-specific adapter ADR (do not put `/etc/synveil` paths into `crates/core`).
- If per-service accounts supersede single `synveil`, update `LoadCredential` ownership documentation but keep `root:root 0600/0700` source boundary.

---

## Quyết định (Tiếng Việt)

**Production Linux Gen-1 nhận secret database qua systemd credential delivery
(`LoadCredential=`), không qua `EnvironmentFile`.**

- **Nguồn admin** (host): `/etc/synveil/credentials/database-url`
  `0600 root:root`, thư mục `0700 root:root`. Tạo skeleton bởi install
  package-neutral; cài mới không bao giờ tạo secret. Xoay atomically (ghi file
  tạm `0600` + `rename`).

- **Phân phối systemd** (PID 1): service chứa:

  ```
  LoadCredential=database-url:/etc/synveil/credentials/database-url
  Environment=SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url
  EnvironmentFile=-/etc/synveil/synveil-scheduled-maintenance.env  # chỉ non-secret
  ```

  `%d` là specifier thư mục credentials (systemd 261). File xuất hiện như
  `$CREDENTIALS_DIRECTORY/database-url` `0400` read-only, unswapped, chỉ
  `synveil` (và root) đọc. Env `SYNVEIL_DATABASE_CREDENTIAL_FILE` mang **path**
  không bí mật.

- **Helper runtime** (`crates/api/src/runtime_database_credential.rs`):

  - `SYNVEIL_DATABASE_CREDENTIAL_FILE`, `CREDENTIALS_DIRECTORY`, `database-url`,
    `8 KiB` bound
  - Thứ tự: 1. file credential explicit, 2. fallback `DATABASE_URL`
  - **Chính sách dual B:** nếu cả file và `DATABASE_URL` đều đặt, fail
    `ambiguous` (không log secret), không kết hợp lặng lẽ. File rỗng sau trim
    coi như không đặt.
  - Đọc file bounded, `metadata.len` + post-read check, xử lý `\n`/`\r\n` đuôi
    đơn, validate qua `DatabaseConfig::from_url`, lỗi generic không chứa secret.

  Cung cấp `load_database_url_from_runtime_source()`,
  `database_config_from_runtime()`. Không log/cache/persist secret.

- **Tích hợp one-shot**: gọi `database_config_from_runtime()` **trước**
  `Timestamp::now_utc()` (vẫn đúng một lần) và trước `MigrationRunner` /
  `run_one_scheduled_backup_maintenance_cycle` (vẫn đúng một lần). Thiếu
  credential → `exit(1)` trước tick/worker.

- **Vai trò EnvironmentFile**: giữ cho `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS`
  (non-secret). Ví dụ `env.example` không chứa `DATABASE_URL=postgresql://`.

- **Manifest installer**: thêm `CREDENTIAL_DIRECTORY` `0700 root:root`;
  package không đóng gói secret. Cài mới tạo cả `/etc/synveil` và `credentials`
  nhưng không tạo `database-url`. Reinstall/upgrade giữ `database-url`
  byte-identical. Uninstall thường giữ; `--purge` mới xóa.

- **Xoay**: admin `install -m 0600 /tmp/new-url /etc/.../database-url` (hoặc tmp
  + `mv`); lần kích sau nhận credential mới. One-shot vô trạng thái, không cần
  restart daemon/watcher/poll.

- **systemd-creds**: `LoadCredentialEncrypted=` là **tùy chọn tương lai mạnh
  hơn**; Gen-1 **plain `LoadCredential=` là LOCKED**. Encrypted là
  **DEFERRED** vì cần host key/TPM2 chưa ở target tối thiểu.

- **Portability**: systemd, `/etc/synveil` nằm ngoài `crates/core` và domain
  portable.

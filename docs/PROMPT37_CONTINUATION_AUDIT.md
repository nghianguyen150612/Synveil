# Prompt 37 continuation audit / Kiểm tra tiếp tục Prompt 37

## Repository provenance

This audit was reconstructed from the inherited worktree before further code
edits on 2026-08-28. The worktree, including uncommitted Prompts 31–37, is the
continuation authority. No reset, clean, restore, rebase, commit, or push was
performed. Existing code is retained unless a specific defect is demonstrated.

- Workspace: `/mnt/Projects/Synveil`; branch: `main`.
- `HEAD` and local `origin/main`: `1d6f4226c8d2dc0144f0669d5f3c14ec2c4f44df`.
- Initial tracked diff: 55 files, 17,028 insertions, 557 deletions; additional
  untracked implementation files were inspected separately.
- All requested initial Git commands were run, including the complete diff
  capture and untracked inventory. Initial `git diff --check` passed.

Worktree hiện có là trạng thái tiếp tục có thẩm quyền. Các thay đổi Prompt
31–37 được giữ nguyên; chỉ sửa lỗi có bằng chứng. Bảng dưới ghi nhận trạng thái
**trước khi sửa tiếp**, không thay thế validation của trạng thái cuối.

## Inherited requirement classification

`DONE` means the implementation was inspected and its relevant available
prerequisite tests were rerun successfully in this continuation. It does not
waive final validation. `NEEDS_VALIDATION` means code exists but the applicable
host/native evidence has not yet been rerun. No major implementation was absent;
the missing testing documentation is called out explicitly.

| Requirement | Inherited state | Inspection / remaining work |
|---|---|---|
| Server credential schema | DONE | Forward-only PostgreSQL migration; digest-only columns and composite scope constraints tested. |
| Device credential identity | DONE | Distinct UUIDv7 core ID and canonical parsing tests. |
| Bearer secret format | DONE | 32 random bytes; 69-byte versioned lowercase-hex wrapper, redacted and zeroized. |
| Credential digest storage | DONE | Purpose-separated SHA-256; database dump/shape tests contain no raw token. |
| Credential authentication | DONE | Per-request owner/Device/credential lookup and constant-time verifier comparison. |
| Credential revocation | DONE | Credential, Device, and inactive-owner rejection; explicit revoke-all/new enrollment. |
| Enrollment grant schema | DONE | Digest-only scoped grant, 10-minute service TTL, 15-minute schema maximum. |
| Grant creation | DONE | Existing owned or new canonical PENDING Device; browser authentication, CSRF, 2-KiB strict JSON. |
| Grant exchange | PARTIALLY_DONE | Atomic activation/issuance/consume works; refresh expiry clock after lock acquisition. |
| Single-use behavior | DONE | Fresh PostgreSQL concurrent exchange has one winner; replay issues no secret. |
| Expiry behavior | BROKEN | Expiry uses a timestamp captured before awaiting database locks; an expired queued exchange can be accepted. Add regression and repair. |
| Lost-response behavior | DONE | Once-only handoff; generic replay rejection, explicit owner revoke-all and fresh grant. |
| Browser-vs-device principal model | DONE | Distinct typed variants; no fabricated SessionId or cookie downgrade. |
| CSRF behavior | DONE | Browser writes retain CSRF; exemption requires an authenticated Device principal. |
| Device-auth route scope | DONE | Enumerated inbound/read routes; route Device match; mutations/conflicts/admin denied. Expand real-router negative evidence. |
| Download authorization | DONE | Existing logical owner/file/version application service reused; exact immutable version transport. |
| Desktop ServerProfile | DONE | Non-secret persistent opaque identity, immutable origin, label and timestamps. |
| URL canonicalization | DONE | Mature parser plus strict origin-root policy and normalization/rejection tests. |
| HTTPS-only production policy | DONE | Normal rustls validation; explicit numeric-loopback test HTTP only; self-signed TLS rejected. |
| Redirect policy | DONE | All redirects disabled and rejected. |
| Profile/replica binding | DONE | SQLite, physical V2 root marker and HTTP profile agree; V1 cannot be silently rebound. |
| SecretStore integration | NEEDS_VALIDATION | Native Linux/Windows builders and fail-closed port exist; rerun isolated native Linux vault and Windows compile. |
| Credential/profile isolation | DONE | Versioned secure-store envelope binds origin/policy/profile/owner/Device/credential; copied SQLite cannot relabel it. |
| Credential lifecycle | DONE | Store/read-back, load, explicit replacement, durable cleanup and forget tested with injected failures. |
| Production HTTP SyncRemote | PARTIALLY_DONE | All seven operations and real-router acceptance pass; correct feed request limit to server contract. |
| Checkpoint transport | DONE | Exact real DTO, scope, epoch and decimal validation. |
| Change-feed transport | PARTIALLY_DONE | Revision-safe projection works; client currently permits 1,000 although server accepts 500. |
| Ack transport | DONE | Same bounded opaque evidence and exact checkpoint response. |
| Rebaseline transport | DONE | Start/page/complete, terminal proof and durable engine handoff pass. |
| Streaming download | DONE | Exact version, length/hash checks, bounded chunks, no whole-file buffering or transparent decompression. |
| Timeouts | DONE | Finite connect/header/metadata/idle/download budgets and deterministic timeout tests. |
| Body limits | DONE | Bounded streamed JSON/error/file bodies; correct feed item limit separately. |
| Protocol validation | PARTIALLY_DONE | Strict wire schemas and identities pass; align feed limit and documented route permissions. |
| HTTP error mapping | DONE | Auth/revocation/scope/rebaseline/checkpoint/evidence/rate/dependency/internal/offline/TLS/protocol distinctions. |
| Connection health | DONE | Anonymous readiness then authenticated checkpoint; HTML and incompatible responses rejected. |
| Retry policy | DONE | reqwest automatic retries explicitly disabled; one-time exchange is never automatically replayed. |
| Cookie isolation / User-Agent | DONE | No cookie jar; bounded non-secret product/version User-Agent. |
| Cross-origin credential-leak protection | DONE | Two-server redirect test observes zero requests at destination; wrong profile rejected before networking. |
| Logging/redaction | BROKEN | New capture test passes alone but misses real trace events in parallel suite. Make the assertion reliable without weakening it. |
| OpenAPI | PARTIALLY_DONE | 3.1 lint passes with seven existing warnings; remove incorrect DeviceBearer permission on system health. |
| Documentation | PARTIALLY_DONE | Bilingual implementation/security/deployment docs exist; Prompt 37 testing sections not started. |
| Unit tests | BROKEN | Client/core tests pass; API trace-capture test fails under parallel workspace execution. |
| PostgreSQL tests | DONE | 26/26 inherited server tests rerun on independently fresh disposable databases; add lock-expiry regression. |
| SQLite/client-sync tests | DONE | 55 unit and 12 inbound integration tests rerun successfully. |
| HTTP adapter tests | DONE | All inherited deterministic local HTTP/TLS adapter tests rerun successfully. |
| End-to-end enrollment/sync test | DONE | Fresh PostgreSQL + real Axum + production remote + SQLite/root + test SecretStore passes. |
| Windows validation | NEEDS_VALIDATION | Existing isolated matching Rust/MinGW tools found; no prior result is accepted as final evidence. |
| Linux validation | NEEDS_VALIDATION | Host client/filesystem/HTTP tests pass; isolated native Secret Service and final workspace gates remain. |
| Dependency policy | BROKEN | ISC in ring/webpki/untrusted and CDLA-Permissive-2.0 in root data need explicit scoped license review. |
| Workspace formatting | BROKEN | Inherited new files are not rustfmt-clean. Run canonical formatter after the focused repairs. |
| Forbidden scope | DONE | No watcher, outbound producer, automatic resolver, GUI, broker, TLS bypass or plaintext secret fallback found. |

## Prerequisite evidence from this continuation

Before code repairs: workspace `check` and strict `clippy` passed; `fmt` failed,
workspace tests stopped at the parallel API trace-capture failure, and
`cargo deny` failed only the new TLS dependency license policy. Focused client,
core and platform suites passed. OpenAPI lint passed with seven warnings.

All 26 inherited ignored PostgreSQL tests were then run individually, with a
new database for each case, in a new loopback-only PostgreSQL 17 container.
This includes the 14 metadata/server foundations, two browser-auth tests, five
storage-GC tests, one credential-schema test, two device-auth tests, the
first-admin timestamp-precision regression and real desktop HTTP acceptance.
None failed. The previous session's PostgreSQL container was left untouched.

Initial command output, source SHA-256 inventory, full diff and prerequisite
logs are captured in `/tmp/synveil-p37-continuation-vsv42jml`. These temporary
logs are supplementary; the final repository documentation must record the
actual commands and outcomes, with native Windows limitations stated explicitly.

## Final continuation results

The completed implementation was validated again on 2026-08-28. All mandatory
Prompt 37 requirements are complete within the specified Linux runtime / Windows
cross-target matrix. Native Windows runtime evidence remains explicitly outside
the available environment; no such result is inferred from cross-compilation.

### Focused repairs and completion

1. **Expiry after lock wait:** a fresh PostgreSQL regression first demonstrated
   acceptance of a grant that expired while the exchange waited for the Device
   lock. The repository now evaluates the production clock after owner, Device
   and grant locks and uses that timestamp for issuance/consumption. The same
   regression passes and verifies zero credential/consumption effects and a
   still-PENDING Device. Deterministic clock-based tests remain supported.
2. **Feed request bound:** a regression demonstrated that the HTTP adapter
   accepted 501–1,000 events while the server contract permits at most 500.
   Feed requests now reject values outside 1–500 before network access;
   manifest requests retain their independent 1–1,000 bound.
3. **Real trace capture and request IDs:** the parallel tracing regression now
   uses a fresh test process and still requires real events. An additional
   failing assertion demonstrated that a syntactically valid request-ID hint
   could copy a machine token into response headers/logs. Hints containing
   `svd1_` or `sve1_` are now replaced with a fresh server ID before tracing.
   Both body/header/URI redaction and the copied-token regression pass.
4. **Least-privilege OpenAPI:** removed the incorrect DeviceBearer permission
   from privileged system health. The actual router was already restrictive.
   Exactly ten reviewed inbound/metadata/content operations advertise bearer
   access; no mutation, conflict-resolution, upload or browser-admin grant was
   added. The real acceptance test now exercises these negative boundaries,
   cross-owner/Device/library/content rejection and all revoked sync routes.
5. **Native persistence:** strengthened the existing ignored OS test to read
   the stored entry from a new process before delete/read-back. Linux passes
   against a new private D-Bus session and synthetic vault. No personal keyring
   is used. Windows workspace compilation and client-sync/platform test linking
   pass with a matching official Rust 1.98 / complete MinGW toolchain.
6. **Dependency policy and documentation:** reviewed upstream TLS licenses and
   added only exact-version exceptions for four locked packages. The global
   allowlist/advisory policy is unchanged. Completed bilingual testing guidance,
   updated architecture/security/API/migration notes, and ran rustfmt.

### Closure of inherited incomplete requirements

| Initially incomplete/broken requirement | Final state | Evidence |
|---|---|---|
| Grant exchange / expiry behavior | DONE | Fresh PostgreSQL lock-wait expiry regression and full credential suite. |
| Production HTTP remote / feed / protocol bounds | DONE | 23 HTTP adapter tests and real-router acceptance. |
| SecretStore integration / lifecycle | DONE | 21 profile/store tests plus independent native Linux process-restart persistence. |
| Logging/redaction / unit tests | DONE | Real trace capture, copied-secret request-ID regression and complete parallel workspace suite. |
| OpenAPI | DONE | 3.1 lint passes; ten scoped bearer operations match the router. |
| Documentation | DONE | Required bilingual sections, migration notes and this continuation audit. |
| Windows validation | DONE | Workspace cross-target check and client-sync/platform executable link; native execution unavailable, not claimed. |
| Linux validation | DONE | Full host suites, local HTTP/TLS fixtures, fresh PostgreSQL, SQLite/root and native Secret Service. |
| Dependency policy / formatting | DONE | cargo-deny advisories/bans/licenses/sources pass, rustfmt and diff checks pass. |

All requirements initially marked DONE were reused and rerun in the final
matrix; no prior-session test claim is used as final evidence.

### Validation matrix

| Command / fixture | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets --locked` | PASS |
| `cargo test --workspace --all-targets --locked` | PASS: 316 passed, 0 failed; 28 environment-dependent tests separately exercised below. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS |
| `cargo test -p synveil-client-sync --all-targets --locked` | PASS: 55 unit + 12 inbound integration tests. |
| `cargo clippy -p synveil-client-sync --all-targets --all-features --locked -- -D warnings` | PASS |
| `cargo deny check` | PASS: advisories, bans, licenses and sources; duplicate-version warnings remain. |
| `npx --yes @redocly/cli lint api/openapi.yaml` | PASS: 0 errors, 7 non-blocking warnings. |
| `git diff --check` | PASS |
| All 27 ignored PostgreSQL cases | PASS: one newly created database per case, PostgreSQL 17, including the extended real HTTP acceptance test. |
| Ignored native Linux SecretStore test | PASS: new isolated vault, store, read from a new process, delete, verify absence. |
| `cargo check --workspace --all-targets --locked --target x86_64-pc-windows-gnu` | PASS with matching official Rust 1.98 and complete MinGW. |
| `cargo test -p synveil-client-sync -p synveil-platform --all-targets --locked --target x86_64-pc-windows-gnu --no-run` | PASS: actual Windows test executables linked; no native execution. |

The 28 tests ignored by the ordinary workspace invocation are all accounted for:
27 fresh PostgreSQL cases plus one native Linux vault case, each run explicitly.
The 67 client tests are a focused rerun, not 67 additional distinct tests.
OpenAPI warnings are five public probe/status operations without a 4xx response,
one unused RateLimited response, and one unused PublicShareContext scheme. No
undocumented endpoint behavior was invented to silence them.

Full commands, individual database names, timings and output are retained under
`/tmp/synveil-p37-continuation-vsv42jml`; the `final-` records are the last rerun
after this report was completed. The earlier `verify-` records independently
confirm the completed code and all matrix results. Temporary evidence is not a
production dependency.

### Preservation and remaining limits

An initial SHA-256 inventory was compared with the completed worktree. No
inherited file was removed: 275 inherited files were inventoried, 23 were
changed by this continuation, and the only added file is this audit. Historical
PostgreSQL migrations and desktop
`0001_initial.sql` are unchanged. The workspace remains intentionally dirty on
the same `main`/HEAD; there was no staging, commit, push or destructive Git
operation. The continuation edits only demonstrated defects, relevant tests,
license policy and documentation, retaining accumulated Prompt 31–37 work.

The new Prompt 37 implementation files below already existed when continuation
began. They were inspected and reused, not recreated. Other untracked files,
including the rest of `client-sync`, belong to the accumulated earlier prompts.

| Boundary | Inherited Prompt 37 files |
|---|---|
| Shared machine secrets | `crates/core/src/device_secrets.rs` |
| Credential persistence | `crates/metadata/src/device_credentials.rs`; `crates/metadata/tests/device_credentials_postgres.rs`; `migrations/20260828000000_device_credentials_enrollment.sql` |
| Authentication | `crates/auth/src/device_credentials.rs`; `crates/auth/tests/device_credentials_postgres.rs`; `crates/auth/tests/bootstrap_precision_postgres.rs` |
| HTTP API and acceptance | `crates/api/src/device_auth.rs`; `crates/api/src/tests/device_auth.rs`; `crates/api/tests/desktop_remote.rs` |
| Profile and local migration | `crates/client-sync/src/profiles.rs`; `crates/client-sync/src/profiles/tests.rs`; `crates/client-sync/migrations/0002_server_profiles.sql` |
| Production HTTP remote | `crates/client-sync/src/http_remote.rs`; `crates/client-sync/src/http_remote/wire.rs`; `crates/client-sync/src/http_remote/tests.rs` |
| Native secure storage | `crates/platform/src/native_secrets.rs` |

This continuation modified these existing files only:

- API: `api/openapi.yaml`, `crates/api/src/middleware.rs` (rustfmt only),
  `crates/api/src/request_id.rs`, `crates/api/src/tests/device_auth.rs`,
  `crates/api/tests/desktop_remote.rs`.
- Auth/persistence: `crates/auth/src/device_credentials.rs`,
  `crates/auth/tests/device_credentials_postgres.rs`,
  `crates/metadata/src/device_credentials.rs`.
- Desktop/platform: `crates/client-sync/src/http_remote.rs`,
  `crates/client-sync/src/http_remote/tests.rs`,
  `crates/platform/src/native_secrets.rs`.
- Policy/docs: `deny.toml`, `README.md`, `docs/README.md`,
  `docs/en/API_ARCHITECTURE.md`, `docs/vi/API_ARCHITECTURE.md`,
  `docs/en/ARCHITECTURE.md`, `docs/vi/ARCHITECTURE.md`,
  `docs/en/SECURITY.md`, `docs/vi/SECURITY.md`, `docs/en/TESTING.md`,
  `docs/vi/TESTING.md`, `migrations/README.md`.

Dependency manifests and `Cargo.lock` remain byte-identical to the inherited
state. Reviewed source scans found no TLS verification bypass, production
subprocess, new unsafe block, watcher/broker/GUI implementation, or literal
machine credential in the Prompt 37 production modules and client-sync source.
Separate scans of OpenAPI and documentation found no literal machine token.
Test-only synthetic secrets and subprocess isolation are explicitly excluded
from the production scan; database/log/Debug/error behavior is also exercised
by the tests, not inferred from text scanning alone.

Production uses verified HTTPS at the configured origin; there is no stable
server-installation ID, custom certificate trust, TOFU/pinning, subpath base URL,
ambient proxy support or TLS bypass. There is no automatic bearer expiry or
rotation; the lifecycle uses explicit re-enrollment and revocation. Revocation
rejects the next authenticated request; it does not cancel a previously authorized
in-flight stream. A caller must drop any already-loaded direct transport after
local forget; the inbound engine independently rejects stale enrollment before
its next synchronization call. Feed projection races require rebaseline.

Native Windows Credential Manager/TLS/NTFS/reboot/power-loss behavior is not
validated here. No filesystem watcher, outbound mutation producer, automatic
conflict resolver, GUI/pairing UX, installer, backup/sharing, WebSocket/SSE or
external broker was added. The only new subprocess use is in isolated test
harnesses, not the production credential or synchronization implementation.

Mọi requirement bắt buộc đã hoàn tất và rerun; worktree Prompt 31–37 được giữ
nguyên có chủ ý. Linux có bằng chứng runtime, Windows có compile và link thực
sự nhưng chưa có runtime native. Gate tiếp theo là observation + durable
outbound-intent capture của Prompt 38; chưa tự động submit mutation.

# PROMPT018 manifest — authenticated path qualification

Scope: provisional P018 under the supplied task; authoritative roadmap unavailable.
Base SHA: `dc00aed815bfd06a0090338f267b19ad7ec8d164` (P017 PR #177).
Target: `develop/v0.3`.
Branch: `test/v03-p018-synveilnet-authenticated-path-qualification`.

Implementation: HTTPS integration fixtures reuse existing profile approval,
enrollment/store loading, TLS/identity checks, checkpoint authentication and P017
selection. A separate opt-in Linux example uses existing approved profiles and OS
credentials. No production transport/authentication logic or Wizard path changes.
The lockfile adds only the client test dependencies already used by client-sync.

Changed files:

- `crates/client/src/transport/qualification.rs`: two-endpoint HTTPS integration.
- `crates/client/src/transport/tests.rs`: share existing deterministic route fixtures.
- `crates/client/src/transport.rs`: register the test module only.
- `crates/client/examples/p018_native_qualification.rs`: explicit native runner.
- `crates/client/Cargo.toml`, `Cargo.lock`: fixture dev dependencies.
- `.github/workflows/v03-p018-authenticated-path.yml`: deterministic qualification.
- This manifest and [qualification record](../network/P018_AUTHENTICATED_PATH_QUALIFICATION.md).

Local validation (Debian 13, Linux 6.18.55, Rust 1.99.1, PostgreSQL 17.11):

- Four P018 HTTPS integration tests passed; final strengthened persisted-state rerun pending.
- All 125 client and 16 auth library tests passed.
- P005 PostgreSQL authorization: 1 passed, 0 ignored.
- P015 PostgreSQL/ObjectStore: 1 passed, 0 ignored, `P015_MATRIX_COMPLETE` emitted.
- P006 focused API: 2 passed.
- Formatting, strict Clippy across client/client-sync/network/Host all targets,
  documentation validation and diff whitespace passed.
- Remaining client-sync/Host/network/config/content/history regressions are running.

Full final outcomes and CI conclusions will be appended before delivery.
CI status: not yet submitted; no passing CI claim.

Real network: `REAL_TAILNET_QUALIFICATION_BLOCKED`; no qualified LAN/tailnet
endpoints available. Loopback TLS and simulated route results are distinct evidence.
Independent security review: not performed. Merge recommendation: hold pending
validation, roadmap reconciliation and review; native acceptance remains blocked.
P004-D02/D03/D11/D14, P028 native acceptance and P036 publisher/signing remain open.
No automatic merge, production onboarding or P019.

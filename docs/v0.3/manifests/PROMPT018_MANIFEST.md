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

Implementation source SHA: `580dcbb58c28fb59906cc3be4755a2f0a5dc9dfa`.
CI command-block correction: `fdf7ad2ef837b924da7a0f031d0ab8440baf0c50`.
The subsequent manifest update changes documentation only. Exact final PR head
and synthetic merge checkout SHAs are available in Git/Actions source evidence;
no self-referential commit SHA is embedded in this file.

Local validation on 2026-10-30 (Debian 13, Linux 6.18.55 x86_64, Rust 1.99.1,
iproute2 6.15.0, no Tailscale CLI, PostgreSQL 17.11):

| Check | Observed result |
| --- | --- |
| P018 HTTPS integration, final persisted-state assertions | 4 passed, 0 failed/ignored; IPv4 and IPv6 loopback both available |
| Full client library, including P017/P018 and generation fence | 125 passed |
| Full client-sync library, credential/auth/HTTPS/lifecycle/snapshot regressions | 297 passed |
| Auth library, including enrollment expiry/revocation | 16 passed |
| Server config / Host / network libraries | 29 / 2 / 5 passed |
| P017 separate network transport contracts | 3 passed |
| P017/P018 transport suite with required native Linux ip query | 26 passed (`SYNVEIL_P017_REQUIRE_IP=1`) |
| P005 real PostgreSQL authorization | 1 passed, 0 ignored |
| P006 focused API / local ObjectStore API | 2 / 1 passed |
| Storage downloads | 19 passed |
| PostgreSQL owner-scoped immutable history | 1 passed, 0 ignored |
| P015 PostgreSQL/ObjectStore joined authorization | 1 passed, 0 ignored; `P015_MATRIX_COMPLETE` emitted |
| P016A live process/controller authentication/restart/shutdown | 1 passed, 0 ignored, 16.48s; generation 1 success, 2 failure, 3 recovery |
| Opt-in Linux native runner | Compiled; authenticated native execution blocked by missing qualified profiles/endpoints |
| Strict Clippy, client/client-sync/network/Host, all targets | Passed with `-D warnings` |
| Rust formatting / documentation validation / whitespace | Passed |

Commands follow the focused P018 workflow and the existing P015/PostgreSQL
workflows. P005/P015 used a disposable loopback PostgreSQL database; P016A used
a separate fresh database. The disposable PostgreSQL container was removed after
validation; no test service remains running. Temporary test roots, generated certificates and
memory credentials are synthetic. No production credential was supplied. The
D-Bus development package was extracted outside the checkout for local compilation.

## CI observations

The initial source's new workflow was rejected due to an unquoted Rust `::`
filter in a YAML plain value. [Run 40986795933](https://github.com/nghianguyen150612/Synveil/actions/runs/40986795933)
concluded failure before any job/test; this was corrected using a command block
and parsed locally. It is not counted as passing qualification.

For corrected head `fdf7ad2ef837b924da7a0f031d0ab8440baf0c50`, the observed
tested PR merge ref is `298a5fccac669cd6561c131180a92c6d0458d732`:

- [P018 run 40986920981](https://github.com/nghianguyen150612/Synveil/actions/runs/40986920981): **PASS**, all qualification, Clippy, documentation and artifact steps succeeded.
- [P017 run 40986921063](https://github.com/nghianguyen150612/Synveil/actions/runs/40986921063): pending at observation.
- [P015/P005/P006 run 40986920984](https://github.com/nghianguyen150612/Synveil/actions/runs/40986920984): **PASS**, including real PostgreSQL authorization and completion marker.
- [PostgreSQL lifecycle run 40986921023](https://github.com/nghianguyen150612/Synveil/actions/runs/40986921023): queued at observation.

PR: [#178](https://github.com/nghianguyen150612/Synveil/pull/178), draft,
targeting `develop/v0.3`; not merged. Passing local deterministic tests do not
establish passing hosted CI, native LAN or real tailnet acceptance.

Real network: `REAL_TAILNET_QUALIFICATION_BLOCKED`; no qualified LAN/tailnet
endpoints available. Loopback TLS and simulated route results are distinct evidence.
Independent security review: not performed. Merge recommendation: hold pending
required CI conclusions, roadmap reconciliation and independent review; native
acceptance remains blocked.
P004-D02/D03/D11/D14, P028 native acceptance and P036 publisher/signing remain open.
No automatic merge, production onboarding or P019.

# PROMPT019 — Installer network preflight manifest

Result: provisional, read-only integration; production execution remains disabled.
Authoritative roadmap reconciliation and native qualification remain blocked.

| Provenance | Exact value |
| --- | --- |
| Repository | `nghianguyen150612/Synveil` |
| Target | `develop/v0.3` |
| Existing delivery branch | `feat/v03-p019-synveilnet-installer-preflight` |
| Exact base / P018 #178 merge | `001c1e6f1104acc679777b060035c9dc09991810` |
| P018 source head | `a310c194359056dc731816f99bc855b6cf975a8e` |
| P017 #177 merge | `dc00aed815bfd06a0090338f267b19ad7ec8d164` |
| P019 implementation head | Recorded after implementation commit; pending validation closeout |

## Audit and scope

The audit preceded code changes: existing wizard/Qt worker/preflight engine,
P017 discovery/evidence/selection, P018 qualification and P009-style confirmation,
artifact planning and execution gates. See the
[architecture and limitations record](../network/P019_INSTALLER_NETWORK_PREFLIGHT.md).
The authoritative `Synveil-v0.3-Context-Roadmap-P001-P060.md` is absent from the
workspace, exact-base repository and scoped GitHub code search (no results).
Checked-in P002 tentatively associates P019–P024 with pairing; this explicit task
instead authorizes provisional installer integration. No pairing or authoritative
roadmap completion is claimed. Reconcile authoritative scope before acceptance.

## Changed surfaces

- `crates/client/src/transport/preflight.rs`: sanitized typed capability statuses,
  async local snapshot deadline/cancellation, freshness and deterministic tests.
- `crates/client/src/transport.rs`: exports the presentation boundary.
- `crates/desktop/src/installer_preflight.rs`: shared network evidence in the
  existing outcome, host freshness and removal of redundant Host-only LAN query.
- `crates/desktop/src/installer_wizard.rs`: explicit overlay choice, fresh-review
  action, stale completion/review rejection and disabled-execution regressions.
- `crates/desktop/src/bridge.rs`: existing worker integration, task cancellation,
  presentation-only expiry refresh and Qt dispatch.
- `crates/desktop/qml/InstallerWizard.qml`: existing three-step UI choices,
  actionable summaries and explicit refresh.
- `.github/workflows/v03-p019-installer-network-preflight.yml`: focused Linux and
  Windows contract gates; neither job is P028 acceptance.
- This manifest and P019 network documentation.

No dependencies, migrations, wire API, storage, credential/enrollment contracts,
endpoint validation rules or production effect adapters change.

## Validation evidence

Validation is in progress; pending/unexecuted jobs are not passes. Exact local
commands/results and hosted run URLs will be recorded in the evidence closeout.
Linux validation uses an isolated Ubuntu 24.04 container with Qt 6.4.2 and the
existing stable Rust toolchain. PostgreSQL regressions use a separate disposable
PostgreSQL 17 container, never a user database. No real tailnet is configured.

Initial environment setup had no compiler/Qt dependencies. A first compile before
package setup completed failed with missing `cc`; this is setup failure, not a
passing test. Validation resumes after dependencies are installed.

## Security and delivery restrictions

Discovery carries no endpoint approval, verifier, credentials, acknowledgement or
execution authority. Preview selection cannot change the normal client's endpoint.
No automatic pairing, tailnet enrollment, network/installer mutation, public
exposure or production installation is introduced. Cancellation stops network
inspection; already-started bounded package queries can finish in their existing
blocking worker, with generation-fenced publication.

**REAL_TAILNET_QUALIFICATION_BLOCKED**; genuine native LAN qualification also
unavailable. Preserve P004-D02/D03/D11/D14, P028, P036 and independent security
review. Windows UI builds/simulation are not P028 native acceptance. Unsupported
Windows/macOS native inspection and unknown/restricted network states stay
explicitly unavailable. No automatic merge, production readiness or P020.

Merge recommendation: **HOLD** for authoritative roadmap reconciliation,
independent review and required focused Linux/Windows CI results. Source review
and CI are not acceptance of production SynveilNet capabilities.

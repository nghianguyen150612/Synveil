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
| P019 Rust implementation | `51830ceef05633eb79cf66780153cb4009dc9bde` |
| Final implementation / QML presentation head | `0478268ff47e8a7236fa0f7d71676bada0ed8e24` |
| Draft PR | [#179](https://github.com/nghianguyen150612/Synveil/pull/179) |

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

Local observations: 2026-10-30. Ubuntu 24.04 in an isolated validation container,
Rust `1.99.1 (58cf8f9c3 2026-10-13)`, Qt `6.4.2`, PostgreSQL
`17.11 (Debian 17.11-1.pgdg13+2)` in a separate disposable container.
No user database, configured real tailnet or genuine native LAN fixture was used.
Rust tests exercised `51830ceef05633eb79cf66780153cb4009dc9bde`; the only subsequent
implementation change, `0478268ff47e8a7236fa0f7d71676bada0ed8e24`, moves the existing
network summary higher in QML Review. Final QML build/lint/smoke exercises that head.
The following evidence commit changes documentation only.

| Check | Observed result |
| --- | --- |
| Formatting, `cargo fmt --all -- --check` | PASS |
| P017/P018/P019 transport tests | 33 passed, zero failed/ignored; includes 7 P019 tests and 4 P018 HTTPS integration tests |
| Full client library with `SYNVEIL_P017_REQUIRE_IP=1` | 132 passed; trusted Linux iproute2 query required |
| Desktop tests, including wizard stale completion/review and no-effect boundary | 123 passed |
| Install-engine library | 50 passed |
| Engine/error/journal/lifecycle/Linux-package contracts | 81 / 118 / 91 / 108 / 124 passed (522 total) |
| AppImage integration, executed as UID 1000 | 9 passed |
| Auth / client-sync / Host libraries | 16 / 298 / 2 passed |
| Server config / network library / network transport contracts | 29 / 5 / 3 passed |
| Storage download contracts | 19 passed |
| P006 API ownership, version, ETag and Range | 2 passed |
| API local ObjectStore/content download | 1 passed |
| Real PostgreSQL 17 owner-scoped immutable version history | 1 passed, explicitly executed with `--ignored` |
| P005 cross-device authorization PostgreSQL integration | 1 passed, explicitly executed with `--ignored` |
| P015 PostgreSQL ObjectStore owner-to-byte | 1 passed; `P015_MATRIX_COMPLETE` observed, no unauthorized get/range calls |
| Final Linux Qt debug build, both QML files linted, offscreen startup smoke | PASS; no native install/network acceptance claim |
| Strict Clippy, selected crates and all targets, `-D warnings` | PASS |
| Workflow YAML parse | PASS; Linux and Windows matrix present, no allowed-failure jobs |
| Documentation validation and whitespace | PASS, including evidence closeout documentation |

Commands (all Cargo checks used `--locked`; no ignored integration is counted as
passed without explicit execution):

```sh
cargo test -p synveil-client --lib transport:: --locked
SYNVEIL_P017_REQUIRE_IP=1 cargo test -p synveil-client -p synveil-client-sync -p synveil-auth -p synveil-server-host --lib --locked
cargo test -p synveil-desktop -p synveil-install-engine --locked
cargo test -p synveil-install-engine --test engine_contract --test error_model_contract --test journal_contract --test lifecycle_contract --test linux_package_integration --locked
cargo test -p synveil-server-network -p synveil-server-config --locked
cargo test -p synveil-storage --test downloads --locked
cargo test -p synveil-api --lib --locked p006_ -- --nocapture
cargo test -p synveil-api --lib --locked api_downloads_use_the_content_service_and_local_object_store -- --nocapture
cargo test -p synveil-metadata --test postgres --locked postgres_version_history_is_owner_scoped_keyset_paged_and_pointer_based -- --ignored --test-threads=1 --nocapture
cargo test -p synveil-api --test authorization_postgres --locked -- --ignored --test-threads=1 --nocapture
cargo test -p synveil-api --test p015_postgres_objectstore_authorization --locked p015_postgres_objectstore_owner_to_byte -- --exact --ignored --test-threads=1 --nocapture
cargo clippy -p synveil-desktop -p synveil-client -p synveil-install-engine -p synveil-client-sync -p synveil-server-network -p synveil-server-host -p synveil-auth -p synveil-api -p synveil-storage --all-targets --locked -- -D warnings
cargo build -p synveil-desktop --locked
cargo fmt --all -- --check
bash scripts/validate-docs.sh
git diff --check
```

Qt lint and startup smoke use the generated module metadata and the same qmllint
and isolated offscreen procedure as `scripts/test-desktop-ui.sh`, for the debug
binary only. Release UI builds and native effectful acceptance were not run.
The unchanged `Main.qml:95` dialog emits implicit-width binding-loop warnings;
startup still exits successfully. The linker also reports its existing gold
linker deprecation on the debug build. These observations are not native acceptance.

Environment corrections: the first compile preceded package setup and failed
because `cc` was missing. The initial all-installer command ran as container root:
its 123 desktop and 50 engine tests passed, but the AppImage suite rejected root
with `InvalidEnvironment` and poisoned its fixture lock. The exact compiled
`appimage_integration-cc048fc490f7ce32` test executable was then run via
`docker exec --user 1000:1000`; all 9 passed. Remaining installer contracts were
executed separately, all 522 passing. No product source was changed to bypass the
root guard. Container provisioning failures are not product test passes.

## Hosted CI

Focused Linux and Windows jobs are required in
`.github/workflows/v03-p019-installer-network-preflight.yml`.
The initial source `51830ceef05633eb79cf66780153cb4009dc9bde` triggered:

- [P019 Linux/Windows run 41051689256](https://github.com/nghianguyen150612/Synveil/actions/runs/41051689256): both jobs queued at observation, neither counted as passed.
- [P017 run 41051689116](https://github.com/nghianguyen150612/Synveil/actions/runs/41051689116): queued at initial observation.
- [P018 run 41051689066](https://github.com/nghianguyen150612/Synveil/actions/runs/41051689066): queued at initial observation.
- [P015/PostgreSQL run 41051689096](https://github.com/nghianguyen150612/Synveil/actions/runs/41051689096): queued at initial observation.

Later source/documentation pushes can supersede these runs. Required final-head
conclusions must be verified on [PR #179](https://github.com/nghianguyen150612/Synveil/pull/179)
before merge; queued, cancelled or simulated checks are not passes. The final
engineering report and PR description identify the latest observed runs.

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

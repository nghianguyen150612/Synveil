# PROMPT022 — First-contact pairing ceremony manifest

Status: **PROVISIONAL / NON-PRODUCTION / ROADMAP RECONCILIATION BLOCKED**.

| Provenance | Exact value |
|---|---|
| Repository | `nghianguyen150612/Synveil` |
| Target | `develop/v0.3` |
| Existing feature branch | `feat/v03-p022-pairing-ceremony-qualification` |
| Exact base | `6d03f35c9db632af7789396aed24e6630f45aefd` |
| Starting refs | Target and feature both verified at the exact base; P021 PR #181 verified MERGED with that merge SHA |
| Separate inherited Windows Clippy repair | `d819d3c10c5a180e635bb1afdc02b38398d7fb7f` |
| P022 implementation source | `c160bf9d7f41d66fca6351f5966fb0cd8ef9923d` |
| Final implementation, including independent device revocation | `a5fb79a00528f99daab2a5b9c103a7d8ef17c41a` |
| Draft PR | [#182](https://github.com/nghianguyen150612/Synveil/pull/182), targeting `develop/v0.3` |

## Deliverables and boundary

Recovered after the quota interruption at `c160bf9d7f41d66fca6351f5966fb0cd8ef9923d`:
the implementation, workflow and draft PR already existed; two local document
edits were preserved. Remote target remained the exact base, and remote feature
matched that checkpoint. P021 ancestry was verified. No history was rewritten,
branch recreated or duplicate PR opened. The resume added an explicit current
target-device authority check, separate from issuer/consent revocation, with
fail-closed missing-device and transaction-fence tests.

See [P022_PAIRING_CEREMONY_QUALIFICATION.md](../security/P022_PAIRING_CEREMONY_QUALIFICATION.md)
for the baseline audit, typed states, independent identity boundary, separate
approval and consent evidence, exact binding, SQLite composition, recovery,
negative matrix and unresolved cryptographic decisions.

The isolated contract module and integration tests introduce no production
dependency, route, API, authentication change, credential issuance, enrollment,
mobile code, storage change or runtime migration. Only test support extracts
P021's existing consume checks for atomic composition. Existing P020/P021
regressions remain in the test suite. Focused CI is
`.github/workflows/v03-p022-pairing-ceremony.yml` (Ubuntu and Windows).

Separate inherited Windows Clippy repair: maintenance-worker declarations used
only by Linux health-file publication are compiled only on Linux, and existing
readiness check results are explicitly discarded on other platforms. Both
checks still run. No warning suppression, weakened validation, or auth change.
The historical Windows job failed on five unused values in
`crates/api/src/bin/synveil-worker.rs`:
[P021 run 41161263421, job 124419108329](https://github.com/nghianguyen150612/Synveil/actions/runs/41161263421/job/124419108329),
head `33466a116a980e6bfe656deb1f444d672924089e`. That run's Ubuntu job passed;
its Windows job failed strict Clippy after contract/auth/installer tests passed.
These are historical P021 observations, not P022 qualification.

## Validation evidence

Local Linux, Rust `1.99.1` / Cargo `1.99.1`. The original checkpoint passed all
checks below; the final implementation reran pairing tests and strict Clippy:

| Check | Actual conclusion |
|---|---|
| `cargo test -p synveil-pairing-contracts --locked` | PASS on final implementation: 7 P020 unit tests, 12 P021 integration tests (including child entrypoint), 16 P022 integration tests; 35 total, no failures |
| `cargo test -p synveil-auth -p synveil-api -p synveil-client -p synveil-client-sync -p synveil-server-network --lib --locked` | PASS: auth 16, API 123, client 132, client-sync 298, server-network 5; 574 total, no failures |
| `cargo clippy -p synveil-pairing-contracts -p synveil-auth -p synveil-api -p synveil-client -p synveil-client-sync -p synveil-server-network --all-targets --locked -- -D warnings` | PASS, no warnings |
| `cargo test -p synveil-install-engine --locked` | PASS on final implementation: 681 tests, no failures |
| `cargo fmt --all -- --check` | PASS |
| `bash scripts/validate-docs.sh` | PASS: 24 validation groups |
| `git diff --check` | PASS |
| `cargo tree -i synveil-pairing-contracts --locked --depth 1` | Only the isolated crate itself; no production reverse dependency |
| Git object comparison with the exact base | Identical Android/iOS, auth, metadata, client/client-sync and migration trees; identical device secret, Host trust, production device-auth API, OpenAPI, Cargo manifest and lockfile blobs |

The local environment lacked DBus development metadata. Initial regression
build/link attempts stopped at that environmental dependency. Successful retry
used unprivileged extracted Debian DBus/systemd development packages in a
temporary sysroot, with links to the installed runtime shared libraries. No
repository dependency or host security setting was changed. No live PostgreSQL
integration, native mobile or tailnet qualification was performed.

## Hosted checkpoint evidence

Observed on 2026-10-31 after resumption; all rows below refer to the exact
checkpoint head `c160bf9d7f41d66fca6351f5966fb0cd8ef9923d`, before the independent
device-revocation addition. No queued/running/cancelled/skipped check is counted
as passing.

| Workflow | Actual conclusion |
|---|---|
| [P022 run 41162909982](https://github.com/nghianguyen150612/Synveil/actions/runs/41162909982) | SUCCESS: Ubuntu job 124424166674 and Windows job 124424166548; contract/race tests, auth/client regressions, formatting and strict Clippy passed on both |
| [P020 run 41162909987](https://github.com/nghianguyen150612/Synveil/actions/runs/41162909987) | SUCCESS |
| [P021 run 41162909984](https://github.com/nghianguyen150612/Synveil/actions/runs/41162909984) | SUCCESS: Ubuntu and Windows, including installer tests and strict Clippy; the inherited worker warning repair was verified |
| [Rust CI run 41162909944](https://github.com/nghianguyen150612/Synveil/actions/runs/41162909944) | SUCCESS |
| [P019 run 41162909951](https://github.com/nghianguyen150612/Synveil/actions/runs/41162909951) | FAILURE: Ubuntu passed; Windows transport, wizard/installer, credential/auth and network tests passed, then strict Clippy failed in unchanged desktop code |
| [Windows native run 41162910040](https://github.com/nghianguyen150612/Synveil/actions/runs/41162910040) | FAILURE: candidate build passed; standard-user-native failed at `Record required unavailable surfaces honestly`. P028 remains unresolved |

The P019 Windows errors are unused desktop enum variants
(`PreviewPartiallyAvailable`, `NativePackageManagerManaged`,
`MissingPackageManager`, `MissingNativeDependency`) and unit-struct `default()`
calls in `host_ui.rs`. No desktop code or lint policy is changed by P022; this
broader workflow is not reported green. The P022 and P021 strict boundaries
passed independently. Other release/native/security gates remain open.

P022 source artifacts 13028765980 (Ubuntu) and 13028109766 (Windows) identify the
checkpoint head above. Original superseded base runs and six duplicate push
runs on this feature branch were cancelled to free capacity; they provide no
qualification evidence. No other branch's runs were cancelled.

The final delivery commit adds these evidence documents to implementation
`a5fb79a00528f99daab2a5b9c103a7d8ef17c41a`. Its exact HEAD and final-head hosted
conclusions are recorded in the [draft PR](https://github.com/nghianguyen150612/Synveil/pull/182)
and delivery report after the final push. This avoids treating the earlier
checkpoint's green jobs as results for a later SHA.

## Changed files

- `crates/pairing-contracts/src/ceremony.rs`: versioned states, DTOs and trusted reference evidence.
- `crates/pairing-contracts/src/lib.rs`: exports the isolated ceremony module.
- `crates/pairing-contracts/tests/ceremony.rs`: deterministic negative, race, revocation, rollback and recovery qualification.
- `crates/pairing-contracts/tests/support/mod.rs`: reuses P021 consume checks under the outer transaction.
- `.github/workflows/v03-p022-pairing-ceremony.yml`: focused Ubuntu/Windows checks and SHA artifacts.
- `crates/api/src/bin/synveil-worker.rs`: separate inherited platform-warning repair.
- `docs/v0.3/security/P022_PAIRING_CEREMONY_QUALIFICATION.md`: audit, design and limitations.
- `docs/v0.3/manifests/PROMPT022_MANIFEST.md`: provenance and observed evidence.

## Unresolved gates

P004-D12/D13 remain `RECOMMENDED_PENDING_APPROVAL`. P028, P036, authoritative
roadmap reconciliation, independent security review, physical Android/iOS
pairing and real-tailnet qualification remain unresolved. The authoritative
roadmap is absent from this checkout/workspace. No simulated test qualifies
native camera, secure-store, cryptographic or real-network behavior. No
production pairing activation, deployment, automatic enrollment, auto-merge or
P023 work is included.

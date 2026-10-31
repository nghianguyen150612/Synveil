# PROMPT022 — First-contact pairing ceremony manifest

Status: **PROVISIONAL / NON-PRODUCTION / ROADMAP RECONCILIATION BLOCKED**.

| Provenance | Exact value |
|---|---|
| Repository | `nghianguyen150612/Synveil` |
| Target | `develop/v0.3` |
| Existing feature branch | `feat/v03-p022-pairing-ceremony-qualification` |
| Exact base | `6d03f35c9db632af7789396aed24e6630f45aefd` |
| Starting refs | Target and feature both verified at the exact base; P021 PR #181 verified MERGED with that merge SHA |

## Deliverables and boundary

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

Implementation SHA, draft PR and actual hosted observations will be recorded
after committing and running the checks. No hosted success is inferred from
local Linux tests. Hosted qualification remains pending until jobs execute.

## Unresolved gates

P004-D12/D13 remain `RECOMMENDED_PENDING_APPROVAL`. P028, P036, authoritative
roadmap reconciliation, independent security review, physical Android/iOS
pairing and real-tailnet qualification remain unresolved. The authoritative
roadmap is absent from this checkout/workspace. No simulated test qualifies
native camera, secure-store, cryptographic or real-network behavior. No
production pairing activation, deployment, automatic enrollment, auto-merge or
P023 work is included.

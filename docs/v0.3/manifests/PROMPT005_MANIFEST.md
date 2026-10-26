# Synveil v0.3 — PROMPT005 manifest

**Result: BLOCKED.** The authorized test-only probe reproduced a production
authorization defect. This manifest records the evidence and stop point; it
does not claim P005 completion.

## Scope and Git preflight

| Item | Evidence |
|---|---|
| Repository / requested target | `nghianguyen150612/Synveil`, target `develop/v0.3` |
| Starting `main` | `604499fce0dfd134d82da3d5bb5d4a055c5eea8d` |
| Starting `develop/v0.3` | `3d06751bdaf47ffe518253f8c31080e751a6d380` |
| P004 integration | PR #162 merged at the expected `develop/v0.3` SHA; merge commit inspected. |
| Working branch | `test/v03-p005-authorization-baseline`, created from the verified P004 SHA. |
| Roadmap | `Synveil-v0.3-Context-Roadmap-P001-P060.md` unavailable in the repo and workspace. This is a provisional test-only increment derived from P003/P004. |
| P004-D01–D14 | Not approved by starting P005; remain pending. |

The local P004 source SHA is `3d06751bdaf47ffe518253f8c31080e751a6d380`.
P004's manifest and maintainer-decision file separately record the roadmap's
absence and the pending decisions. The relevant P001–P004 references listed in
PROMPT005 were read before fixture design.

## Changes and fixture

| File | Purpose |
|---|---|
| `crates/api/tests/authorization_postgres.rs` | Ignored, explicit PostgreSQL 17 integration probe using the existing API router and auth/metadata/sync/mutation services. |
| `docs/v0.3/security/P005_LEGACY_AUTHORIZATION_BASELINE.md` | Provisional observed matrix, source enforcement references, evidence status, defect and P006 handoff. |
| `docs/v0.3/manifests/PROMPT005_MANIFEST.md` | This run and scope record. |

The fixture creates a unique child database for each run, applies normal
migrations, asserts PostgreSQL 17, and force-drops the child database after
success or a caught test assertion panic. It uses synthetic owners, passwords,
device credentials, cookies, and ETag keys. No production database or
production service is used. The test is ignored by default following the
workspace's PostgreSQL integration-test convention; it requires an explicit
`SYNVEIL_TEST_DATABASE_URL` to a disposable PostgreSQL 17 instance.

## Test result and blocker

The probe covers owner-isolated library/node reads, a foreign metadata rename
with unchanged target state, missing/invalid auth, browser/device class
separation, mixed credentials, own and mismatched device checkpoint access,
foreign library access, credential revocation, session expiry/logout, a valid
device mutation, and a cross-device mutation attempt.

All assertions before the final cross-device mutation assertion passed. That
assertion failed against a local disposable PostgreSQL 17 server:

```text
device-one bearer impersonated device-two at
/api/v1/devices/{device_id}/libraries/{library_id}/mutations:
observed HTTP 200 OK, node count 2->3; expected concealed denial and unchanged state
```

The authenticated device identity is not compared with the URL device ID in
`crates/api/src/mutations.rs::submit`; the metadata layer checks that the
provided device and library are active and belong to the authenticated owner.
The result is a same-owner cross-device authorization bypass and persisted
mutation. This is production behavior, but P005 authorizes tests and
documentation only. Production code, API contracts, migrations, and storage
formats were not changed. Per the prompt, work stops here without an unrelated
security refactor; the failing test is retained as the reproducer.

## Traceability and unqualified cases

| Reference | Evidence status |
|---|---|
| `TM-006`, `SV-SEC-005`, `AT-07` | Partial. Owner/library/node reads, a foreign rename, device checkpoint and mutation contexts are covered. The required wrong-device mutation is a reproducible failure, so the authorization baseline is blocked. |
| `TM-013`, `SV-SEC-010`, `AT-15` | Unverified. No future role mapping or legacy-to-new policy comparison was attempted. |
| `TM-014`, `SV-SEC-011`, `AT-16` | Partial. Existing credential/session expiry and revocation behavior is probed; concurrent revocation or future policy-revision fencing is not. |
| P004-D01–D14 | Pending maintainer decisions; none are approved or implemented here. |

File-version listing/metadata, current/historical content, physical ObjectStore
keys, all sync feed/acknowledgement paths, malformed identifiers, mutation
replay/conflict rules, full wrong-owner write matrix, and revocation races are
not verified. See the security baseline for the observed operation matrix and
source enforcement points. No full P003 security acceptance is claimed.

## Validation and CI

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed. |
| `cargo test -p synveil-api --test authorization_postgres --locked --no-run` | Passed; focused target compiled. |
| Focused ignored PostgreSQL probe | Intentionally fails at the final assertion with the reproducible authorization defect above. Earlier assertions pass. |
| Relevant existing authentication/API/metadata tests | Not run after the defect was reproduced; no broad test-pass claim. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Blocked by environment before workspace completion: `cxx-qt` could not find a Qt installation. |
| `cargo clippy -p synveil-api --all-targets --locked -- -D warnings` | Passed for the changed Rust test target and API crate. |
| `cargo deny check` | Passed with cargo-deny 0.20.2; advisories, bans, licenses, and sources checks were `ok`. Existing duplicate-version warnings remain configured as warnings. |
| `./scripts/validate-docs.sh` | Passed. |
| `git diff --check` | Passed. |
| Post-merge CI on P004 merge SHA | Rechecked 2026-10-25. PostgreSQL 17 scheduled maintenance and Linux AppImage completed successfully; Linux native packages, Windows installer, and Rust CI remained `in_progress`; Windows native acceptance failed at the separate P028 environment gate because the hosted runner lacks qualified interactive GUI/new-logon support and the named-pipe end-to-end gate is pending. Running checks are not counted as successful. |
| Windows P028 | Separate release-qualification gate; not run or claimed. |

The focused test used a local, disposable PostgreSQL 17 Docker service, not
hosted CI or a simulated fixture. The test target is ignored by default, so
ordinary CI success alone does not demonstrate that this regression probe ran.
The existing P004 Windows native-acceptance failure is the separately reported
P028 qualification gate, not a P005 test result.

## Git, PR, and P006 handoff

Implementation commit: `4cce39c5a538f199cbf7c3e3c4e15038e7d7f8de`
(`test(security): establish P005 legacy authorization regression baseline`).

Draft PR #163 is open at
https://github.com/nghianguyen150612/Synveil/pull/163, targeting
`develop/v0.3` at base `3d06751bdaf47ffe518253f8c31080e751a6d380`. It was
created with the implementation commit as its initial head; PR checks were
queued or in progress at that point, and ordinary CI does not run the ignored
database probe by default. A manifest closeout commit records this post-push
PR evidence without rewriting the published implementation commit. Keep the
PR open and unmerged while the defect and applicable checks/reviews remain
unresolved. No squash, rebase, or P006 work is authorized.

P006 prerequisites: obtain the roadmap and required maintainer decisions;
separately qualify the production fix for device-ID binding; then expand and
pass coverage for version/content/storage, all sync routes, malformed IDs,
mutation protocol cases, and revocation concurrency. Keep AT-15/AT-16 and full
P003 acceptance unclaimed until their specified evidence is supplied.

## Post-blocker staging update — 2026-10-25

The test-only PR #163 has been **retargeted as a stacked draft PR** to the P005A repair branch `fix/v03-p005a-device-mutation-binding` (PR #164), not merged. This allows the test diff to be reviewed against the one-line production authorization repair without putting the known failing regression onto `develop/v0.3` prematurely.

Commit `eec125f97ff5c6bd79f21d084bf034910d5d5432` expands the previously failing PostgreSQL probe with durable-state assertions: the mismatched device request must leave the node count, change-journal count and device-mutation-operation count unchanged, and must not persist the forged mutation ID. These assertions are **committed but not yet executed against the stacked PR head**; they must not be reported as passed.

P005A PR #164 is still unmerged and has no independent review. Re-run the ignored PostgreSQL 17 probe with the repair applied and record exact results before P005 is considered complete. After P005A merges, retarget PR #163 to `develop/v0.3` and verify its effective diff remains test/documentation-only. All P004 policy decisions remain pending.

## P005 security regression qualification — 2026-10-26

P005A security fix PR #164 merged into `develop/v0.3` at `dd82bb3ee01477a3bea0632e0febbe97113c7517`; the tested P005 branch includes that merge and the expanded negative assertions. The explicitly ignored PostgreSQL 17 regression ran as a mandatory, hard-fail GitHub Actions test on P005 head `f88fe2564dd7cc4ed944119a7b323ca230d9a0e3`. Workflow [40181341086](https://github.com/nghianguyen150612/Synveil/actions/runs/40181341086), job `P005 cross-device mutation regression (PostgreSQL 17)`, **completed SUCCESS**. This proves the scoped full integration probe, including no unauthorized node, journal or device-mutation-operation persistence, passed against the merged P005A fix. It does **not** prove future RBAC/revocation fencing (AT-15/AT-16), storage/content coverage, or full P003 acceptance.

The PR's other broad jobs include pre-existing P028 Windows-native acceptance environment failures and P036 publisher keyring not provisioned; they remain release/infrastructure blockers, not passed gates. The original P004-D01–D14 policy decisions, missing P001–P060 roadmap, and unqualified version/content/stronger concurrency scenarios remain explicitly outstanding.

At qualification time PR #163 was still DRAFT with **zero independent review submissions**. Any maintainer-directed merge without that review must be recorded as an exceptional integration decision, not as a completed independent security review. Do not delete the earlier reproduced-defect record or rewrite historical blocked evidence.

# PROMPT015 manifest

Scope: provisional maintainer-directed PostgreSQL 17 → local ObjectStore owner-to-byte security qualification. Authoritative v0.3 P001–P060 roadmap unavailable.

- Starting branch/commit: `test/v03-p015-postgres-objectstore-owner-to-byte`, `23af76f027b0e17f1186d160a3e7d20c551bdcf1`.
- Remote `develop/v0.3` and existing P015 branch fetched and verified before edits; both matched P014 PR #174's merge.
- Implementation: dedicated real-router integration fixture and fail-closed CI gate; production authorization unchanged.
- Files: `crates/api/tests/p015_postgres_objectstore_authorization.rs`, `.github/workflows/v03-p015-postgres-objectstore-authorization.yml`, this manifest and [qualification record](../security/P015_POSTGRES_OBJECTSTORE_AUTHORIZATION.md).
- Local PostgreSQL/ObjectStore matrix: PASS on PostgreSQL 17.11; exactly 1 passed, 0 failed, 0 ignored, completion marker reached. Initial fixture assertions were corrected to the established `authentication_failed` error and missing-head 404; no production change was required.
- Local formatting, documentation validation and diff checks: PASS. P005 regression: 1 passed; P006 HTTP: 2 passed; real local-store HTTP: 1 passed; storage downloads: 19 passed; PostgreSQL version history: 1 passed. API all-target Clippy initially found an unnecessary Vec in the new fixture; corrected to an array; API all-target Clippy recheck PASS (`-D warnings`). Joined P015 test rerun after correction: 1 passed, 0 failed, 0 ignored (2.54s).
- Hosted exact-commit CI: initial run FAIL on the new fixture’s unnecessary Vec; corrected candidate awaiting hosted qualification. No hosted PASS claim yet.
- Native security qualification: BLOCKED. P014 Windows native acceptance, P028 native acceptance and P036 publisher/signing remain unresolved; P004-D02/D03/D11/D14 remain independent decisions.
- Production readiness: not claimed. P016: not started. Automatic merge: prohibited.

## Exact-commit evidence

Implementation commit tested locally: `d20c66f0d1ce4c2048ae1dad4e392c7107c9b55a`. Initial hosted run [40807100294](https://github.com/nghianguyen150612/Synveil/actions/runs/40807100294) checks PR merge candidate `2be6068cad2239909eea14d1b4bd22532cd2f937` (parents: P014 baseline and that implementation commit). This run concluded FAILURE on `clippy::useless_vec`; later integration steps were skipped and are not PASS. The subsequent array-only lint correction and this evidence update require a new exact-candidate run.

PR: [#175](https://github.com/nghianguyen150612/Synveil/pull/175), targeting `develop/v0.3`, open for maintainer security review. Final checkout SHA, latest Actions run IDs/conclusions and final merge recommendation are recorded in the PR evidence section. The evidence-only update and one-line lint correction do not change production behavior. No automatic merge is authorized.

Corrected-source hosted run [40807557199](https://github.com/nghianguyen150612/Synveil/actions/runs/40807557199), head `5e15d445bd55b86cc0733954e3e5bbf714d6b13b`, merge checkout `e89757a71d39a199fb2c4370be5a50dc3ead6d6b`: **FAIL overall**. Formatting, Clippy, PostgreSQL 17 confirmation, P005/P006 regressions, storage downloads, version history and the explicitly executed P015 integration all passed. Documentation validation failed because the runner lacked `rg`. The workflow now explicitly installs `ripgrep`; a new full gate is required. This is a workflow dependency correction, not a security-policy relaxation.

The independent server self-host acceptance [artifact-identity job](https://github.com/nghianguyen150612/Synveil/actions/runs/40807557173/job/123230015681) failed at “Materialize approved public publisher keyring” (exit 2); it remains outside P015. Collaborator `ducminh231011` has been requested as reviewer. Local disposable databases, roots and PostgreSQL container were removed; final worktree was clean before this evidence update.

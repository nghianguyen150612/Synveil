# PROMPT015 manifest

Scope: provisional maintainer-directed PostgreSQL 17 → local ObjectStore owner-to-byte security qualification. Authoritative v0.3 P001–P060 roadmap unavailable.

- Starting branch/commit: `test/v03-p015-postgres-objectstore-owner-to-byte`, `23af76f027b0e17f1186d160a3e7d20c551bdcf1`.
- Remote `develop/v0.3` and existing P015 branch fetched and verified before edits; both matched P014 PR #174's merge.
- Implementation: dedicated real-router integration fixture and fail-closed CI gate; production authorization unchanged.
- Files: `crates/api/tests/p015_postgres_objectstore_authorization.rs`, `.github/workflows/v03-p015-postgres-objectstore-authorization.yml`, this manifest and [qualification record](../security/P015_POSTGRES_OBJECTSTORE_AUTHORIZATION.md).
- Local PostgreSQL/ObjectStore matrix: PASS on PostgreSQL 17.11; exactly 1 passed, 0 failed, 0 ignored, completion marker reached. Initial fixture assertions were corrected to the established `authentication_failed` error and missing-head 404; no production change was required.
- Local formatting, documentation validation and diff checks: PASS. P005/P006 retained regressions and Clippy: pending.
- Hosted exact-commit CI: pending; no hosted PASS claim yet.
- Native security qualification: BLOCKED. P014 Windows native acceptance, P028 native acceptance and P036 publisher/signing remain unresolved; P004-D02/D03/D11/D14 remain independent decisions.
- Production readiness: not claimed. P016: not started. Automatic merge: prohibited.

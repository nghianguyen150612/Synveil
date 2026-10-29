# P015 — PostgreSQL/ObjectStore authorization qualification

Provisional maintainer-directed security qualification. The authoritative P001–P060 roadmap was unavailable; this document does not replace it. Starting baseline: P014 merge `23af76f027b0e17f1186d160a3e7d20c551bdcf1` (PR #174). Production authorization is unchanged.

## Authorization path

Production Axum router and authentication middleware → PostgreSQL session authentication → authenticated owner → owner-scoped `PostgresContentReadRepository` → `ContentReadApplicationService` → trusted replica key → local filesystem ObjectStore → streamed HTTP body. Version history uses the production PostgreSQL `VersionHistoryService` installed by `with_postgres_auth`. The public routes accept logical node/version IDs, never a physical storage key.

The ignored `p015_postgres_objectstore_owner_to_byte` integration test requires `SYNVEIL_TEST_DATABASE_URL`, verifies PostgreSQL major 17, creates a UUID-named child database, applies real SQLx migrations and uses two password-authenticated owners with independent libraries. Each file has two immutable versions backed by distinct physical objects and synthetic content. A delegating ObjectStore wrapper counts every `get` and `range_read` attempt; it makes no authorization decisions. Requests use the production router in process, without a TCP listener.

## Matrix

| Scenario | Assertions |
| --- | --- |
| A current and both historical versions; B own versions | Exact bytes, length, ETag, full read counter |
| B copied A node/version IDs | Concealed 404; safe JSON; no private bytes, keys, validators or storage fields; unchanged read counters |
| Copied ETag, Range, combined If-None-Match/Range and If-Range/Range | Same concealed denial before physical read |
| Bounded, open-ended, suffix ranges on current and historical content | 206, exact Content-Range, exact body slice, range counter |
| Owner conditional read | 304, empty body, no physical read |
| Missing, invalid, revoked session | 401, safe body/headers, no physical read |
| Foreign version metadata/history and genuine A cursor | Concealed 404, no storage read |
| A cursor on B's node | Invalid cursor rejection, no A version IDs |
| Missing node/version and missing current pointer | Fail closed before physical read |
| Trashed node, current and historical routes | Concealed 404 and zero physical reads |
| Foreign current pointer | PostgreSQL foreign-key rejection |
| Missing verified replica | Safe 503 before physical read |
| Inconsistent verified replica length | Safe 500 before physical read |
| Owner reads after denials and restored fault injections | Exact immutable bytes; full persisted nodes/versions/objects/replicas snapshot unchanged |

Database cleanup runs after caught test panics; an additional drop guard covers fixture setup failure. The isolated filesystem root is removed on success or unwinding. Credentials exist only in the disposable database/test process. Abrupt process termination may prevent cleanup; CI's disposable service and runner bound that lifetime.

## Reproduction and qualification

```sh
SYNVEIL_TEST_DATABASE_URL=postgresql://TEST_ONLY cargo test -p synveil-api \
  --test p015_postgres_objectstore_authorization --locked \
  p015_postgres_objectstore_owner_to_byte -- --exact --ignored --test-threads=1 --nocapture
```

The dedicated [workflow](../../../.github/workflows/v03-p015-postgres-objectstore-authorization.yml) uses PostgreSQL 17 and requires both the explicit test's one-pass summary and its final matrix completion marker. Missing database access, failed migrations, zero selected tests and skipped tests cannot satisfy that gate. It retains P005 mutation binding, P006 HTTP/local-store regressions, storage downloads and PostgreSQL version-history coverage, plus formatting, API Clippy, documentation and diff checks. The Actions checkout SHA, run ID and attempt are printed with the test evidence.

Observed results and exact commit/run evidence are recorded in the [manifest](../manifests/PROMPT015_MANIFEST.md). A queued or blocked run is not a pass.

## Evidence boundaries and remaining blockers

This qualifies a bounded, sequential local-filesystem composition, not production readiness, host filesystem access controls, arbitrary metadata corruption, streaming TOCTOU races, concurrent revocation, alternate storage adapters, upload/restore/GC/backup authorization or API downgrade behavior. PostgreSQL constraints remain enabled during fault injection. No policy, privilege, access grant, token type or v0.2 API v1 contract changes are introduced.

P004-D02/D03/D11/D14 remain unresolved independently. P014 Windows native acceptance is BLOCKED; P028 Windows native acceptance and P036 publisher/signing qualification remain independent unresolved gates. No production Wizard execution, release-ready declaration, security-policy relaxation or P016 work is authorized. Maintainer security review and successful exact-candidate CI are required before recommending merge; P015 must remain open pending those gates.

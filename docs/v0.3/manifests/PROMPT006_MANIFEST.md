# Synveil v0.3 — PROMPT006 Manifest

**Result:** P006 test implementation in progress; integration evidence pending. Do not mark complete solely because tests have been authored.

## Baseline and prerequisite

| Item | Evidence |
|---|---|
| Repository | \`nghianguyen150612/Synveil\` |
| Base | \`develop/v0.3\` at \`e11a7778a2ed2ae5a825b6d95a911101fe7e337c\` |
| P005A | PR #164 merged at \`dd82bb3ee01477a3bea0632e0febbe97113c7517\` |
| P005 | PR #163 merged at \`e11a7778a2ed2ae5a825b6d95a911101fe7e337c\` |
| P005 PostgreSQL regression | Run 40181341086 completed SUCCESS |
| Scope | Provisional P006 security regression; original P001–P060 roadmap not found |
| P004 security decisions | D01–D14 pending; no approvals invented |
| Production source changes | None |

## Changed files

- \`crates/api/src/tests.rs\` — P006 cross-owner Axum version/list/cursor and content ETag/Range cases; real local ObjectStore integration denial.
- \`.github/workflows/v03-p006-content-security.yml\` — hard-fail focused API and storage tests, real PostgreSQL 17 metadata version test, retained P005 ignored PostgreSQL regression.
- \`docs/v0.3/security/P006_CONTENT_AUTHORIZATION_BASELINE.md\` — route/matrix evidence and limitations.
- \`docs/v0.3/manifests/PROMPT006_MANIFEST.md\` — this manifest.

## Acceptance coverage and qualification

| Gate | State |
|---|---|
| Authenticated foreign version/list/cursor denied | Implemented in P006 API test; awaiting actual run |
| Foreign ETag/Range on current and historical content denied without stream | Implemented in P006 API test; awaiting actual run |
| Real filesystem ObjectStore maintains owner isolation under Axum | Existing integration extended; awaiting actual run |
| Owner-scoped PostgreSQL 17 version metadata and pagination | Existing metadata integration explicitly invoked by focused workflow; awaiting run |
| P005 regression preserved | Explicit PostgreSQL 17 workflow command; awaiting run |
| Rust formatting, API Clippy, storage tests, docs, diff | CI/run evidence pending |
| Fully joined PostgreSQL+ObjectStore owner-to-byte end-to-end test | Not implemented; scoped limitation |
| Independent review and merge | Pending |
| P028/P036 release qualification | Unrelated external blockers, not waived |

## Risk and security evidence

This PR adds test-only security assertions, not authorization features or policy changes. It preserves v0.2 API v1 owner/device and object-key abstraction. It does not establish full TM-006/032–036, AT-07/34/35, RBAC or release qualification. Failures in the new focused tests are genuine P006 blockers until investigated; no test is allowed to be skipped or changed to assert vulnerable behavior.

## Handoff

After exact-head P006 CI completes, inspect actual test logs, correct formatting/compilation or behavioral regressions, and expand the combined PostgreSQL+ObjectStore evidence if the applicable P006 gate requires it. Merge only after acceptable security/test review and validated CI. P007 remains deferred until P006 gate closure. Record final commit, CI IDs and PR URL in a follow-up after publication.

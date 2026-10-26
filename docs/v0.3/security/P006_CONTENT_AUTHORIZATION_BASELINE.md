# Synveil v0.3 — P006 Content Authorization Baseline

**Status: IMPLEMENTED TEST COVERAGE; HOSTED QUALIFICATION PENDING.** This is a provisional P006 scope because the authoritative `Synveil-v0.3-Context-Roadmap-P001-P060.md` was unavailable in the verified repository. P004-D01–D14 remain pending maintainer decisions; P006 does not introduce new policy.

## Starting integration
- P005A PR #164 merged at `dd82bb3ee01477a3bea0632e0febbe97113c7517`.
- P005 PR #163 merged at `e11a7778a2ed2ae5a825b6d95a911101fe7e337c`.
- The P005 explicit PostgreSQL 17 authorization workflow run [40181341086](https://github.com/nghianguyen150612/Synveil/actions/runs/40181341086) completed successfully on the tested P005 regression code.
- Production API and storage behavior are unchanged in P006.

## Enforcement map

| Entry point | Existing auth | Resource authority | Observed or test-gated behavior |
|---|---|---|---|
| `GET /api/v1/nodes/{node_id}/versions` | Browser-session authentication | `VersionHistoryBackend::list_file_versions(owner, node, cursor, limit)` | Existing bounded owner-scoped pagination; new P006 test checks foreign owner with real owner cursor, expects concealed 404 |
| `GET /api/v1/versions/{version_id}` | Authenticated inbound principal | `VersionHistoryBackend::get_file_version_metadata(owner, version)` | New test checks foreign current/historical version IDs, expects 404; no physical locator in version response |
| `GET /api/v1/nodes/{node_id}/content` | Authenticated inbound principal | `DownloadBackend` / `ContentReadApplicationService` requires owner-scoped metadata resolution before streaming | New mock-backed test denies ETag/Range substitution before open; existing local ObjectStore integration extended to verify foreign ETag/Range denial |
| `GET /api/v1/versions/{version_id}/content` | Authenticated inbound principal | Historical version ID resolved in owner scope before ObjectStore read | New mock-backed test and local ObjectStore integration deny copied foreign version ID even with correct ETag/Range |
| `GET /api/v1/nodes/{node_id}/versions?cursor=…` | Browser session | Cursor is pagination state, not authority | New test uses a valid owner-issued continuation cursor with another authenticated owner; expects 404 |

## Test coverage and isolation

| Case | Evidence source | Result type |
|---|---|---|
| Authorized version list/direct metadata, immutable historical pagination, no object key leak | Existing `crates/api/src/tests.rs` version tests | Existing mock-backend coverage; retain |
| Foreign version list, direct current/historical version IDs, valid copied pagination cursor | `p006_foreign_version_metadata_and_cursor_are_concealed` | New Axum/mock-backend regression; CI pending |
| Foreign current/historical content, original ETag, bounded Range and combined headers | `p006_foreign_current_and_historical_etag_range_cannot_open_bytes` | New Axum/mock-backend regression; verifies zero additional open/range calls; CI pending |
| Authorized full and Range content and denied foreign ETag/Range over on-disk objects | `api_downloads_use_the_content_service_and_local_object_store` | Existing Axum + real `LocalFilesystemObjectStore` integration, expanded negative test; PostgreSQL metadata remains mocked; CI pending |
| Owner-scoped immutable version history and cursor on SQLx repository | `postgres_version_history_is_owner_scoped_keyset_paged_and_pointer_based` | Existing **real PostgreSQL 17** metadata integration explicitly executed by P006 workflow; CI pending |
| Current/historical full/range object integrity, missing/cross-owner rejection before store open | `crates/storage/tests/downloads.rs` | Existing content application service test suite, memory-backed store plus filesystem test; CI pending |
| Cross-device mutation guard remains sound | `crates/api/tests/authorization_postgres.rs` | Existing P005 regression explicitly reexecuted against disposable PostgreSQL 17 by P006 workflow |

All new denial tests must confirm no unauthorized content response, no ETag/content-range disclosure and unchanged ObjectStore/mock open-call state. Existing local backend test additionally checks that denied foreign reads do not remove the immutable object. This is **not** a substitute for physical access-control hardening of a host filesystem.

## Evidence boundaries and remaining coverage

- New tests are committed; no passing result is asserted before the exact-head CI finishes.
- PostgreSQL metadata authorization and local filesystem ObjectStore transport are independently exercised, but the new Axum+LocalFilesystemObjectStore integration uses a synthetic metadata resolver, **not one transactional PostgreSQL+ObjectStore fixture**. That full composition must be qualified before claiming comprehensive production content-read security.
- Full cross-owner PostgreSQL-backed version/content routing, streaming race/TOCTOU adversarial tests, physical-object-key substitution beyond supported public API, revocation concurrency, client-version downgrade handling, and content upload/restore/GC/backup controls remain open.
- No release/native OS qualification is claimed; P028 Windows-native environment and P036 publisher-keyring release gates remain separate.
- P003 traceability: TM-006, TM-032–TM-036; SV-SEC-005 and related storage integrity requirements; partial AT-07, AT-34, AT-35, no full AT-15/AT-16 claim.
- This P006 implementation does not grant RBAC, root, cluster, API-token, snapshot or Tailscale authority.

## P007 gate

P007 may not claim P006 `COMPLETE` until the dedicated P006 workflow completes successfully on the exact reviewed head, the required integration/review gates pass, and any unqualified coverage required by the authoritative roadmap is resolved or explicitly approved as a scope deferral. A test-only PR merge is not evidence of global security or release readiness.

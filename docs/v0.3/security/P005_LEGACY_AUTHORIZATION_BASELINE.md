# P005 legacy authorization baseline

**Status: BLOCKED by a reproduced cross-device mutation authorization defect.**

This is a provisional, test-only v0.3 security-foundation increment based on
P003/P004. The authoritative `Synveil-v0.3-Context-Roadmap-P001-P060.md` was
not present in the repository or workspace, so its original P005 scope could
not be reconciled. Starting this work does not approve P004-D01–D14; all remain
pending maintainer decision.

The source baseline is `develop/v0.3` at
`3d06751bdaf47ffe518253f8c31080e751a6d380` (P004 merge PR #162). The probe is
`crates/api/tests/authorization_postgres.rs`, test
`p005_owner_and_device_authorization_probe_reveals_mutation_binding_gap`.
It runs the real Axum router and PostgreSQL services against a fresh child
database. Its final assertion intentionally records the current defect; it is
not a passing acceptance suite.

## Observed operation matrix

“Expected” describes the existing owner/device boundary targeted by the
authorization requirements. A difference between expected and observed is a
blocker, not a compatibility policy decision.

| Operation and principal | Owner/resource and binding | Expected | Observed and enforcement point | Evidence |
|---|---|---|---|---|
| `GET /api/v1/libraries`, browser A | A's library catalog | Permit A only | `200`; catalog contains A's library only. `files::list_libraries` supplies principal owner to `FileMetadataService::list_libraries`, which calls `list_owned_libraries`. | Probe: catalog assertions pass. |
| `GET /api/v1/nodes/{node_id}`, browser A | A root, then B root | Permit A; conceal B | A read `200`; B read `404`. `files::get_node` calls `get_node(owner, id)`; `find_owned_node` scopes the lookup. | Probe: own and foreign node assertions pass. |
| `GET /api/v1/libraries/{library_id}/nodes`, browser A | B library ID | Deny without existence leak | `404`. `files::list_children` resolves an owner-scoped parent and lists owner-scoped children. | Probe: foreign library assertion passes. |
| `PATCH /api/v1/nodes/{node_id}`, browser A | B root and a valid server-issued ETag | Deny; preserve target | `404`; B's stored name remains unchanged. `files::update_node` passes the authenticated owner to the metadata rename/move operation; repository owner-scoped lookup enforces the boundary. | Probe: foreign rename and direct unchanged-state assertions pass. |
| `GET /api/v1/auth/session`, missing/invalid, device bearer, browser session | Session identity class | Reject missing/invalid and device bearer; accept valid browser session without manufacturing a device identity | Missing/invalid and device bearer `401`; valid browser session `200` with no `device_id`. Browser-only `require_authentication` rejects Authorization headers. | Probe assertions pass. |
| Authenticated request with both browser cookies and device bearer | Ambiguous principal | Deny | `401`; inbound auth rejects a request combining cookie and bearer. | Probe assertion passes. |
| `GET /api/v1/devices/{device}/libraries/{library}/checkpoint`, device A2 | A2 and A library | Permit | `200`; API checks bearer-to-path device with `require_device`, then sync backend receives owner/device/library. | Probe assertion passes. |
| Same checkpoint route, device A1 bearer claiming A2 | A2 and A library | Deny cross-device impersonation | `404`; `sync::get_checkpoint` calls `AuthenticatedPrincipal::require_device`. | Probe assertion passes. |
| Same checkpoint route, browser A session | A2 and A library | Existing behavior must be preserved and recorded | `200`, with A2 in the response. The browser principal is not converted to a device identity. | Probe assertion passes; do not infer device-only access for this read. |
| Same checkpoint route, device A bearer against B library | A device and B library | Deny | `404`; device identity is checked at the route and sync service scopes the owner/device/library. | Probe assertion passes. |
| Same checkpoint route, device B before/after owner B revokes its credential | B device and B library | Permit before revoke; reject later use | `200`, then `401`. `device_auth::revoke_credential` revokes under the browser owner's identity; subsequent bearer authentication fails. | Probe assertions pass. |
| `GET /api/v1/auth/session`, expired session; then session after logout | A browser session | Deny | Both return `401`; expiry is set directly in the disposable database, without a sleep. | Probe assertions pass. |
| `POST /api/v1/devices/{device}/libraries/{library}/mutations`, device A2 bearer | A2 and A library | Permit authorized mutation | `200`; node count increases for the valid mutation. | Probe assertion passes. |
| Same mutation route, device A1 bearer naming A2 | A2 and A library | Deny; leave durable state unchanged | **Defect:** `200`; node count changes from `2` to `3`. `mutations::submit` passes the URL's device ID and principal owner to `ClientMutationService`; `load_client_mutation_scope` checks that the URL device and library are active and belong to that owner, but does not compare the device ID to the authenticated device principal. | Probe's final assertion fails with the observed status and state change. Reproduced against local disposable PostgreSQL 17. |
| File-version metadata/history, current or historical content, storage object keys | Owner/resource and library bindings | Deny foreign resource identifiers and do not expose content or mutate objects | Not exercised by this probe. Source paths include `versions::{list_versions,get_version}`, `downloads::{current_content,historical_content}`, and the download metadata/ObjectStore adapters. No P005 evidence claim is made. | **Unqualified.** |
| Sync feed/acknowledgement and mutation replay/conflict rules | Device/library and protocol context | Preserve current protocol rules; deny wrong device/library | Checkpoint path only was exercised. `sync::{get_changes,acknowledge}` and mutation replay/conflict cases remain outside this probe. | **Unqualified.** |

## Traceability and limits

| Reference | P005 evidence | Status |
|---|---|---|
| `TM-006` / `SV-SEC-005` / `AT-07` | Owner catalog, node, library, metadata mutation, checkpoint and device-mutation cases above. | Partial; cross-device mutation case violates the targeted boundary. No complete route/action/resource matrix. |
| `TM-013` / `SV-SEC-010` / `AT-15` | No future role or policy grants are created or compared. | Unverified; legacy-only cases cannot establish future-policy equivalence. |
| `TM-014` / `SV-SEC-011` / `AT-16` | Direct expiry and credential/session revocation checks; no concurrent policy/revocation schedule. | Partial for current credentials; policy freshness and concurrent revocation are unverified. |
| P004-D01–D14 | No policy decision implemented or approved by this increment. | Pending maintainer decision. |

The probe uses synthetic identities and credentials, two owners, one library
per owner, two credentials/devices for owner A, one for owner B, and an isolated
PostgreSQL 17 child database. It covers metadata reads, one metadata write,
session/device class separation, checkpoint scope, expiry/revocation, and the
device-scoped mutation binding defect. It does not establish coverage for file
versions/content/ObjectStore-key isolation, all sync routes, malformed IDs,
mutation replay/conflicts, or cross-owner writes beyond rename.

## Security stop and P006 handoff

The reproducible exploit requires a valid active device credential for one
device and a request naming another active device belonging to the same owner,
with that owner's active library and a valid mutation context. The API accepts
and persists the mutation under the named device scope. The dedicated failing
test and observed database change are the evidence. Production repair is out of
P005's authorized test-only scope; no production files were changed.

Before P006 proceeds, obtain the authoritative roadmap, resolve the applicable
P004 decisions, and qualify a narrowly scoped fix for binding authenticated
device identity to device-scoped mutation submission. Then add coverage for
file-version/content and storage boundaries, all sync operations, malformed
resource identifiers, mutation replay/conflict behavior, and revocation races.
Keep AT-15/AT-16 and full P003 acceptance explicitly open until their distinct
future-policy and concurrency evidence exists.

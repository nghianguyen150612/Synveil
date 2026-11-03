# P025 identity and authorization foundation

Status: provisional reference qualification; not production authorization. Base `bb577f91210796db21b4c3005ea4268fe89b0055`. The authoritative P001–P060 roadmap was not found in the workspace; reconciliation remains open. P004 decisions are not approved by this work.

## Existing authority and decision points

| Boundary | Source and enforcement |
|---|---|
| Browser | `auth::AuthenticationService` verifies a password and issues a session; `metadata::auth` persists session digest, expiry and revocation. `api` authenticates the session and binds browser mutations to CSRF proof. Login alone is not approval. |
| Device | `auth::DeviceAuthenticationService` authenticates credentials against metadata state, including active owner/device and revocation. Enrollment grants are scoped, expiring and consumed in metadata; they do not grant browser session authority. |
| API | `AuthenticatedPrincipal` distinguishes browser and device. `require_device` binds device-scoped sync and mutation requests to the bearer device. Browser behavior on existing routes is retained; mixed bearer/cookie authentication is rejected. |
| Resources | API file, version, download, sync and mutation handlers pass authenticated owner/device scope to services. Metadata repository owned-library/node/version lookups and sync/mutation scope checks enforce owner/resource relationships. See P005, P006 and P015 for observed cases and limits. The P005 cross-device mutation defect was repaired; the historical baseline is not a current defect claim. |
| Pairing | P020–P024 are qualification contracts; QR or network membership does not establish ownership or admin authority. Production pairing remains disabled. |

No general production role table, approval service or CLI token policy exists. Host/service principals are not inferred from network identity; unknown principal types fail closed.

## Reference semantics

`authorization_reference` is isolated and has no production callers. A trusted snapshot supplies ownership, vocabulary, assignments, provenance, revocation and policy revision. These are not request claims; using untrusted data here voids the model. Grants bind principal, host, owner, device, resource type and exact ID, and operation. Owner-origin and owner-issued delegation are explicit provenance fixtures, not a production role taxonomy. The role vocabulary is test-provided. A valid explicit deny wins over an allow; unknown vocabulary, invalid provenance, missing authority, expiry, revocation and stale revisions fail closed. The reference approval evidence must be supplied by a trusted independent authority, with distinct stable actor IDs and exact requester, binding, revision and expiry. This in-memory structure does not verify signatures, issue approval, provide durable audit or consume single-use evidence. These are production prerequisites, not implemented guarantees.

`qualify_legacy` illustrates the pending D05 recommendation:

| Legacy | Proposed | Effective |
|---|---|---|
| allow | allow | allow |
| allow | deny | deny |
| deny | allow | deny |
| deny | deny | deny |

Other proposed outcomes remain non-allow. Existing sessions, credentials, API and device behavior are unchanged; no roles are imposed on existing users. App permission does not grant OS privilege. Approval and OS authorization are separate trust boundaries. D05/D06 remain pending.

## Qualification and limits

The test fixtures exercise unknown/unauthenticated principals, browser/device confusion, cross-owner/library/host/device scope, stale or revoked principal/device/assignment, expiry, forged or unsupported roles and actions, missing grants, denial precedence, approval binding, legacy intersection, replay, and OS privilege separation. A mutex/barrier fixture illustrates ordering after a revocation fence; it does not establish PostgreSQL transaction isolation, multi-process safety or production revocation fencing. More exhaustive device, approval and concurrency integration qualification remains necessary before production.

## Open gates

P004-D05/D06/D09/D12/D13 and all remaining P004 decisions, P028 native Windows acceptance, P036 publisher signing, independent security review, physical Android/iOS pairing and recovery, real LAN/Tailscale testing, and roadmap reconciliation remain open. The historical Windows installer secret-marker failure remains unclassified despite a later passing resilience workflow. Production work requires approved policy taxonomy, ownership/delegation governance, authenticated and atomic authority snapshots, revocation fencing, independent approval and audit lifecycle, migration/rollback and platform privilege checks. No new production RBAC, tokens, schema or privileged endpoint is added here.

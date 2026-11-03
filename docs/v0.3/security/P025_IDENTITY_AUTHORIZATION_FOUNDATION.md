# P025 identity and authorization foundation — incomplete draft

Base: `bb577f91210796db21b4c3005ea4268fe89b0055`. The authoritative P001–P060 roadmap is unavailable; scope remains provisional. This draft does not approve P004 decisions or enable production RBAC.

## Existing authority

Browser sessions authenticate an owner; API CSRF checks protect browser mutations. Device credentials authenticate owner and device; enrollment grants and credential revocation are maintained by the existing authentication and metadata services. API routes bind device-scoped requests to the authenticated device and metadata queries scope libraries and nodes to the owner. P005 documents the historical mutation binding defect and its repair; P006 and P015 document further content and storage boundaries. Pairing contracts P020–P024 are reference qualifications, not production pairing.

## Reference model

The isolated auth module describes browser and device principals, unauthenticated and unknown callers, and exact host, owner, device, resource and action bindings. Trusted snapshot inputs supply assignments and vocabulary; caller claims must never be used as that snapshot. Assignment revocation, expiry, policy revision, principal revocation and exact scope are checked. Approval-required returns a separate result; it does not accept claimed approval. Application authorization grants no OS privilege. The fixture vocabulary has no shipped role taxonomy. The legacy intersection is a reference only: both decisions must allow.

This draft is incomplete: explicit ownership and delegation, independent approval evidence, comprehensive negative and race fixtures, and production-grade authority provenance need further design and validation. An in-memory test cannot prove PostgreSQL concurrency. No production consumer should use this model.

## Gates

P004-D05, D06, D09, D12 and D13 and remaining P004 decisions remain pending. Production prerequisites include approved roles and scope, policy and revocation fencing, trusted authority provenance, approval/audit policy, migration and rollback, plus independent security review. P028 Windows acceptance, P036 signing, physical mobile pairing/recovery, real LAN/Tailscale qualification, roadmap reconciliation and the historical Windows installer secret-marker investigation remain open. The latest installer resilience pass does not classify that historical failure.

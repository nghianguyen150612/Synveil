# Synveil API architecture

Status: **Foundation transport, browser/bootstrap auth, logical metadata, and exact-offset resumable upload HTTP transport implemented; upload UI, download, sync, and backup remain PLANNED**

This document defines the target HTTP contract and the blueprint for
`api/openapi.yaml`. The foundation currently implements bounded health transport,
the browser authentication subset (`POST /auth/login`, `POST /auth/logout`,
`GET /auth/session`, and `GET /auth/csrf`), and the first-run bootstrap subset
(`GET /system/bootstrap-status` and `POST /bootstrap/admin`). The React setup,
login, session, route-guard, and logout shell is covered by frontend tests.
The owner-scoped logical metadata subset is also implemented:

- `GET /libraries` with bounded owner-library pagination;
- `GET /libraries/{library_id}/nodes` with bounded active-child pagination;
- `POST /libraries/{library_id}/nodes` for empty logical directories;
- `GET/PATCH /nodes/{node_id}` for safe reads, rename, and move;
- `POST /nodes/{node_id}/trash` and `POST /nodes/{node_id}/restore` for
  conditional logical state changes.

These metadata routes use slash-separated action paths because the current
Axum path grammar does not support a parameter followed by a literal suffix in
one segment. They remain metadata-only. The repository also exposes the
transport-neutral persisted upload-session service through authenticated
`/upload-sessions` create/status/exact-offset append/complete/abort routes.
Mutations use the existing CSRF boundary, PATCH bodies stream raw bytes under
the service-configured limit, and status is the authoritative recovery path
after an ambiguous response. Upload UI, download, sync, backup, sharing,
device, and other product endpoints described below remain planned.
The canonical entity meanings and states come from
[DOMAIN_MODEL.md](DOMAIN_MODEL.md); storage, upload, sync, backup, photo, AI,
and integration specifications refine behavior without inventing alternate
IDs, errors, or mutation semantics.

## Contract boundary

All supported product clients use the versioned API:

```text
/api/v1
```

The canonical unversioned HTTP probes are `/health/live` and `/health/ready`.
The foundation also serves `/live` and `/ready` as short compatibility aliases.
These routes are deployment signals, not product resource APIs, and expose no
version, topology, or dependency details. The restricted operational view
remains under `/api/v1/system/health`.

Caddy may terminate TLS and route traffic, but proxy behavior is not part of
domain semantics. Clients never connect to PostgreSQL, construct object-store
keys, or call optional workers directly.

The API is responsible for:

- authenticating a principal and authorizing every resource action;
- parsing bounded, typed input before domain execution;
- enforcing quotas, rate limits, idempotency, and optimistic concurrency;
- streaming bytes without loading complete files into memory;
- mapping domain outcomes to stable HTTP and machine error contracts;
- committing metadata, journal, audit, and outbox facts together; and
- exposing freshness and asynchronous-operation state honestly.

The API is not responsible for making thumbnail, OCR, AI, notification, or
Forgejo indexing success a prerequisite for a core content commit.

## Protocol profile

### Transport and media types

- Production traffic uses HTTPS. Plain HTTP is permitted only on an explicitly
  private container hop behind a correctly configured TLS proxy.
- JSON request and response bodies use `application/json` with UTF-8.
- Canonical content upload and download use a binary body with an explicit
  `Content-Type` and bounded `Content-Length` or reviewed streaming framing.
- The server rejects unsupported media types and ambiguous duplicate headers.
- Successful empty responses use `204 No Content`. A JSON response never
  returns an empty body while claiming `application/json`.
- Compression of API JSON or download responses is negotiated at HTTP level;
  it does not alter the canonical `Object` integrity hash.

### Names, IDs, time, and numbers

- JSON property names are `snake_case`.
- Public IDs are opaque UUIDv7 strings. Their lexical order has no API meaning.
- Instants are UTC RFC 3339 strings. Date-only values use `YYYY-MM-DD` only
  where the domain truly represents a civil date.
- Revisions, change/entry sequences, byte lengths/counts, quota/accounting
  quantities, and any other non-negative value that can exceed JavaScript's
  safe integer range are canonical unsigned decimal strings. OpenAPI uses one
  shared scalar with pattern `^(0|[1-9][0-9]*)$` and a named
  `uint64-decimal` extension/format. Leading signs, whitespace, exponent
  notation, fractions, and leading zeroes are invalid.
- Bounded protocol controls such as page `limit`, part index, attempt count,
  and enumerated progress percentage remain JSON integers with explicit schema
  minima/maxima. A field never changes between number and string by client,
  value, or deployment.
- ETags, keyset/change cursors, IDs, and hashes remain opaque strings and are
  not numeric scalars.
- Hash values include their algorithm, for example
  `sha256:<lowercase-hex>`. Clients do not use a hash as an object ID.
- Unknown JSON properties in a request are rejected for security-sensitive
  commands and handled according to the reviewed schema elsewhere. Responses
  may add optional fields; clients ignore fields they do not understand.
- Nullable and omitted are different. Omitted means “not supplied/not
  selected”; `null` is allowed only where a schema assigns a domain action.

### Resource representation

A single-resource success uses:

```json
{
  "data": {
    "id": "opaque-uuidv7",
    "type": "node",
    "revision": "7",
    "attributes": {}
  },
  "meta": {
    "request_id": "opaque-request-id"
  }
}
```

A normal resource collection uses a `data` array and:

```json
{
  "page": {
    "next_cursor": "opaque-or-null",
    "has_more": true
  },
  "meta": {
    "request_id": "opaque-request-id"
  }
}
```

Protocol-specific feeds may use a registered specialized envelope where its
semantics require one. In particular, the sync contract uses `events`,
`next_cursor`, and `has_more` as defined in [SYNC.md](SYNC.md); it does not
silently alternate between the generic and feed envelopes.

Resource relationships may be represented by IDs or links, but recursive
embedding is bounded and never substitutes for authorization. Sensitive
internal fields—password verifiers, token hashes, storage keys, secret
references, raw provider errors, and server paths—have no public schema.

### Request correlation

- The server returns `X-Request-Id` on every response and mirrors the value in
  JSON error/meta bodies.
- A syntactically valid client request ID may be accepted as a correlation
  hint, but the server creates its own trusted trace identity and prevents log
  injection.
- Request IDs are diagnostic identifiers, not idempotency keys and not
  credentials.

## Authentication and authorization

### Planned credential profiles

- Browser sessions use high-entropy opaque credentials in `Secure`,
  `HttpOnly` cookies with an appropriate `SameSite` policy. The server stores
  only a verifier/hash.
- State-changing cookie-authenticated requests require the selected CSRF
  defense and an allowed-origin check; CORS is deny-by-default.
- Device/API clients use independently scoped, rotatable, revocable opaque
  bearer credentials associated with a `Device` or API grant. They travel in
  the `Authorization` header, never a URL, and do not reuse a user's password.
- Refresh/rotation is one-time and replay-detecting. Logout/revocation is
  idempotent.
- Public share access uses a separate bounded capability flow; it never becomes
  an ordinary user session or reveals the source owner's credential.

Every operation also authorizes the action against current ownership,
library/share relationship, device scope, and resource state. A `404` may be
returned instead of `403` where distinguishing existence would leak another
user's resource.

Authentication endpoints are intended to receive deployment and application
abuse controls, but this phase does not include a distributed rate-limiter
subsystem. Logs, errors, list/read responses, and all responses except the
explicitly named first one-time issuance response never include a password,
refresh secret, API/device credential, share capability, recovery code,
object-store credential, or Git integration token. The bootstrap and browser
authentication responses use `Cache-Control: no-store`, are redacted from
tracing, and exclude raw material from persisted idempotency outcomes and
replay.

## Conditional mutation

Mutable resource reads return a quoted `ETag` derived from the opaque resource
identity and domain `revision`. The encoding is not client-manufactured and
does not expose SQL transaction IDs.

- `PATCH`, `PUT`, and `DELETE` of an existing mutable resource require
  `If-Match` unless the endpoint has a stronger named base-version field.
- A documented desired-state relation endpoint may be an explicit exception:
  for example, favorite PUT means “ensure my relation exists” and DELETE means
  “ensure it is absent.” Such an endpoint must be idempotent, must name the
  exception in OpenAPI, and must honor a supplied relation `If-Match`; callers
  may not infer this exception for ordinary mutable resources.
- A recursive subtree command also presents the server-issued opaque subtree
  precondition defined by [SYNC.md](SYNC.md) OD-SYNC-004. It is separate from
  the node metadata ETag so a client cannot approve deletion using a view that
  predates a descendant edit. The exact OpenAPI field/header and backing schema
  are frozen at the Phase 1 schema/API gate before Phase 2 recursive Trash, not
  invented by individual clients; Phase 4 reuses that contract for sync.
- Missing precondition returns `precondition_required`.
- A stale precondition returns `version_conflict` with the safe current
  revision/ETag only if the caller may still read the resource.
- File-content replacement declares `base_version_id` during initiation. The
  sync conflict policy may preserve a stale offline edit instead of simply
  rejecting it, but never silently overwrites bytes.
- Immutable versions, committed snapshots, audit facts, and repository backup
  manifests are not patched. A restore creates a new resource/state.
- Collection-level commands validate each target's precondition or use a
  server-created operation snapshot. They never apply an unbounded,
  partly-authorized wildcard.

## Idempotency and retry

### Required key

`Idempotency-Key` is required for non-naturally-idempotent commands, including:

- upload initiation when replay could create duplicate sessions;
- upload completion;
- node copy and large/bulk operations;
- restore of a file version, trash entry, backup snapshot, or repository;
- share/public-link creation;
- API/device credential issuance or rotation and recovery-code-set generation;
- backup snapshot and repository-backup commit; and
- integration-triggered backup or reindex commands.

The key is an opaque client random value of a bounded length. The server scopes
it to authenticated principal, operation family, and target boundary; hashes a
canonical request fingerprint; and stores the terminal HTTP status, safe
headers, secret-redacted response body, and created resource IDs in the same
transaction as the outcome.

An identical retry returns the stored outcome and marks the response as a
replay through a documented header. Reusing the key with different material
input returns `idempotency_conflict`. A concurrent duplicate waits for or
observes the single outcome; it does not execute independently.

Secret-issuing endpoints are the named exception to response-body replay. Raw
API/device credentials, public-link capabilities, and recovery-code sets are
returned only in the first successful response, excluded from the persisted
idempotency outcome, and never reconstructed. A same-key replay returns safe
candidate metadata with `one_time_secret_unavailable`. Because the candidate
is still `PENDING` and unusable, the authorized caller can revoke or explicitly
replace it with a fresh issuance command and new key; an identical retry never
silently creates another secret.

### Retry classification

- Clients may automatically retry only when the error says `retryable: true`
  and the method is naturally idempotent or has a persisted idempotency key.
- `Retry-After` is returned for rate limiting and bounded service backoff when
  known.
- Transport failure after sending a body is an unknown outcome. The client
  queries the resource/session or retries with the same key; it must not choose
  a new key until it has observed that outcome. For a one-time-secret endpoint,
  the safe replay then directs an explicit revoke/replace command rather than
  replaying secret material.
- The server never reports a core mutation as successful before its database
  transaction commits and its referenced object is durable and verified.

## Pagination, sorting, filtering, and snapshots

Normal collections use keyset cursors:

- `limit` is bounded by the OpenAPI operation;
- `cursor` is opaque, versioned, integrity-protected, and scoped to principal,
  collection, filter, sort, and authorization context;
- `next_cursor: null` and `has_more: false` end traversal;
- changing the filter or sort requires a new traversal;
- a malformed token returns `invalid_cursor`, while an expired retained
  snapshot/change position returns the domain-specific expiry response.

Offset pagination is not a public default. Supported sort keys are enumerated;
arbitrary field names or SQL expressions are rejected. Every sort has an
immutable unique tie-breaker, normally the resource ID.

Search cursors also bind query normalization, search layer, index generation,
and authorization scope. If an index changes too far to honor a cursor, the
API expires it instead of silently mixing incompatible rankings.

### Sync rebaseline

An initial or stale client uses the server-directed sync bootstrap:

1. `POST /libraries/{library_id}/sync-bootstrap` creates a bounded bootstrap
   lease, captures current epoch/head `H` on the primary, and pins required
   journal retention. It does not expose a usable resume cursor prematurely.
2. The client pages the current node projection in immutable ID order using
   the bootstrap token. Mutations may continue; repeats are harmless.
3. After the listing finishes, completion returns a server cursor positioned
   at exactly `H`.
4. The client atomically installs its staged listing, consumes `changes` after
   that cursor, and applies events idempotently by node revision/version.
5. If the bootstrap lease or journal retention expires, the client discards
   the incomplete staged view and restarts.

This avoids a database transaction spanning HTTP requests. A node created
after `H` may appear in both listing and journal; a node deleted after `H` may
be absent from listing but still has a deletion event. Revision-aware
application converges both cases.

## Error contract

Every non-success JSON response uses:

```json
{
  "error": {
    "code": "version_conflict",
    "message": "The resource changed before this request was applied.",
    "request_id": "opaque-request-id",
    "retryable": false,
    "details": {
      "resource_type": "node",
      "current_revision": "8"
    }
  }
}
```

`code` is stable and machine-readable. `message` is safe, human-readable, and
not intended for branching. `details` is an allow-listed schema per code and
must not contain stack traces, SQL, filesystem paths, secret values, raw
provider bodies, another user's IDs, or content excerpts. Internal diagnostics
are correlated by request ID in redacted logs.

### Error presentation boundary

The API contract is the stable source beneath every client surface. A response
may expose a non-secret remediation/action key in addition to its code,
retryability, safe message/detail, and request ID; it never exposes a raw path,
secret, provider body, stack trace, or privileged diagnostic bundle. The same
underlying state is presented at three levels:

- **User:** plain language, the next safe action, and whether retrying is safe;
  ordinary users should not need to interpret HTTP, SQL, filesystem, or service
  terminology.
- **Administrator:** the user explanation plus stable code, health context,
  affected backend/service, and an authorized diagnostic reference.
- **Developer/operator:** redacted structured diagnostics correlated by request
  ID, with the original failure classification and remediation evidence.

The presentation layer must not invent a different state or silently turn an
unknown outcome into success. Examples include mapping a full storage backend
to “Storage is full; choose another location or free space,” an unavailable
database to “Synveil is repairing its local service; try again shortly,” an
expired/reused pairing code to “Generate a new pairing code,” an unsupported
filesystem capability to “This storage location cannot perform this action,” a
failed signed update to “The update was not installed; keep the current version
and review the update details,” a migration requiring restore to “Restore from
the verified recovery package before continuing,” and unavailable remote
access to “Local access still works; check the configured network path.”

These messages are product copy, not alternate error semantics. The stable code,
retry rule, safe action, request ID, and authorization boundary remain available
to clients and diagnostics.

### Baseline error registry

| HTTP | Code | Meaning and retry rule |
|---:|---|---|
| 400 | `invalid_request` | Syntax, schema, or bounded validation failed; do not retry unchanged. |
| 400 | `invalid_cursor` | Cursor is malformed, wrong version, wrong query, or wrong resource scope; follow the operation's restart route. |
| 401 | `authentication_failed` | Credential missing, invalid, expired, rotated, or revoked. |
| 403 | `permission_denied` | Authenticated principal lacks the action; no automatic retry. |
| 404 | `not_found` | Resource absent or deliberately concealed by authorization policy. |
| 409 | `version_conflict` | ETag/base version or concurrent state no longer matches. |
| 409 | `name_conflict` | Destination comparison key already exists. |
| 409 | `idempotency_conflict` | Key was already bound to different material input. |
| 409 | `one_time_secret_unavailable` | The first secret-bearing response cannot be replayed; the returned candidate is inert and must be inspected, revoked, or explicitly replaced. |
| 409 | `invalid_state` | Resource exists but transition is not legal from its current state. |
| 409 | `part_conflict` | Upload part identity already has different verified content. |
| 409 | `completion_conflict` | Upload completion identity/manifest conflicts with the stored attempt. |
| 409 | `upload_sealed` | The upload manifest is frozen and no longer accepts part mutation. |
| 409 | `cursor_epoch_mismatch` | Cursor belongs to an old/new library journal epoch; perform directed bootstrap. |
| 409 | `unsupported_event_version` | Client cannot safely apply a required event schema; stop advancement and upgrade/recover. |
| 410 | `cursor_expired` | Retained change/bootstrap/search position is gone; follow rebootstrap instructions. |
| 410 | `upload_expired` | Upload session can no longer accept parts or completion. |
| 412 | `checksum_mismatch` | Observed bytes do not match the declared checksum; retransmit only verified source bytes. |
| 413 | `payload_too_large` | Body, part, manifest, result set, or operation count exceeds a documented bound. |
| 415 | `unsupported_media_type` | Representation is not accepted for this endpoint. |
| 422 | `invalid_manifest` | Required upload parts/ranges are missing, overlapping, inconsistent, or otherwise invalid. |
| 428 | `precondition_required` | Required `If-Match` or base version was omitted. |
| 429 | `rate_limited` | Retry according to `Retry-After`; key outcome has not been applied unless documented. |
| 500 | `internal_error` | Unexpected failure; outcome is unknown for a sent mutation, so reuse the same key. |
| 503 | `storage_unavailable` | Required backend/durability check failed; retry only when indicated. |
| 503 | `internal_dependency_unavailable` | PostgreSQL or an operation-specific required internal dependency is unavailable. |
| 507 | `quota_exceeded` | Logical/staging/account quota reservation failed; do not retry until capacity or policy changes. |

Operation-specific codes must be registered in one OpenAPI enum/extension
registry with a privacy-reviewed details schema. The backup protocol reserves
`backup_set_paused`, `snapshot_not_restorable`, `snapshot_incomplete`,
`manifest_conflict`, `capture_inconsistent`, `object_corrupt`,
`restore_conflict`, `unsupported_entry_type`, and `device_revoked` in addition
to common codes. Integration and AI protocols similarly register
`integration_reauth_required` and `ai_disabled` rather than returning raw
provider/worker strings. Adapters map their failures into this registry;
provider-specific diagnostics do not escape.

## Asynchronous operation contract

Commands that may traverse a subtree, restore many entries, generate a large
snapshot, or call an external integration return `202 Accepted` with
`Location: /api/v1/operations/{operation_id}`.

An `Operation` representation contains:

- opaque ID, type, owner and target links;
- state `QUEUED`, `RUNNING`, `SUCCEEDED`, `PARTIAL`, `FAILED`,
  `CANCEL_REQUESTED`, or `CANCELED`;
- created/started/finished instants;
- bounded progress counts/bytes when meaningful, never fabricated percentage;
- safe terminal error or a paginated per-item result;
- idempotency key correlation and ETag/revision.

Cancellation is conditional and best-effort. It never rolls back already
committed child mutations by pretending distributed work was atomic. A partial
operation names every committed, skipped, conflicted, and failed item so retry
can target the remainder.

## Endpoint inventory and implementation status

The four browser authentication routes, two bootstrap routes, logical metadata
routes, and exact-offset upload-session routes named above are `IMPLEMENTED`
and specified in `api/openapi.yaml`. Unmarked endpoint groups below are
`PLANNED`. Except for the two explicitly named absolute
`/health/*` probes, paths in the inventory are relative to `/api/v1`. A
collection route never removes the need to authorize each returned resource.

### Bootstrap, authentication, and users

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `GET /system/bootstrap-status` | **IMPLEMENTED** — Report only whether setup is required, without configuration secrets. | Public bounded status; `no-store`; deployment must keep first-run exposure trusted/private or correctly terminated by TLS. |
| `POST /bootstrap/admin` | **IMPLEMENTED** — Create the first administrator through the existing race-safe bootstrap service and close setup. | Public only while setup is open; strict 16 KiB JSON; same-origin provenance when browser headers are present; no setup-secret field or automatic session; explicit login follows. |
| `POST /auth/login` | **IMPLEMENTED** — Verify the canonical login identifier and password and issue a browser session cookie. | Public; bounded JSON input; raw session is Secure/HttpOnly cookie-only; no raw token JSON. |
| `POST /auth/refresh` | Atomically consume a refresh credential and issue its successor; detect replay. | Existing refresh grant; an ambiguous lost response is fail-closed and requires login again, not transparent retry. |
| `POST /auth/logout` | **IMPLEMENTED** — Revoke the current browser session and clear session/CSRF cookies. | Authenticated valid sessions require session-bound CSRF and same-origin provenance; absent/expired/revoked logout is safely repeatable. |
| `GET /auth/session` | **IMPLEMENTED** — Return the safe current browser principal only. | Authenticated browser session; no token, verifier, password, or database row. |
| `GET /auth/csrf` | **IMPLEMENTED** — Issue a fresh signed proof bound to the current browser session. | Authenticated browser session; response is `no-store`; proof is sent in `X-CSRF-Token`, never a query parameter. |
| `GET /sessions` | List the caller's safe browser/API session metadata, current-session marker, expiry, last use, and revocation state; device grants link to Devices. | Authenticated owner; keyset pagination; no token/verifier. |
| `POST /sessions/{session_id}:revoke` | Revoke one owned browser/API session family independently; this is the sole public revoke authority for an API grant. | Authenticated owner; `If-Match` and idempotency key; current session also clears its cookie; device revocation uses the Device endpoint. |
| `GET /api-grants` | List the caller's named API grants, scopes, status, expiry, last use, and safe generation metadata. | Authenticated owner; keyset pagination; no credential/verifier. |
| `POST /api-grants` | Create a bounded `PENDING` API grant/generation and display its raw bearer secret exactly once; it is unusable until activation. | Recent step-up; required `Idempotency-Key`; explicit least scopes/expiry; audited/rate-limited; one-time-secret rule; lost response leaves an expiring pending grant. |
| `POST /api-grants/{grant_id}:activate` | Activate a newly issued pending API grant after the owner confirms saving the secret. | Owner; grant `If-Match` plus idempotency key; no raw secret returned. |
| `POST /api-grants/{grant_id}/credentials:rotate` | With a new key, atomically expire/replace any prior pending candidate and generate one pending replacement while the current generation remains active. | Owner with recent step-up; grant `If-Match`; required `Idempotency-Key`; one-time-secret rule. |
| `POST /api-grants/{grant_id}/credentials/{generation}:activate` | Atomically activate the saved replacement and retire the prior credential generation. | Owner; generation precondition plus idempotency key. |
| `GET /users/me/recovery-code-status` | Read active/pending generation, remaining-code count, and safe timestamps only. | Authenticated owner; never returns verifier or raw code. |
| `POST /users/me/recovery-code-sets` | With a new key, atomically expire/replace any prior pending candidate; generate one bounded pending replacement set and display raw codes once while the old active set remains valid. | Recent step-up authentication; required `Idempotency-Key`; audited/rate-limited; one-time-secret rule; a lost response leaves only an expiring pending candidate. |
| `POST /users/me/recovery-code-sets/{set_id}:activate` | Confirm codes were saved, activate the pending set, atomically retire the prior set, and expire its pending recovery transactions/reservations. | Authenticated owner; candidate `If-Match` plus idempotency key; no raw codes returned. |
| `POST /auth/recovery/code-exchanges` | Validate/reserve one active unused code; atomically change any older transaction for it from `PENDING` to `EXPIRED`, rebind the reservation, and issue one short-lived `PENDING` transaction secret; do not consume the code yet. | Public, enumeration-resistant, serialized, strict account/source rate limits, secret fields redacted; retrying the same code safely replaces a lost response. |
| `POST /auth/recovery/password-reset` | Revalidate the referenced set/generation is `ACTIVE`; atomically consume both the current recovery transaction and its reserved code, replace the password, bump canonical `session_epoch`, revoke all `WEB`/`API`/`DEVICE` grants, and pause retained devices. | Public with valid one-use current transaction; audited; returns no normal session; every client must reauthenticate. |
| `GET /users/me` | Read the current user's safe profile and capabilities. | Authenticated owner. |
| `PATCH /users/me` | Change allowed profile/account settings. | Authenticated owner; `If-Match`. |
| `GET/POST /admin/users` | List/create accounts under instance policy. | Instance administrator; keyset pagination; create requires idempotency key. |
| `GET/PATCH /admin/users/{user_id}` | Read/lock/disable/administer an account without silent data purge. | Instance administrator; `If-Match`; audited. |

Bootstrap status fails closed when database state is unavailable or ambiguous,
and the create operation uses the existing serialized PostgreSQL service
boundary so only one administrator claim succeeds. A closed bootstrap state is
never re-enabled by deleting a browser cookie. The current HTTP contract has no
setup-secret field: until a later installer or secret-gate contract is reviewed,
operators must expose first-run endpoints only on a trusted/private or correctly
terminated TLS network and use the configured canonical origin. The endpoint
has a strict body bound and provenance checks, but no distributed rate limiter
is implemented in this phase.

The successful create response contains only `setup_required=false` and
request correlation metadata. It does not issue a browser session; the web
client transitions to explicit login. Passwords, verifiers, cookies, and raw
session/CSRF material are never returned in the bootstrap response or logged.

Browser refresh prioritizes theft response over transparent availability. If
the consume-and-issue transaction commits but its response is lost, reuse of
the consumed credential deterministically sets that `Session` and all its
non-terminal credentials to `REVOKED`; the client discards it and performs a
fresh login. Both ambiguous loss and malicious replay return the generic
`authentication_failed` response while audit records
`REFRESH_REPLAY_DETECTED`. No public quarantine state exists, and the old
credential is never accepted again.

The initial recovery contract is recovery-code only. A generated candidate is
not active until explicit confirmation, so losing the one-time response cannot
invalidate the previous active set. Code exchange reserves rather than spends
the code: retry replaces the unreachable pending transaction, and only password
reset atomically consumes both current transaction and code. Neither endpoint
accepts an email/admin override.
Those optional paths remain blocked on OD-005. Session revocation is checked on
the next authenticated request subject only to a documented bounded cache lag.
API-grant issue/rotation uses the same pending-then-activate safety rule as
recovery-code replacement: no lost one-time response can create an unknown
usable secret. A grant carries only explicit bounded scopes and never amplifies
the owning user's current authorization.

`ApiGrant` is the API-specific projection of `Session.kind = API`;
`grant_id` is exactly `Session.id`, not a second identity. Credential
generations are child records, so retiring a generation does not introduce a
`RETIRED` session status. `POST /sessions/{session_id}:revoke` is the only
public family-revocation command and atomically revokes every remaining
generation; `/api-grants` deliberately defines no competing revoke route.

### Devices and credentials

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `POST /devices` | Register a named `PENDING` device and negotiate protocol capabilities without issuing a credential. | Authenticated user; ordinary replayable idempotency key. |
| `GET /devices` | List the caller's devices and freshness/status. | Owner; keyset pagination. |
| `GET/PATCH /devices/{device_id}` | Read or rename/pause a device, policies, and safe credential-generation status. | Owner; `If-Match`; no verifier/raw credential. |
| `POST /devices/{device_id}/credentials` | For a new `PENDING` device, issue/replace one pending initial credential; for a password-reset `PAUSED` device, create and bind a fresh pending `DEVICE` Session/generation while the old family remains revoked. Display the raw bearer secret once. | Owner with recent step-up; device `If-Match`; required `Idempotency-Key`; one-time-secret rule. |
| `POST /devices/{device_id}/credentials:rotate` | Issue or replace one pending rotation candidate while the current generation remains active; display the candidate secret once. | Owner with recent step-up; device `If-Match`; one-time-secret idempotency rule. |
| `POST /devices/{device_id}/credentials/{generation}:activate` | Activate a saved initial/replacement generation; initial activation activates the device, while rotation retires the old generation atomically. | Authenticated owner with recent step-up; generation `If-Match` plus idempotency key; no secret returned and a pending credential cannot self-activate. |
| `POST /devices/{device_id}:revoke` | Revoke credentials and future activity; optionally queue a best-effort request to remove Synveil-managed cache. | Owner; `If-Match` plus idempotency key; audited; cache request is not proof of execution. |
| `GET /devices/{device_id}/status` | Read sync, backup, storage/cache, and last-contact state with freshness. | Owner; no client claim treated as verified server health. |

No response claims full remote operating-system wipe. An offline or compromised
device may never process the managed-cache request; server credential
revocation is the authoritative security action.

Future platform contract families will cover short-lived pairing sessions,
storage discovery and capability evidence, layered health/maintenance status,
signed update state, uninstall/data-preservation state, machine migration and
recovery, and remote-access configuration/status. They may be implemented over
the authenticated API, local platform IPC, or both. They are not added as
speculative public endpoints until schemas, authorization, idempotency,
redaction, and recovery semantics are stable; the checked-in
[`api/openapi.yaml`](../../api/openapi.yaml) remains unchanged by this
documentation revision.

A pending device credential cannot authenticate. Losing an issuance response
therefore leaves the device's prior state safe: no active credential for
initial registration, or the old active generation during rotation. After
observing `one_time_secret_unavailable`, the owner explicitly replaces the
pending candidate with a new issuance and key; activation is the only point
that retires the old generation.

Password reset changes retained non-revoked devices to `PAUSED` and leaves each
old credential family `REVOKED`. Owner step-up re-enrollment through
`POST /devices/{device_id}/credentials` creates/rebinds a fresh pending family;
activation alone returns that device to `ACTIVE`. A revoked device record is
never resurrected by this flow.

### Libraries, nodes, files, and folders

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `POST /libraries` | Create an ownership/policy/sync namespace and root node. | Authenticated user; idempotency key. |
| `GET /libraries` | **IMPLEMENTED** — List libraries owned by the authenticated user with bounded opaque cursor pagination. | Authenticated owner; inaccessible libraries are not returned. |
| `GET/PATCH /libraries/{library_id}` | Read or update safe library metadata/policy. | Authorized owner/admin; `If-Match`. |
| `GET /libraries/{library_id}/nodes` | **IMPLEMENTED** — List active direct children under the root or an active directory in stable node-ID order. | Authenticated owner; bounded opaque cursor scoped to library and parent. |
| `GET /libraries/{library_id}/favorites` | List the caller's readable active favorite nodes without exposing inaccessible relations. | Current user and library read; keyset pagination; authorization rechecked per result. |
| `GET /recent-nodes?library_id=...` | List caller-readable active nodes by recent committed server-side mutation, optionally within one library; this is not view tracking. | Authenticated; snapshot-watermarked keyset cursor bound to caller/filter; authorization rechecked per result. |
| `POST /libraries/{library_id}/nodes` | **IMPLEMENTED** — Create an empty logical directory only; no upload intent or physical directory. | Authenticated owner; strict 16 KiB JSON; raw logical names; duplicate siblings remain permitted by the current schema. |
| `GET/PATCH /nodes/{node_id}` | **IMPLEMENTED** — Read safe metadata or perform one conditional rename/move. | Node owner; signed `ETag`/`If-Match`; move validates destination and cycles transactionally. |
| `PUT /nodes/{node_id}/favorite` | Ensure the caller's personal `NodeFavorite` exists and return its relation ETag. | Current user with node read; desired-state idempotency key; optional relation precondition; never changes `Node`. |
| `DELETE /nodes/{node_id}/favorite` | Ensure the caller's personal favorite is absent. | Relation owner; idempotent and non-revealing when absent/inaccessible; optional relation `If-Match`. |
| `POST /nodes/{node_id}:copy` | Copy a node or enqueue a bounded subtree copy. | Source read plus destination write; idempotency key and destination precondition. |
| `POST /nodes/{node_id}/trash` | **IMPLEMENTED** — Logically delete one non-root node; non-empty directories are rejected while recursive subtree preconditions remain open. | Node owner; CSRF and `If-Match`; file versions/objects are untouched. |
| `POST /nodes/{node_id}/restore` | **IMPLEMENTED** — Restore one trashed node only when its existing parent remains valid and active. | Node owner; CSRF and `If-Match`; no guessed recovery destination. |
| `DELETE /nodes/{node_id}` | Request purge only when retention/policy permits. | Owner/admin; `If-Match`, idempotency key, explicit irreversible intent. |
| `GET /nodes/{node_id}/content` | Stream the authorized current version. | Node read; supports validators and ranges. |
| `GET /versions/{version_id}/content` | Stream one authorized immutable historical version. | Node/version read; supports ranges. |
| `POST /libraries/{library_id}/node-operations` | Execute a bounded multi-item move/copy/trash/metadata command. | Per-item authorization/precondition, including subtree preconditions where recursive; idempotency; synchronous only below reviewed bounds. |

The implemented metadata slice intentionally leaves the following decisions
explicit. The current PostgreSQL schema has no sibling `name_key` or unique
constraint, so duplicate raw logical names under one parent remain valid. The
service does not normalize Unicode, fold case, reject host-platform reserved
names, or construct filesystem paths. `POST /nodes/{node_id}/trash` is a
single-node logical transition: root nodes are protected and non-empty
directories return `invalid_state` until the recursive subtree precondition
decision in `SYNC.md` OD-SYNC-004 is closed. Restore uses the existing parent
only; a missing, deleted, or invalid parent returns a stable conflict rather
than guessing a recovery location. No `FileVersion`, `Object`, journal,
outbox, or physical-content row is changed by these operations.

Folder and file are `Node.kind` variants, not incompatible identity systems.
Raw `Object` URLs are not a browsing API.

Favorites are caller-scoped preferences, not shared library metadata. Setting
or clearing one never grants access, holds content against retention, or emits
a content-sync event. The server commits concurrent opposing desired states in
transaction order; a retry with the same idempotency key returns the stored
outcome after a lost response. Trash hides the node from the default Favorites
query while preserving the authorized preference for restore; purge or access
cleanup removes it.

Recent nodes are ordered by server-observed update time and immutable ID under
an `as_of` cursor watermark. Reads/previews/downloads do not change this list.
Each page omits newly inaccessible or trashed nodes and minimizes shared-root
context; a refresh starts a new watermark and includes later mutations.

### Downloads, ranges, and object diagnostics

- A content response supplies `Content-Length`, safe `Content-Type`,
  `Content-Disposition` with correctly encoded untrusted filename, immutable
  version ETag, and integrity metadata where exposing it is authorized.
- Single byte ranges follow standard HTTP range semantics. Invalid or
  unsatisfiable ranges fail without reading unbounded content. Multi-range
  support may be omitted initially and must be declared in OpenAPI.
- Conditional `If-None-Match` may return `304` for the exact immutable version.
- The API authorizes every request/range; an internal or future signed backend
  URL is short-lived, resource/version/range-scoped, non-loggable, and issued
  only after the same authorization and audit decision.
- `GET /objects/{object_id}/integrity` is an owner/operator diagnostic if
  retained in OpenAPI. It returns safe state/hash/verification evidence, never
  a storage key, backend credential, or authorization-bypassing content URL.
- Corrupt or quarantined content returns a stable failure and is never streamed
  with a success status.

### Resumable uploads

The currently implemented exact-offset subset is:

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `POST /upload-sessions` | **IMPLEMENTED** — Create a strict tagged `CREATE_FILE` or `REPLACE_CONTENT` intent with expected bytes and optional canonical SHA-256. | Authenticated principal becomes owner; CSRF; strict 16 KiB JSON; no client `user_id`, object key, or staging handle. |
| `GET /upload-sessions/{upload_session_id}` | **IMPLEMENTED** — Return safe state, target, expiry, completion, and authoritative offset. | Owner-scoped authentication; no CSRF; after any unknown PATCH outcome this is the only recovery oracle. |
| `PATCH /upload-sessions/{upload_session_id}` | **IMPLEMENTED** — Stream a non-empty raw chunk at exactly `Upload-Offset`. | Owner + CSRF; exact `application/octet-stream`; canonical unsigned decimal offset; aggregate service limit, 8 MiB by default; success and `invalid_offset` return the authoritative offset. |
| `POST /upload-sessions/{upload_session_id}/complete` | **IMPLEMENTED** — Delegate verification/promotion/logical commit and return canonical completion metadata. | Owner + CSRF; retry reuses the validated service's exactly-once outcome. |
| `POST /upload-sessions/{upload_session_id}/abort` | **IMPLEMENTED** — Delegate abort without direct filesystem or metadata manipulation. | Owner + CSRF; safe repeat. |

The handler does not aggregate a full PATCH body. Accepted transport frames are
durably progressed through the application service, so a lost response or a
stream failure can leave an accepted prefix. The client must GET status and
resume from `Upload-Offset`/`received_bytes`; it must never blindly replay the
old offset or locally increment progress. The typed browser helper sends
`Blob`/`ArrayBuffer` directly and no upload product UI is implemented.

The following richer part-manifest inventory remains `PLANNED`; its `/uploads`
paths are not aliases for the implemented `/upload-sessions` subset:

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `POST /uploads` | Initiate create/replace intent, expected length/hash, target library/destination, base version, and negotiate parts. | Library write; idempotency key; reserves quota. |
| `GET /uploads/{upload_id}` | Read state, expiry, constraints, bounded part/missing-range summary, progress, and terminal outcome/link relations. | Session owner/device; safe recovery after unknown response; never embeds an unbounded part list. |
| `GET /uploads/{upload_id}/parts?cursor=...` | Page authoritative verified part receipts/ranges/checksums for resume. | Session owner/device; opaque keyset cursor bound to session and part generation. |
| `PUT /uploads/{upload_id}/parts/{part_number}` | Stream one declared range with length and checksum. | Session owner; naturally idempotent only for identical fingerprint/bytes. |
| `POST /uploads/{upload_id}/complete` | Validate coverage, assemble/finalize, verify, and atomically expose one version. | Session owner; completion idempotency key required. |
| `DELETE /uploads/{upload_id}` | Abort `OPEN`, or durably request cancellation of `VERIFYING`/pre-commit `COMMITTING`; release reservation/staging or finalized orphan bytes through safe cleanup. | Session owner; `If-Match`; idempotent; may return `202` while a lease reaches a safe boundary; cannot undo `COMMITTED`. |

Initiation returns explicit part size/count/range limits, expiry, accepted
checksum algorithms, and any direct-upload capability. Completion cannot
reference `OPEN` or merely uploaded bytes; object durability and verification
precedes the metadata transaction. The complete state machine belongs to
[UPLOADS.md](UPLOADS.md).

### Versions and trash

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `GET /nodes/{node_id}/versions` | List immutable history with retention/conflict metadata. | Node history read; keyset pagination. |
| `GET /versions/{version_id}` | Read one authorized immutable version. | Node history read. |
| `POST /versions/{version_id}:restore` | Create a new head version from historical bytes. | Node write; current node `If-Match` plus idempotency key. |
| `GET /libraries/{library_id}/trash` | List retained trash entries. | Library read; keyset pagination. |
| `POST /trash/{trash_entry_id}:restore` | Restore with explicit destination/name collision policy. | Library write; idempotency and destination precondition. |
| `DELETE /trash/{trash_entry_id}` | Request permanent logical purge. | Owner; `If-Match`/explicit confirmation, idempotency, audited. |

Restoring a version creates a version and change event. Restoring trash does
not silently replace a conflicting active node.

### Change feed and sync

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `GET /libraries/{library_id}/changes?cursor=...` | Return ordered committed changes for the library after validating the cursor carries the same library/epoch/profile. | Library/device read scope; cursor required after bootstrap; bounded long poll may be added. |
| `POST /libraries/{library_id}/sync-bootstrap` | Establish bounded bootstrap lease and capture primary journal head `H`. | Library/device read scope; idempotency optional when request carries stable client bootstrap ID. |
| `GET /sync-bootstraps/{bootstrap_id}/nodes` | Page authoritative current projection in immutable ID order. | Bootstrap owner; opaque page cursor. |
| `POST /sync-bootstraps/{bootstrap_id}/complete` | Prove listing completion and return a cursor for exactly `H`. | Bootstrap owner; idempotent terminal outcome. |
| `DELETE /sync-bootstraps/{bootstrap_id}` | Release a completed/abandoned lease. | Bootstrap owner; idempotent. |
| `POST /devices/{device_id}/sync-checkpoints` | Acknowledge safely applied cursor/status for observability and retention policy. | Matching device; monotonic/idempotent; cannot skip server validation. |

Changes are scoped to one `Library` and ordered only by its sequence. The
cursor is not an authorization token. A client cannot ask the server to accept
an arbitrary numeric sequence. [SYNC.md](SYNC.md) owns event kinds, conflict
rules, tombstones, compaction, and rescan fixtures.

### Backup and restore

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `POST/GET /backup-sets` | Define a device source/exclusion/schedule/retention policy or list authorized sets. | Owner and matching device policy; create uses idempotency; list uses keyset pagination. |
| `GET/PATCH /backup-sets/{backup_set_id}` | Read/update policy/status. | Owner; `If-Match`. |
| `POST /backup-sets/{backup_set_id}/snapshots` | Begin a `BUILDING` snapshot and negotiate manifest/content submission. | Authorized source device; idempotency key. |
| `GET /backup-snapshots` | List authorized snapshots with state, consistency, and retention. | Owner; backup-set filter and keyset pagination. |
| `GET /backup-snapshots/{snapshot_id}` | Read one snapshot and safe verification/completeness summary. | Owner; no object locator exposure. |
| `POST /backup-snapshots/{snapshot_id}/entry-batches` | Submit bounded, checksummed manifest batches. | Source device; batch identity and idempotent replay. |
| `POST /backup-snapshots/{snapshot_id}/content-claims` | Bind entries to verified/reusable content under domain and quota checks. | Source device; bounded idempotent claims; never trusts a client hash alone. |
| `POST /backup-snapshots/{snapshot_id}/complete` | Seal, verify, and atomically make the complete manifest restorable. | Source device; idempotency key; one terminal outcome. |
| `GET /backup-snapshots/{snapshot_id}/entries` | Browse immutable manifest. | Owner; path/ID keyset pagination. |
| `POST /restores` | Create a verified restore operation from an explicit snapshot/version to an explicit destination. | Owner; idempotency, non-destructive destination policy, async operation. |
| `GET /restores/{restore_id}` | Read restore state, verification summary, and safe terminal result. | Owner; ETag/freshness. |
| `GET /restores/{restore_id}/entries` | Page per-entry restore results. | Owner; keyset pagination. |
| `POST /restores/{restore_id}/resume` | Resume eligible failed/pending entries without duplicating completed work. | Owner; idempotency and restore precondition. |
| `POST /restores/{restore_id}/cancel` | Request best-effort cancellation without pretending to undo committed output. | Owner; conditional/idempotent intent. |

`BUILDING` and `FAILED` snapshots are never described as restorable. A missing
source path is not a live deletion command. [BACKUP.md](BACKUP.md) owns
retention, symlinks, snapshot consistency, restore verification, and
device-loss recovery.

### Shares

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `GET /shares?direction=received` | Populate “shared with me” from active private grants and a minimal authorized share-root node projection. | Authenticated grantee; authorization rechecked per result; keyset cursor bound to caller, direction, status, and sort. |
| `GET /shares?direction=sent` | List grants created/administered by the caller, with an explicit pending/active/expired/revoked status filter and no secrets. | Grantor/delegation administrator; keyset pagination; per-result authority check; lost pending links remain manageable. |
| `POST /nodes/{node_id}/shares` | Create an active private user grant, or a `PENDING` public-link grant whose raw capability is displayed exactly once. | Resource owner with delegation right; idempotency key; public link follows the one-time-secret rule and cannot authorize before activation. |
| `GET /nodes/{node_id}/shares` | List safe grants without link/password secrets. | Owner/delegation administrator; keyset pagination. |
| `GET/PATCH /shares/{share_id}` | Read/change permission, expiry, or allowed settings. | Grant owner/admin; `If-Match`. |
| `POST /shares/{share_id}:activate` | Activate a pending public link after the owner confirms saving its capability. | Grant owner/admin; `If-Match` plus idempotency key; no capability returned. |
| `POST /shares/{share_id}:revoke` | Revoke future access. | Grant owner/admin; `If-Match` and idempotency; audited. |
| `POST /public-share-sessions` | Exchange a capability/password for a short-lived, share-scoped access context. | Public, strict rate/abuse limits; secret-bearing fields redacted. |
| `GET /public-share-sessions/{session_id}/content` | Browse/download only the resolved share scope. | Valid share context; rechecks expiry/revocation and range scope. |

The public browser link should keep the high-entropy capability out of proxy
query logs where practical, for example by using a URL fragment and posting it
from the static client. Exact transport is a Phase 3 decision; it cannot weaken
revocation or CSRF/XSS controls.

A lost public-link creation response leaves only an inert `PENDING` share.
Same-key replay returns `one_time_secret_unavailable` and safe share metadata,
never the capability. There is no in-place capability replacement: the owner
must revoke the pending share, create a new share with a new key, and activate
only a capability known to be saved. Activation races with revoke/expiry under
the share revision; one terminal transition wins and no pending link is
accepted by `POST /public-share-sessions`.

The received collection returns only currently authorized private shares. Its
node projection begins at the shared root and cannot disclose unreadable
ancestors, other grantees, storage locators, or public-link material. Revocation
or expiry takes effect at request authorization, so a later page may omit an
item that existed when an earlier cursor was issued; it must never return the
now-unauthorized row. The sent collection may expose safe revoked history to
its grantor, but raw capabilities and password material are never list fields.

### Photos and albums

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `POST /photo-imports` | Bind uploaded immutable resource versions to a device import identity/group. | Owner/device; idempotency by source identity plus content/version. |
| `GET /photos` | Timeline/filter/search photo projections with processing freshness. | Source-authorized user; keyset cursor binds timeline sort and filters. |
| `GET/PATCH /photos/{photo_asset_id}` | Read projection or edit user-owned favorite/title/time corrections. | Owner; `If-Match`; never rewrites original bytes/EXIF. |
| `GET /photos/{photo_asset_id}/derivatives/{kind}` | Stream an authorized replaceable rendition. | Source access; fallback/processing state explicit. |
| `POST/GET /albums` | Create/list manual albums. | Owner; creation idempotency and keyset list. |
| `PATCH/DELETE /albums/{album_id}` | Edit/delete album only. | Owner; `If-Match`; no asset cascade deletion. |
| `POST/DELETE /albums/{album_id}/members/{photo_asset_id}` | Idempotently add/remove membership. | Owner; conditional album revision. |

The original uses normal node/version content APIs. Photo API behavior is
defined in [PHOTOS.md](PHOTOS.md).

### Search, tags, and AI

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `GET /search` | Query named layers (`METADATA`, `FULL_TEXT`, `SEMANTIC`, `PHOTO`, `CODE`) and return provenance/freshness. | Authenticated; authorization before results; query-bound cursor. |
| `POST/GET /tags` | Create/list user tags. | Owner; creation idempotency, keyset listing. |
| `PUT/DELETE /subjects/{subject_type}/{subject_id}/tags/{tag_id}` | Idempotently set/remove user assignment. | Subject/tag owner or allowed editor; does not overwrite AI provenance. |
| `GET/PATCH /ai/settings` | Read/change mode, scopes, remote consent, and derived-retention policy. | Owner/admin as scoped; `If-Match`; audited. |
| `POST /ai/reindex-operations` | Reindex a bounded scope/version/config asynchronously. | Authorized source owner; idempotency key. |
| `DELETE /ai/index-records` | Request deletion of authorized derived data by bounded scope. | Owner; idempotency, async operation, audit. |
| `GET /ai/status` | Report mode, configured pipeline generations, safe health and lag. | Authenticated/admin view separated; no indexed content. |

`ai_disabled` is a normal state for AI-only routes, not a core health failure.
Search always permits a deterministic non-AI layer. [AI.md](AI.md) owns
consent, data egress, model provenance, prompt-injection, and stale-index
behavior.

### Git integrations, repositories, and projects

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `POST/GET /integrations/git` | Configure/list safe Forgejo connections. | Owner; create idempotency; credentials write-only and redacted. |
| `GET/PATCH/DELETE /integrations/git/{integration_id}` | Read status, pause/reconfigure, or revoke connection. | Owner; `If-Match`; deletion never deletes Forgejo data. |
| `POST /integrations/git/{integration_id}:test` | Perform bounded SSRF-safe capability/credential test. | Owner; rate-limited, audited; safe provider error mapping. |
| `POST /integrations/git/{integration_id}:refresh` | Enqueue inventory refresh. | Owner; idempotency; async. |
| `GET /repositories` | List accessible cached repositories with freshness. | Owner/current provider authorization policy; keyset pagination. |
| `GET /repositories/{repository_id}` | Read metadata, branch/tag summary, backup health. | Authorized owner; stale state explicit. |
| `POST /repositories/{repository_id}/backups` | Enqueue a repository recovery point. | Owner; idempotency; async. |
| `GET /repositories/{repository_id}/backups` | List verified/incomplete backup records. | Owner; keyset pagination. |
| `POST /repository-backups/{backup_id}:restore` | Restore to an explicit safe Forgejo destination. | Owner, fresh provider authorization, step-up/confirmation, idempotency. |
| `POST/GET /projects` | Create/list optional workspaces. | Authenticated owner; idempotency/keyset rules. |
| `GET/PATCH/DELETE /projects/{project_id}` | Read/edit/delete project metadata only. | Owner; `If-Match`; links/targets do not cascade. |
| `PUT/DELETE /projects/{project_id}/links/{link_id}` | Add/remove an authorized typed link. | Must be able to access both project and target; idempotent, conditional. |

Forgejo remains the Git protocol and permission authority.
[CODE_INTEGRATION.md](CODE_INTEGRATION.md) owns provider identity, backup
consistency, restore verification, webhook validation, and stale-state rules.

### Activity, administration, and health

| Method and path | Responsibility | Access and retry |
|---|---|---|
| `GET /activity` | List caller-authorized product activity projections. | Authenticated; keyset pagination; redacted. |
| `GET /admin/audit-events` | Query append-only security events. | Instance admin/auditor role; bounded filters; no secrets/content. |
| `GET /admin/storage/backends` | Read configured backend status and capacity evidence. | Instance administrator; secrets omitted. |
| `POST /admin/storage/verification-operations` | Enqueue bounded integrity verification. | Instance administrator; idempotency; async/audited. |
| `GET /system/health` | Aggregated authenticated operational health and freshness. | Administrator/monitor credential; safe details. |
| `GET /health/live` | Absolute unversioned process-liveness probe. | Deployment probe; no dependency/topology/version leak. |
| `GET /health/ready` | Absolute unversioned cached readiness probe for required configuration, DB, and selected object backend. | Deployment probe; minimal result. |
| `GET /operations/{operation_id}` | Read authorized async state/results. | Operation owner/admin; ETag and optional wait. |
| `POST /operations/{operation_id}:cancel` | Request safe cancellation. | Operation owner/admin; `If-Match`; idempotent intent. |

Public health endpoints do not disclose database hostnames, mount paths,
bucket names, free-space totals, provider URLs, worker stack traces, or secret
configuration.

## Transaction and side-effect map

| API outcome | PostgreSQL atomic boundary | Object/external boundary | Recovery |
|---|---|---|---|
| Upload complete | session terminal outcome + canonical `Object` identity + verified `ObjectReplica` backend/key/representation binding + version + node head + quota + change + audit + outbox | Object bytes are finalized and verified before the transaction | Unreferenced finalized bytes are reconciled after lease/grace; replay returns the stored outcome. |
| Rename/move | node revision/ancestry/name + change + audit + outbox + idempotency | None synchronous | Rollback exposes no change; stale precondition conflicts. |
| Trash/restore | node/trash state + change + audit + outbox + outcome | Physical deletion never synchronous | GC later proves all retention references absent. |
| Share create/revoke | grant/revision + audit + outbox + outcome | Active streams/already delivered bytes are outside rollback | Every new request reauthorizes; disclose recall limits. |
| Backup snapshot commit | verified manifest + references + accounting + state + audit + outbox | All required objects already verified | Incomplete snapshot stays non-restorable; retry same key. |
| Photo import binding | asset/resources + revision + change if node mutation + outbox | Extraction/derivative generation asynchronous | Processing can retry or stay failed; original remains accessible. |
| AI settings change | policy revision + audit + scoped invalidation/outbox | Remote queued/in-flight cancellation is best effort | Block new egress immediately; expose deletion/drain state. |
| Repository backup commit | verified component manifest + refs + consistency class + audit/outbox | Forgejo capture occurred before; optional service | Incomplete capture is labeled; orphan bytes reconciled safely. |

PostgreSQL failure means the API cannot claim a metadata mutation succeeded.
Object-store failure means a new content version cannot commit. An optional
external provider failure affects only its operation and freshness.

## Caching and consistency headers

- Mutable metadata uses `Cache-Control: private, no-cache` so a client may
  revalidate with ETag without serving it to another user.
- Secret-bearing, authentication, share-resolution, recovery, and sensitive
  settings responses use `Cache-Control: no-store`.
- Immutable authorized version content may be privately cached by immutable
  ETag; public caching requires a separate share-specific policy and cannot
  outlive revocation promises.
- `Vary` includes every header that changes representation/authorization
  behavior. Bearer secrets never appear in cache keys visible to logs.
- The API does not use replica-lagged database reads for immediate
  authorization, conditional mutation, upload completion, or change cursors.

## API version and compatibility policy

- `v1` identifies the public protocol major version, not the product release.
- Backward-compatible additions include optional response fields, new
  endpoints, and new opt-in enum values only when old clients have defined
  unknown-value behavior.
- Breaking changes include changing field meaning/type, weakening an invariant,
  removing a state/code, changing cursor interpretation, or altering
  authorization/idempotency semantics. They require an ADR and new compatible
  route/media version or a migration strategy.
- Deprecation is documented in OpenAPI and response metadata with an announced
  minimum support window tied to release policy. A self-hosted upgrade never
  assumes every client updates simultaneously.
- Readers deploy before writers when a new stored/event representation may be
  observed by mixed versions.
- A client sends a protocol/capability declaration at device registration;
  unsupported mandatory capability returns a stable upgrade-required response
  rather than approximating destructive behavior.

## OpenAPI blueprint and review gate

The checked-in [`api/openapi.yaml`](../../api/openapi.yaml) is the reviewed
transport authority below the domain/protocol specifications. It contains the
validated foundation health, browser-authentication, bootstrap, and logical
file/folder metadata routes, shared conventions, safe resource DTOs, bounded
pagination, conditional mutation responses, and reusable schemas. Content,
sync, backup, sharing, and device operations remain `PLANNED` until their
corresponding contracts are ready. It should continue to contain:

```text
info and server/profile metadata
tags by domain
securitySchemes
paths and operationId values
components/
  schemas/
    Resource envelopes
    Error and registered error details
    IDs, timestamps, revisions, hashes
    Domain resource representations
    Operation, pagination, freshness
  parameters/
    Cursor, limit, If-Match, Idempotency-Key
  headers/
    ETag, X-Request-Id, Retry-After
  responses/
    Standard errors and conditional outcomes
  securitySchemes/
    BrowserSession, DeviceBearer, PublicShareContext
```

Each operation declares:

- status (`PLANNED` until evidence promotes it), owner, summary, and stable
  `operationId`;
- exact auth scheme and authorization action;
- idempotency and conditional-request requirements;
- request/response limits and streaming behavior;
- all success and error codes including safe details;
- pagination/filter/sort/freshness semantics;
- audit effects and async operation/outbox effects; and
- examples for success, stale precondition, forbidden/not-found concealment,
  duplicate replay, and dependency failure where applicable.

Generated clients come only from a reviewed contract and are never hand-edited.
Contract tests validate examples, error registry, status codes, cursor
round-trips, conditional behavior, and unknown-field/enum compatibility. The
implementation must fail CI if a reachable operation drifts from OpenAPI.

No speculative installer, service-manager, storage-picker, pairing, update,
uninstall, migration, or relay endpoint is being claimed by this blueprint.
When one becomes implementable, its request/response schemas, error registry,
authorization actions, local-IPC boundary (if any), idempotency behavior, and
recovery tests must be reviewed here and in the paired Vietnamese contract
before the operation is promoted.

## Abuse and privacy requirements

- Enforce body, decompressed-body, header, filename, part, manifest, batch,
  page, recursion, search-query, and operation-result limits before expensive
  allocation or parsing.
- Stream and backpressure uploads/downloads. CPU-heavy checksum, compression,
  archive, image, document, or Git parsing runs with bounded concurrency and
  time/memory/disk limits outside the async reactor.
- Never fetch a user/provider URL from a general file or AI endpoint.
  Integration URL fetching passes SSRF validation, redirect revalidation, DNS
  rebinding controls, egress policy, and private-address policy.
- Sanitize `Content-Disposition` and all UI-rendered names; API JSON escaping
  is not the only XSS defense.
- Do not reveal cross-user dedup hits, object existence, filenames, search
  snippets, embeddings, photo locations, repository visibility, quota totals,
  or timing differences.
- Public links and webhook receivers have separate strict rate limits,
  signature/secret handling, replay windows, size bounds, and redacted logs.
- Remote AI requests are impossible unless current policy and explicit consent
  authorize the exact provider and data category; the API exposes what is
  queued, derived, and deletable.

## Failure and recovery requirements

| Scenario | API behavior |
|---|---|
| Client disconnects mid-part | Part is accepted only if the complete declared range and checksum became verified; otherwise state remains retryable/rejected and no version is visible. |
| Disk fills after some staging writes | Terminate safely, preserve committed objects, release/retain only bounded session state, return storage/quota error and observability signal. |
| Completion races with expiry/abort | Row transition serializes one terminal winner; losing request receives the stored state, never a second outcome. |
| DB commit succeeds but response is lost | Same idempotency key or `GET /uploads/{upload_id}` returns exact committed IDs/ETag; no duplicate change. |
| Cursor token is copied to another library/user | Integrity/scope validation fails before query; return concealed `invalid_cursor` without leaking original scope. |
| Recursive operation partly succeeds before worker crash | Durable per-item checkpoint and idempotency resume; terminal result is `PARTIAL` until reconciled. |
| Share revoked during cached/public access | New resolution/range requests fail; responses do not claim recall of bytes already delivered or improperly cached by a client. |
| AI/thumbnail worker is offline | Core commit response remains successful with derived state `PENDING`/`STALE`; core health stays healthy and worker lag is visible separately. |
| Forgejo returns 401/500 or malicious body | Map to safe integration status/error, bound body parsing, never return raw secret/provider trace, and leave Drive available. |
| Object fails checksum during read | Do not return `200` with corrupt bytes; quarantine/abort stream when detected, record integrity incident, seek verified replica/restore path. |

## Bounded open decisions

OPEN DECISION OD-API-001: idempotency outcome retention
Owner: Architecture / API / Database / Sync
Needed by: Phase 1 first mutating OpenAPI gate
Options: fixed seven-day retention; operation-specific retention at least upload/session expiry plus retry window; retain compact outcomes for the resource lifetime
Recommendation: operation-specific retention with a documented minimum of the longest supported offline retry window, and compact long-lived tombstones for keys whose replay could duplicate durable user data
Decision evidence: client retry/offline requirements, database growth benchmark, purge safety proof, and replay contract tests

OPEN DECISION OD-API-002: collection page limits
Owner: API / Web / Client / Performance
Needed by: Phase 1 listing OpenAPI gate
Options: one global default/maximum; operation-specific bounds; server-advertised adaptive maximum
Recommendation: operation-specific fixed defaults and maxima recorded in OpenAPI, with conservative Phase 1 measurements and no adaptive behavior that makes client memory unpredictable
Decision evidence: metadata-row size measurements, web/mobile memory tests, database plans, and abuse-load tests

OPEN DECISION OD-API-003: sync baseline representation
Owner: Sync / API / Database
Needed by: Phase 4 protocol freeze
Options: bounded live ID-order scan plus retained resume cursor; materialized manifest; exported database snapshot held across requests
Recommendation: bounded live ID-order scan plus retained sequence-N resume cursor and revision-idempotent replay; materialize only if adversarial churn tests cannot converge within lease bounds
Decision evidence: property tests under create/move/delete churn, journal-retention load test, stale-baseline recovery test, and mobile interruption test

OPEN DECISION OD-API-004: public share capability transport
Owner: Security / API / Web
Needed by: Phase 3 public sharing gate
Options: URL path token; URL fragment posted into a scoped session; one-time code exchange
Recommendation: URL fragment plus explicit POST exchange into a short-lived share-scoped context so common proxy logs do not receive the high-entropy capability
Decision evidence: browser/XSS/CSRF threat review, accessibility and no-JavaScript product decision, proxy-log test, and revocation test

OPEN DECISION OD-API-005: large collection command threshold
Owner: API / Worker / Performance
Needed by: Phase 2 bulk and recursive operation gate
Options: always asynchronous; synchronous below a fixed item/work estimate; client-selected preference constrained by server
Recommendation: use a server-fixed, operation-specific work bound and return `202` above it; never decide from item count alone when subtree expansion or byte work is unknown
Decision evidence: transaction-lock duration benchmark, worker recovery tests, API timeout limits, and partial-result UX review

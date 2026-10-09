# Prompt029 Manifest — Library Catalog API & Domain Models

## Baseline and Scope

- Starting hosted `origin/ios-app`: `66c89f033f9eeaaf85d44d7cf4d6e5dbe7377410`.
- Working branch: `ios/p029-library-catalog-api`.
- Reused the existing checkout. Fetch succeeded, the worktree was clean, and `git merge-base --is-ancestor 66c89f033f9eeaaf85d44d7cf4d6e5dbe7377410 origin/ios-app` succeeded before switching directly from `origin/ios-app`. The default `work` checkout was not used as the implementation baseline.
- Scope: transient authenticated Library Catalog data foundation. No browser screen, metadata mutations, enrollment changes, persistent library cache, or new credential vault.

## Authoritative References

Inspected `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`, `docs/ios/IOS_V0_1_ROADMAP.md`, `docs/ios/IOS_PLATFORM_MAPPING.md`, P026–P028 manifests, and `api/openapi.yaml` (Library collection/resource/attributes, Page, ResponseMeta, OpaqueId, U64Decimal, and Error schemas). Reviewed the iOS session controller, restoration and authorization services, secure credential protocol and Keychain store, Rust protocol, bounded URLSession transport, dependency container, and root view. Android parity references: `LibraryModels.kt`, `SynveilHttpTransport.kt`, `DeviceSessionManager.kt`, and `LibraryScreen.kt`.

## Domain and Wire Validation

- `LibraryId` and `NodeId` have private construction and async Rust-backed validation. Library IDs and root node IDs must pass `validateLibraryID` / `validateNodeID`, including canonical lowercase UUIDv7 requirements.
- `LibraryRevision` stores exact canonical unsigned decimal text. It accepts only `0` or a nonzero digit followed by digits. No fixed-width numeric or floating-point conversion occurs, so values beyond UInt64 remain exact, matching Android's text semantics.
- `LibraryStatus`: `ACTIVE`, `READ_ONLY`, `QUARANTINED`; unknown values fail decoding.
- `Library` contains validated ID, revision, unchanged logical name, root node ID, typed status, and Foundation `Date` timestamps. Rust `validateLogicalName` enforces nonempty names and the 1024-byte UTF-8 bound without filesystem interpretation.
- Separate Codable DTOs represent `data`, `page`, `meta`, library resources, and attributes. Shape checks enforce every relevant `additionalProperties: false` rule before Codable decoding; required fields, types, `type=library`, metadata request IDs, and pagination relationships are validated.
- Timestamps require RFC3339-compatible complete date/time plus explicit offset or Z. Strict Foundation civil-date checks reject calendar normalization (such as February 30); ISO8601DateFormatter handles offsets and fractional seconds.
- Request IDs follow `^[A-Za-z0-9._~-]{8,128}$`. Invalid JSON, missing fields, unknown keys/statuses, invalid identifiers/names/revisions/dates, and duplicates produce deterministic typed failures.
- OpenAPI Page permits an optional string cursor, not a null value: present null is rejected. Completion requires cursor absence; continuing pages require a nonempty bounded cursor.

## Authenticated Session Boundary

- Internal `AuthenticatedLibraryRequestProviderProtocol` creates a transient redacted scope containing the validated Keychain session and captured `SessionController.lifecycleRevision`.
- Production calls the existing `SecureCredentialSinkProtocol.load(expectedServerEndpoint:)` on the same `KeychainCredentialStore` used by enrollment/restoration/logout. That store validates the credential through Rust; the provider also fails closed on bearer syntax, HTTPS policy, and exact canonical endpoint mismatch.
- No second persistent credential cache or global manager exists. UI receives only `LibraryCatalogRepositoryProtocol`; the provider and its credential-bearing scope are internal and absent from observable root state.
- Each request checks the authenticated lifecycle, loads and compares the current Keychain identity, and builds its URL from the validated endpoint. Scope checks after HTTP and after async Rust mapping fence logout, recovery transitions, origin changes, and credential replacement.
- No operation begins outside `.authenticated`, during logout, or after cleanup. A stale result cannot return a loaded catalog. Logout need not wait for the catalog's HTTP response; the session owner remains authoritative.
- Documented authentication rejection/device revocation are routed through the controller's revision-guarded existing recovery mechanism. Repository code never assigns root state. Generic network/server/protocol failures do not delete credentials, revoke authentication, or enroll again.
- Both request scope and HTTP request debug/ordinary descriptions are redacted. Failure values carry no response bodies, server messages, bearer strings, or raw headers.

## Transport and Pagination

- `GET /api/v1/libraries?limit=100`, then opaque `cursor` query items via Foundation URLComponents. Literal plus is percent-encoded to preserve it under form-style server query decoding.
- Headers: DeviceBearer Authorization, JSON Accept, identity Accept-Encoding, and established `Synveil/0.1.0 (iOS)` User-Agent. No browser cookie, CSRF header, enrollment token, body, mutation, or retry.
- Existing `URLSessionHTTPTransport`, 10-second request / 15-second resource timeouts, **1 MiB** maximum response body for both success and error responses. Repository checks the same bound for injected transports.
- Preserves ephemeral cookie-free, cache-free configuration, trusted Apple TLS, redirect rejection, and HTTPS-only catalog policy; no cleartext exceptions or cross-origin forwarding are introduced.
- Complete retrieval permits **100 libraries/page**, **64 pages**, **4096 libraries total**, and **512 Unicode scalar cursor characters** (JSON Schema maxLength). Cursors remain unchanged and scoped to one listing.
- Loop detection compares cursor UTF-8 bytes, preserving opacity even for canonically equivalent Unicode strings. Duplicate library IDs are rejected within and across pages.
- Page/total limits are checked before domain accumulation; reaching the page bound with more data returns a resource-limit failure. No partial result is reported complete. Empty `loaded([])` is successful.
- Cancellation is typed, checked before acquisition, between pages, during mapping, and before completion. Structured cancellation reaches the transport; there are no retries or state advancement on cancellation.

## Failure Classifications

Authentication unavailable, missing/unreadable credentials, invalid credential, origin mismatch, stale session, authentication rejected, device revoked, offline, DNS, timeout, TLS, redirect, HTTP 503/server unavailable, other HTTP status, wrong content type, protocol failure, resource limit, repeated cursor, and cancellation remain distinct typed cases. Only valid documented 401 errors trigger authentication recovery. Error DTO validation rejects malformed error envelopes before classification, and server text is never propagated into errors.

## Implementation Files

- `clients/ios/Domain/Library/LibraryModels.swift`
- `clients/ios/Domain/Library/LibraryResponseDTO.swift`
- `clients/ios/Application/Library/AuthenticatedLibraryRequestProvider.swift`
- `clients/ios/Application/Library/AuthenticatedLibraryCatalogRepository.swift`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/App/AppDependencyContainer.swift`
- `clients/ios/Domain/Services/Transport/HTTPTransportRequest.swift`
- `clients/ios/Tests/SynveilTests/LibraryCatalogTests.swift`
- `clients/ios/Support/tests/test_library_catalog_registration.py`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`
- This manifest.

## Validation and Limitations

- **78 new deterministic catalog tests** cover canonical and malformed models, real Rust validation, strict JSON/schema decoding, UTF-8 logical names, timestamp offsets/calendar validity, pagination/query encoding/duplicates/limits, empty catalogs, headers and origin policy, typed failures, credential retention, cancellation, Keychain-load/logout races, cleanup-in-progress gating, credential replacement, and stale responses.
- Linux iOS source/architecture validator: PASS.
- Python support tests including PBX structural and correct-target registration: **24 passed**.
- Documentation validation and `git diff --check`: PASS at local implementation review.
- Swift 6.1.2 strict recursive formatting and strict-concurrency production/test typecheck: PASS. Swift 6.2.3 strict formatting and parsing of every iOS Swift source also passed. Existing RustBridgeAdapter emits a pre-existing always-true Int-bound warning on Linux.
- Rust `cargo test -p synveil-ios-ffi`: **16 passed, 0 failed**.
- Linux Swift 6.2.3 XCTest harness linked the real Rust FFI and passed **158 tests, 0 failures**, including all 78 catalog tests, 43 session restoration tests, session lifecycle/transport contracts, and Rust bridge suites. Production Foundation/Observation sources were unchanged; temporary test copies converted synchronous XCTest declarations to async solely for Linux actor-aware test discovery. Apple UI, native URLSession construction, and Security.framework suites are excluded here. Initial Swift 6.1.2 runtime linking was blocked by its bundled libswiftObservation undefined symbol; the successful 6.2.3 run supersedes that attempt. This is not an Apple Simulator result.
- Hosted iOS Static Validation / Build / Simulator tests on final feature head: PENDING.
- Apple Rust build: no Rust/FFI changes; normal iOS Build/Simulator workflows rebuild Rust static libraries. Explicit final-head Apple workflow evidence: PENDING.
- Real Keychain round-trip: retains the established unsigned Simulator missing-entitlement skip. Deterministic injected Keychain tests remain required; the real round-trip is not claimed passing.
- Physical-device validation: `NOT_AVAILABLE`.

## Hosted Delivery

- Final feature commit SHA: PENDING (recorded after the immutable commit exists).
- PR URL / number: PENDING; target must be `ios-app`.
- Final-head hosted CI: PENDING.
- Merge status: NOT_MERGED.
- Resulting hosted `ios-app` SHA: PENDING.
- Unrelated CI failures: no final-head evidence yet.
- Prompt030 readiness: pending genuine hosted merge and final-head iOS CI.

Post-merge immutable commit, CI, PR, and resulting integration evidence will be finalized in a documentation-only follow-up, as in P028; this initial record does not claim future evidence.

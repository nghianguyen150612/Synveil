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
- Hosted final-head iOS validation: all four required workflows PASS on `4693cb3652e1fa5b351e6c4d9a01222f4a246b26`; links and test totals below.
- Apple Rust build: PASS via explicit workflow dispatch on the final feature head, including device ARM64 and Simulator ARM64/x86_64 static artifacts and approved-symbol verification. No Rust/FFI source changed.
- Real Keychain round-trip: `KeychainCredentialStoreTests.testSimulatorKeychainRoundTripUsesUniqueTestService` was SKIPPED. Hosted log: “The unsigned Simulator test process has no Keychain access entitlement.” Deterministic injected Keychain tests passed; the real round-trip is not claimed passing.
- Physical-device validation: `NOT_AVAILABLE`.

## Hosted Delivery

- Final feature commit SHA: `4693cb3652e1fa5b351e6c4d9a01222f4a246b26` (`feat(ios): add authenticated library catalog API`).
- Feature PR: [#86](https://github.com/nghianguyen150612/Synveil/pull/86), target `ios-app`. Opened as draft, marked ready after all required final-head iOS workflows passed, and squash-merged.
- iOS Static Validation: [run 37874583754](https://github.com/nghianguyen150612/Synveil/actions/runs/37874583754), PASS on the exact feature head. Apple Swift 6.3.3 strict formatting, all 24 support checks, and source/architecture validation passed. Duplicate PR run 37874607604 also passed.
- iOS Build: [run 37874583771](https://github.com/nghianguyen150612/Synveil/actions/runs/37874583771), PASS on the same head, `BUILD SUCCEEDED`. Duplicate PR run 37874607717 also passed.
- iOS Simulator Tests: [run 37874607621](https://github.com/nghianguyen150612/Synveil/actions/runs/37874607621), PASS on the same head, `TEST SUCCEEDED`. Native result bundle: **334 total, 333 passed, 1 skipped, 0 failed**. All **78 new catalog tests passed**; existing enrollment, strict token, Keychain injection, restoration, verified authorization, logout, stale transitions, recovery, origin, and transport regressions remained green. The skipped test is the real Keychain round-trip described above.
- iOS Rust Apple Build: [run 37874607060](https://github.com/nghianguyen150612/Synveil/actions/runs/37874607060), PASS on the same head. Explicitly dispatched even though no Rust/core/FFI paths changed.
- Hosted feature merge: GitHub REST confirmed **`merged=true`, `state=closed`**, merged at **2026-10-09 02:38:15 UTC**.
- P029 feature merge / resulting hosted `ios-app` SHA: **`ac453f3bf5523b872569a058c2e0fb623b6246b3`**, verified from the hosted PR and a fresh integration-branch fetch. Catalog models and this manifest were verified to exist in that hosted tree.
- Manifest finalization: documentation-only follow-up branch `ios/p029-manifest-finalization` starts directly at that verified integration SHA. It records already-observed evidence and changes no tested iOS source. Its own commit/merge cannot be embedded self-referentially; the hosted follow-up PR record provides that separate documentation integration metadata.
- Physical-device status: `NOT_AVAILABLE`.
- Prompt030 readiness: **`READY_FOR_PROMPT030`**, based on genuine hosted P029 feature merge and passing exact-head iOS workflows.

## Unrelated CI Evidence

- [Rust CI run 37874583751](https://github.com/nghianguyen150612/Synveil/actions/runs/37874583751) and duplicate PR run 37874607789 failed in unchanged non-iOS code. Failures include `synveil-object-store` Clippy `double_must_use` errors, Windows `synveil-install-engine` Unix-only API compilation errors, Linux metadata install lifecycle tests, and macOS client control/launch tests with `UnsafeEndpoint`. No Rust source was changed by P029; these failures were not broadened into this feature.
- [Linux AppImage run 37874607568](https://github.com/nghianguyen150612/Synveil/actions/runs/37874607568) and push run 37874583763 failed independently reproducing the desktop AppImage: `[synveil-artifact] ERROR: private or temporary build path found in synveil-desktop`. No desktop packaging source was changed.
- Other Linux native package and PostgreSQL scheduled-maintenance workflows are reported only according to their observed hosted state; they are not prerequisites for the iOS feature and are not claimed passing without evidence.
- The duplicate push Simulator run was still in progress at feature merge inspection; the completed successful exact-head PR Simulator run above provides the native test evidence.

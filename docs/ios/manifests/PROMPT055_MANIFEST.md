# P055 — Verified File Materialization, Offline Content Access & Files-Originated Operation Safety

## Implementation Summary

- **Verified P054 merge baseline:** `b0a977a15878f04dbe5c351f352996de68f86211`
- **Feature branch:** `ios/p055-file-provider-materialization`
- **Status:** `COMPLETE_WITH_APPROVED_DEFERRALS` for isolated, verified offline file content materialization.
- **Platform:** Swift, FileProvider.framework, Foundation, CryptoKit, App Groups, iOS 17+

---

## 1. Architecture & Security Boundaries

P055 implements the safe, three-boundary content pipeline:
1. **A. Content verified by Synveil host app:** P043 verified download storage (`FileDownloadCoordinator`).
2. **B. Content published into App Group:** Host-mediated `FileProviderVerifiedContentPublicationCoordinator` copies P043-verified files into protected App Group storage under `Synveil/FileProvider/v1/content/<domain>/` with a signed/atomic `FileProviderContentManifest`.
3. **C. Content materialized by Apple File Provider:** Extension `SynveilFileProviderExtension.fetchContents` reads published App Group content, verifies streaming SHA-256 and byte length, and returns a temporary file copy on the `NSFileProviderManager.temporaryDirectoryURL()` volume.

### Key Security & Integrity Invariants
- **No Direct Extension Network Access:** The File Provider extension does not open network connections, perform authentication, or access Keychain secrets.
- **Exact FileVersion Matching:** Explicit requested `NSFileProviderItemVersion` is compared against immutable `FileVersionIdentity` SHA-256 hashes. No newer or older revision is substituted.
- **Streaming Digest & Size Verification:** `FileProviderContentStore` computes SHA-256 in 64 KiB chunks using `CryptoKit.SHA256` and enforces exact byte length match before returning temporary files to Apple's framework.
- **Session & Authorization Fencing:** Active domain lease, authorization state (`.authorized`), and generation epochs are checked both before staging and at completion. Logout or credential revocation invalidates materialization eligibility.
- **Storage Quota Enforcement:** Bounded 64 MiB total storage quota enforced for App Group content storage, independent of metadata quota.
- **Fail-Closed Mutations:** `createItem`, `modifyItem`, and `deleteItem` remain fail-closed and return `cannotSynchronize` / `unsupported` errors.
- **Item Capabilities:** `SynveilFileProviderItem.capabilities` advertises `.allowsReading` for files with valid version identities, while write, rename, reparent, and delete capabilities remain strictly disabled.
- **Target Membership Isolation:** `SynveilFileProviderExtension.swift` is assigned strictly to `SynveilFileProviderExtension.appex` to isolate extension dependencies from the main app, while reusable materialization adapters (`FileProviderContentStore`, `FileProviderContentManifest`, `FileProviderVerifiedContentPublicationCoordinator`, `SynveilFileProviderItem`, `FileProviderErrorMapper`) are compiled in both host app and `SynveilTests` targets.

---

## 2. Source & Test Inventory

### Production Source Files
- `clients/ios/Extensions/FileProvider/FileProviderContentManifest.swift`: Codable, versioned content manifest (schema version 1, expected length, expected SHA-256, authorization generation, storage key).
- `clients/ios/Extensions/FileProvider/FileProviderContentStore.swift`: Versioned App Group content storage, 64MB storage quota check, streaming SHA-256 verification in 64 KiB chunks, crash-safe atomic manifest updates.
- `clients/ios/Application/FileProvider/FileProviderVerifiedContentSource.swift`: Protocol bridging host P043 verified download storage with File Provider content publication.
- `clients/ios/Application/FileProvider/FileProviderVerifiedContentPublicationCoordinator.swift`: Host publication coordinator inspecting P043 verified download evidence, verifying domain authorization lease, and staging files to App Group content store.
- `clients/ios/Extensions/FileProvider/SynveilFileProviderExtension.swift`: Implemented native `fetchContents` callback with lifecycle, domain lease, FileVersion matching, content store access, same-volume temporary file copy, and `Progress` cancellation handling.
- `clients/ios/Extensions/FileProvider/SynveilFileProviderItem.swift`: Updated item capabilities to expose `.allowsReading` for valid file versions and `isDownloaded` check via App Group manifest store.
- `clients/ios/Extensions/FileProvider/FileProviderErrorMapper.swift`: Added error mapping for `FileProviderContentStoreError` cases to native `NSFileProviderError` codes.

### Native & Portable Tests
- `clients/ios/Tests/SynveilTests/FileProviderMaterializationTests.swift`: Comprehensive XCTest suite covering Groups A through J:
  - **Group A:** Verified-content eligibility (`testVerifiedContentEligibility`).
  - **Group B:** Integrity, byte length, streaming SHA-256 chunk validation, and mismatch detection (`testContentIntegrityAndMismatch`, `testStreamingSHA256VerificationSuccessAndFailure`).
  - **Group C:** Requested version contract semantics (`testRequestedVersionContractSemantics`).
  - **Group D:** Temporary directory volume attributes & handoff (`testTemporaryDirectoryAttributesAndHandoff`).
  - **Group E:** Cancellation handling via `Progress` (`testCancellationHandling`).
  - **Group F:** Logout fencing and authorization revocation (`testLogoutFencing`).
  - **Group G:** Concurrent content reads (`testConcurrentContentReads`).
  - **Group H:** Storage quota enforcement (`testStorageQuotaAndEviction`).
  - **Group I:** Item capabilities & downloaded status (`testItemCapabilitiesAndDownloadedStatus`).
  - **Group J:** Fail-closed error mapping restrictions (`testErrorMappingFailClosed`).
- `clients/ios/Support/tests/test_file_provider_materialization.py`: Python unit tests asserting source membership and extension isolation.
- `python3 -m unittest discover -s clients/ios/Support/tests`: **122 passed, 0 failed**.
- `python3 clients/ios/Support/validate_ios_sources.py`: **PASS**.

---

## 3. Explicit Deferred Scope

Status: `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`

As authorized by the product specification:
- Signed-device File Provider acceptance on physical hardware.
- App Group entitlement activation on physical hardware.
- Production domain registration.
- Actual Files app end-to-end user materialization on physical device.
- Remote on-demand fetching from an unverified deployment.
- Files-originated remote mutations / uploads.
- P046 / P051 background-transfer reactivation.

All core materialization infrastructure, exact FileVersion checking, streaming SHA-256 integrity verification, App Group storage correctness, cancellation, session fencing, target membership isolation, and unit tests are fully implemented and verified.

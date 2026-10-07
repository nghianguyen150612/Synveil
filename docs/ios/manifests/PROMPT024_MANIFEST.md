# Prompt024 Manifest — Authentication API Integration

## Overview
- **Prompt Number**: Prompt024
- **Title**: Authentication API Integration
- **Goal**: Implement native iOS device enrollment / authentication API integration (`POST /api/v1/device-enrollment/exchange`) following server readiness confirmation.
- **Starting `ios-app` SHA**: `7f8ee238a26c2a7b44fad8a82e11dd6f98f1fce4`
- **Working Branch**: `ios/p024-auth-api-integration`

---

## Authoritative References Inspected
- `docs/ios/IOS_V0_1_PRODUCT_CONTRACT.md`
- `docs/ios/IOS_PLATFORM_MAPPING.md`
- `docs/ios/manifests/PROMPT023_MANIFEST.md`
- `api/openapi.yaml`
- `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentModels.kt`
- `clients/android/app/src/main/java/com/synveil/android/data/enrollment/EnrollmentManager.kt`
- `clients/ios/Application/Services/RustBridgeProtocol.swift`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Support/validate_ios_sources.py`

---

## Architecture & Design Summary

### Enrollment Token & Credential Validation Strategy
- `EnrollmentToken`: Encapsulates `sve1_` + 64 lowercase hex characters (69 ASCII chars). `description` and `debugDescription` output is explicitly redacted as `[REDACTED_ENROLLMENT_TOKEN]`.
- `DeviceCredential`: Encapsulates `svd1_` + 64 lowercase hex characters (69 ASCII chars). `description` and `debugDescription` output is explicitly redacted as `[REDACTED_DEVICE_CREDENTIAL]`.
- Rust Shared-Core Integration: Shared core token validation (`RustBridgeProtocol.validateEnrollmentToken(_:)` and `validateDeviceBearerToken(_:)`) is reused without importing raw FFI modules into UI or Application layers.

### Exchange Request Semantics & Single-Shot Invariants
- Endpoint: `POST /api/v1/device-enrollment/exchange`
- Request Content-Type: `application/json`
- Request Body: `{ "enrollment_token": "<sve1_...>" }`
- Token Privacy: Token appears strictly in the HTTP request body; never in URLs, query parameters, logs, or persistent storage (`UserDefaults`).
- Single-Shot / Non-Retry: Automatic retries are strictly prohibited. Timeout, disconnect, HTTP 503, redirect, or response loss outcomes map to an ambiguous/recovery-required state (`EnrollmentRecoveryReason`), requiring owner recovery rather than automated grant replay.

### Response Bounding & Parsing
- Response Body Bound: Max **16 KiB** (`16 * 1024` bytes). Oversized responses fail deterministically (`.oversizedResponse`).
- Content-Type: Must contain `application/json`.
- Redirects: Prohibited and rejected.
- Expected Response: HTTP 201 Created with JSON envelope containing `data` (`owner_user_id`, `device_id`, `credential_id`, `device_credential`, `created_at`) and `meta` (`request_id`).
- Record Validation: `DeviceCredentialRecord` strictly validates returned opaque IDs (UUID format) and ISO 8601 timestamps.

### Prompt024 ↔ Prompt025 Security Boundary
- Prompt025 owns durable Keychain credential persistence.
- Prompt024 introduces `SecureCredentialSinkProtocol` (`preflight()`, `store(...)`).
- Before sending the one-time network request, `preflight()` verifies secure storage readiness so single-shot grants are never consumed without persistence availability.
- Successful exchange alone does NOT transition `SessionController` state to `.authenticated`.

---

## Files Added / Modified

### Added Files
- `clients/ios/Domain/Services/Enrollment/EnrollmentToken.swift`
- `clients/ios/Domain/Services/Enrollment/DeviceCredential.swift`
- `clients/ios/Domain/Services/Enrollment/DeviceCredentialRecord.swift`
- `clients/ios/Domain/Services/Enrollment/EnrollmentExchangeResult.swift`
- `clients/ios/Domain/Services/Enrollment/EnrollmentState.swift`
- `clients/ios/Domain/Services/Enrollment/ExchangeDeviceEnrollmentRequest.swift`
- `clients/ios/Domain/Services/Enrollment/DeviceCredentialResponseDTO.swift`
- `clients/ios/Application/Services/SecureCredentialSinkProtocol.swift`
- `clients/ios/Domain/Services/Enrollment/EnrollmentExchangeServiceProtocol.swift`
- `clients/ios/Domain/Services/Enrollment/EnrollmentExchangeService.swift`
- `clients/ios/Features/Onboarding/EnrollmentViewModel.swift`
- `clients/ios/Features/Onboarding/EnrollmentView.swift`
- `clients/ios/Tests/SynveilTests/EnrollmentTests.swift`
- `docs/ios/manifests/PROMPT024_MANIFEST.md`

### Modified Files
- `clients/ios/App/RootView.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

---

## Testing & Local Validation Summary

### Unit Tests (`EnrollmentTests.swift`)
30 comprehensive test cases added covering:
1. Empty token rejected locally.
2. Malformed prefix (`bad1_...`) rejected.
3. Uppercase hex rejected.
4. Wrong length rejected.
5. Canonical `sve1_` token accepted.
6. Invalid local token performs zero network requests.
7. Correct endpoint path `/api/v1/device-enrollment/exchange`.
8. POST method used.
9. JSON body shape contains exactly `enrollment_token`.
10. Token is omitted from URL and query parameters.
11. Redirects are rejected.
12. Timeout avoids automatic retry.
13. Disconnect avoids automatic retry.
14. 16 KiB response body bound enforced.
15. Wrong Content-Type (`text/html`) rejected.
16. Malformed JSON rejected.
17. Valid HTTP 201 response parsed successfully.
18. Malformed `svd1_` credential rejected.
19. Malformed opaque IDs rejected.
20. Invalid timestamp rejected.
21. `invalid_enrollment` error code mapped to rejected state.
22. HTTP 503 mapped to recovery-required state.
23. Response loss mapped to recovery-required state.
24. Cancellation cannot authenticate session.
25. Successful exchange alone does not transition state to `.authenticated`.
26. Model descriptions redacted (`[REDACTED_ENROLLMENT_TOKEN]`, `[REDACTED_DEVICE_CREDENTIAL]`).
27. Duplicate submission prevented in ViewModel.
28. Prompt023 server validation state transitions preserved.
29. Preflight check prevents consuming grant when secure storage is unavailable.
30. Retry does not replay the same single-shot exchange.

### Local Validations Passed
- `python3 clients/ios/Support/validate_ios_sources.py` -> [SUCCESS]
- `python3 /home/jules/self_created_tools/verify_pbxproj.py` -> [SUCCESS]
- `python3 -m unittest discover clients/ios/Support/tests` -> [OK]
- `cargo test -p synveil-core -p synveil-ios-ffi` -> [OK]

---

## Explicitly Deferred Work (Prompt025+)
- Keychain credential persistence (Prompt025)
- DeviceBearer session restoration (Prompt026)
- Logout and credential deletion (Prompt027)
- Authentication error UX hardening (Prompt028)

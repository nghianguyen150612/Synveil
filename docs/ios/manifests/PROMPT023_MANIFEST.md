# Prompt023 Manifest — Server Reachability Probe

## Overview
- **Prompt Number**: Prompt023
- **Title**: Server Reachability Probe
- **Goal**: Implement the first real pre-authentication network boundary for Synveil iOS to probe server reachability and readiness (`GET /health/live` followed by `GET /health/ready`) before advancing session state from `.readyForServerValidation` to `.needsEnrollment`.
- **Baseline SHA**: `9e9ebd5c351793b1b924083758f2112f34603807` (`origin/ios-app`)
- **Working Branch**: `ios/p023-server-reachability`

---

## Authoritative References Inspected
- `clients/android/app/src/main/java/com/synveil/android/data/network/SynveilHttpTransport.kt`
- `clients/ios/Domain/Configuration/ServerEndpoint.swift`
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/Support/validate_ios_sources.py`
- `docs/ios/IOS_ARCHITECTURE.md`

---

## Files Added / Modified

### Added Files
- `clients/ios/Domain/Services/Transport/SynveilTransportError.swift`
- `clients/ios/Domain/Services/Transport/ServerProbeResult.swift`
- `clients/ios/Domain/Services/Transport/ServerValidationService.swift`
- `clients/ios/Infrastructure/Network/URLSessionHTTPTransport.swift`
- `clients/ios/Features/Onboarding/ServerValidationViewModel.swift`
- `clients/ios/Features/Onboarding/ServerValidationView.swift`
- `clients/ios/Tests/SynveilTests/ServerReachabilityTests.swift`
- `docs/ios/manifests/PROMPT023_MANIFEST.md`

### Modified Files
- `clients/ios/App/RootView.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

---

## Architecture & Transport Summary

### Transport Architecture & Policies
1. **HTTP Client**: Implemented `URLSessionHTTPTransport` conforming to `HTTPTransportProtocol` using ephemeral `URLSessionConfiguration`.
2. **Timeout Policy**: 10.0s request timeout, 15.0s resource timeout.
3. **Redirect Policy**: Redirects strictly rejected via `RedirectRejectingDelegate` (`willPerformHTTPRedirection` returning `nil`).
4. **Response Size Bound**: Bounded to 64 KiB maximum (`maxProbeResponseBodyBytes = 64 * 1024`).
5. **Content-Type Policy**: Strictly enforces `application/json` (including parameters like `application/json; charset=utf-8`).
6. **TLS & HTTP Policy**: Standard trusted Apple TLS validation for production HTTPS. Cleartext HTTP allowed only for numeric loopback (`127.0.0.1`, `[::1]`) when explicitly permitted for testing.

### Health Endpoint Probe Sequence
1. Construct URL for `GET /health/live`.
2. Verify HTTP 200, JSON Content-Type, bounded body <= 64 KiB, `status == "live"`.
3. If `/health/live` succeeds, construct URL for `GET /health/ready`.
4. Verify HTTP 200, JSON Content-Type, bounded body <= 64 KiB, `status == "ready"`.
5. If `/health/ready` returns HTTP 503, classify as `aliveButNotReady(requestId:code:)`.

### Typed Failure Model
`SynveilTransportError` meaningfully distinguishes:
- `offline`
- `dnsFailure`
- `timeout`
- `tlsError`
- `redirectRejected(statusCode: Int)`
- `httpError(statusCode: Int, code: String?, requestId: String?)`
- `bodyLimitExceeded`
- `unexpectedContentType(contentType: String?)`
- `malformedResponse`
- `protocolError(ProtocolErrorKind)`
- `configurationError`
- `cancelled`

### UI & Session State Machine
- `ServerValidationView` replaces placeholder UI with native SwiftUI status, checking indicator, error messages, and retry action.
- Only confirmed readiness advances `SessionController` to `.needsEnrollment` via `requireEnrollment()`.
- No authentication (`markAuthenticated()`) is implemented in Prompt023.

---

## Testing & Validation Summary

### Unit Tests Added (`ServerReachabilityTests.swift`)
20 test cases covering:
1. `/health/live` valid 200 `{"status":"live"}` succeeds.
2. Liveness failure stops readiness request.
3. Readiness called only after liveness succeeds.
4. `/health/ready` valid 200 `{"status":"ready"}` produces ready success.
5. HTTP 503 readiness maps to `aliveButNotReady`.
6. DNS failure classification.
7. Timeout classification.
8. Offline/connection failure classification.
9. TLS failure classification.
10. Redirects rejected.
11. Unexpected Content-Type rejected.
12. Body > 64 KiB rejected.
13. Malformed JSON rejected.
14. Incorrect health status string rejected.
15. Generic HTTP error handling.
16. Task cancellation safety.
17. Retry starts fresh `live -> ready` probe sequence.
18. Valid readiness transitions `.readyForServerValidation` -> `.needsEnrollment`.
19. Failed check remains pre-enrollment.
20. Validation never produces `.authenticated`.

### Linux Local Validation
- `python3 clients/ios/Support/validate_ios_sources.py` -> [SUCCESS]
- `python3 /home/jules/self_created_tools/pbxproj_verifier.py` -> [SUCCESS]
- `python3 -m unittest discover clients/ios/Support/tests` -> [OK]

---

## Explicitly Deferred Work
- Device enrollment token entry (`sve1_`)
- Post-enrollment exchange POST
- Device credential generation (`svd1_`)
- Keychain secure storage
- DeviceBearer headers
- Persistent profile storage
- Authentication shell

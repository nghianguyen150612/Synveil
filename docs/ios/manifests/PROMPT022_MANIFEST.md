# Synveil iOS — Prompt022 Manifest

## 1. Prompt Number
Prompt022: Server Setup Screen

## 2. Goal
Implement the native SwiftUI Server Setup Screen for server address entry, local address validation using `ServerEndpoint`, user-friendly error mapping, and root application startup state integration (`.needsServerProfile` -> `.readyForServerValidation`).

## 3. Starting Integration Branch
`ios-app`

## 4. Exact Starting SHA
`9198f3c7ef23df94ccb195bfd3d526868840a157`

## 5. Working Branch
`ios/p022-server-setup`

## 6. Files Added
- `clients/ios/Features/Onboarding/ServerSetupViewModel.swift`
- `clients/ios/Features/Onboarding/ServerSetupView.swift`
- `clients/ios/Tests/SynveilTests/ServerSetupViewModelTests.swift`
- `docs/ios/manifests/PROMPT022_MANIFEST.md`

## 7. Files Modified
- `clients/ios/Application/Session/SessionController.swift`
- `clients/ios/App/RootView.swift`
- `clients/ios/Tests/SynveilTests/SessionControllerTests.swift`
- `clients/ios/Synveil.xcodeproj/project.pbxproj`

## 8. Architecture Decisions
- **ViewModel Separation**: `@Observable` `@MainActor` `ServerSetupViewModel` in `Features/Onboarding/` manages UI input state, trims whitespace, validates input via `ServerEndpoint(validating:)`, maps `EndpointValidationError` cases to actionable user messages, and triggers state transitions.
- **Native SwiftUI View**: `ServerSetupView` provides a clean platform-native experience with `.keyboardType(.URL)`, `.autocorrectionDisabled(true)`, `.textInputAutocapitalization(.never)`, `.submitLabel(.continue)`, inline error feedback banner, and full VoiceOver accessibility support (`synveil.server-setup.screen`, `synveil.server-setup.input`, `synveil.server-setup.continue-button`, `synveil.server-setup.error-message`, `synveil.server-setup.title`).
- **Session State Integration**: `SessionController.configureServerEndpoint(_:)` sets `serverEndpoint` and transitions state from `.needsServerProfile` to `.readyForServerValidation`. Accepting a valid server address does NOT mark the user as authenticated.

## 9. UX Behavior Implemented
- Clear title, purpose text, and server URL text field with placeholder (`https://synveil.example.com`).
- Empty and whitespace-only inputs are rejected with "Please enter a server address."
- Invalid URL structure, missing scheme, unsupported schemes (e.g. `ftp://`), userinfo credentials (`user:pass@`), query parameters, and fragments are rejected with clear, non-technical error banners.
- Continue button is disabled when input is empty or validation is in progress.
- Typing in the input field automatically clears active error banners.

## 10. Local Validation Rules
- Uses domain parsing in `ServerEndpoint(validating:)`.
- Scheme: `https` (or `http` for local development/testing).
- Rejects userinfo credentials, query parameters, and URL fragments.
- Conservatively trims leading/trailing whitespace and normalizes trailing slashes.

## 11. Tests Added & Updated
- `clients/ios/Tests/SynveilTests/ServerSetupViewModelTests.swift`: 11 test cases testing empty input, whitespace input, missing scheme, unsupported scheme, userinfo credentials, query parameters, URL fragments, valid HTTPS input, valid HTTP local dev input, whitespace trimming, and error clearing on input change.
- `clients/ios/Tests/SynveilTests/SessionControllerTests.swift`: Added `testConfigureServerEndpointSetsEndpointAndTransitionsState`.

## 12. Local Validation Commands & Results
- `python3 clients/ios/Support/validate_ios_sources.py` -> SUCCESS (0 violations)
- `python3 /home/jules/self_created_tools/check_pbxproj.py` -> SUCCESS (All Swift files registered)
- `cargo test -p synveil-core -p synveil-ios-ffi` -> PASSED (16 FFI unit tests green)

## 13. macOS / Xcode CI Results
- Workflow: iOS Simulator Tests & iOS Build (`.github/workflows/ios-simulator-tests.yml`, `.github/workflows/ios-build.yml`)
- Status: `PENDING_PR_RUN`

## 14. Simulator Validation Status
- Status: `PENDING_PR_RUN`

## 15. Physical-Device Validation Status
- Status: `NOT_AVAILABLE`

## 16. Security Considerations
- Zero credentials, tokens, or secrets collected or stored in Prompt022.
- HTTPS scheme enforced by default; HTTP permitted solely for local dev endpoints.
- No interaction with Keychain, UserDefaults, or unencrypted local persistence.

## 17. Known Limitations & Explicitly Deferred Work
- Server reachability health probe deferred to later Phase D prompt.
- Profile persistence to disk/Keychain deferred to later Phase D prompt.
- User authentication and session token exchange deferred to later Phase D prompt.

## 18. Commit & PR Metadata
- Branch: `ios/p022-server-setup`
- Final Commit SHA: `PENDING_COMMIT`
- PR URL: `PENDING_PR`
- Merge Status: `PENDING_MERGE`

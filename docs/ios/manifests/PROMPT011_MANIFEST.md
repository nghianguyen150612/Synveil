# Prompt011 Manifest — Configuration, Server URL & Safe Defaults

## 1. Metadata
- **Prompt**: `011`
- **Goal**: Establish the production-quality configuration model for the native Synveil iOS client (`v0.1`), providing strongly typed server base endpoint validation (`ServerEndpoint`), safe non-secret application configuration (`AppConfiguration`), test injection capabilities, and safe production defaults (`nil` server endpoint).
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `a36be638337a7c8c75b9effc598e63cd0ea315d0`
- **Actual Work Branch**: `ios/p011-configuration-15200694015697220485`
- **Validated Implementation Head**: `f9fce4830c9dc6631478fdd06001b9b3f25baed1`
- **PR**: #36 — https://github.com/nghianguyen150612/Synveil/pull/36
- **PR Base**: `ios-app`
- **Authoritative Files Inspected**:
  - `clients/ios/README.md`
  - `clients/ios/App/SynveilApp.swift`
  - `clients/ios/Application/README.md`
  - `clients/ios/Domain/README.md`
  - `clients/ios/Synveil.xcodeproj/project.pbxproj`
  - `clients/ios/Support/validate_ios_sources.py`
  - `.github/workflows/ios-static-validation.yml`
  - `.github/workflows/ios-build.yml`
  - `.github/workflows/ios-simulator-tests.yml`
  - `docs/ios/IOS_ARCHITECTURE.md`
  - `docs/ios/IOS_PLATFORM_MAPPING.md`
  - `docs/adr/ADR-058-ios-v0.1-client-architecture.md`
  - `docs/ios/manifests/PROMPT010_MANIFEST.md`
- **Android Parity References Inspected**:
  - Android v0.1 non-secret configuration loading and server URL validation contracts.

## 2. Configuration & Endpoint Policy
- **Configuration Types Created**:
  - `ServerEndpoint` (`Domain/Configuration/ServerEndpoint.swift`): strongly typed immutable value object representing normalized Synveil server base endpoints.
  - `EndpointValidationError` (`Domain/Configuration/EndpointValidationError.swift`): typed error taxonomy for endpoint parsing and safety validation failures.
  - `AppEnvironment` (`Application/Configuration/AppEnvironment.swift`): execution environment identity enum (`.production`, `.development`, `.testing`).
  - `AppConfiguration` (`Application/Configuration/AppConfiguration.swift`): non-secret application startup configuration struct with safe defaults and process override loader.
- **Validation Rules**:
  - Requires non-empty input string.
  - Requires valid URL structure parsed via `Foundation.URLComponents`.
  - Scheme must be `https` or `http` (case-insensitive).
  - Must specify a host component.
  - Must NOT contain username or password userinfo credentials (`user:pass@`).
  - Must NOT contain query parameters (`?param=val`).
  - Must NOT contain fragments (`#anchor`).
- **Normalization Rules**:
  - Surrounding whitespace trimmed.
  - Scheme and host lowercased.
  - Explicit ports preserved.
  - Root path normalized to `/` (for example, `https://example.com` becomes `https://example.com/`).
  - A trailing slash on an explicit non-root subpath is removed.
- **HTTP / HTTPS Policy**:
  - HTTPS is preferred for production transports.
  - HTTP is permitted for local development, home-LAN, or self-hosted testing servers.
  - `isSecureScheme` distinguishes HTTPS endpoints.
- **Base-Path Policy**:
  - Explicit subpaths are supported for reverse-proxy deployment setups.
- **Safe Production Default**:
  - `AppConfiguration.serverEndpoint == nil` by default.
  - No real server, localhost, loopback address, or personal endpoint is committed as a production default.
- **Environment & Build Override Policy**:
  - Optional non-secret process variables `SYNVEIL_SERVER_URL` and `SYNVEIL_APP_ENV` are read by `AppConfiguration.load(processEnvironment:)`.
  - Explicit dictionary injection allows tests to avoid mutating global process state.
  - An invalid server override is ignored and falls back to `nil`.
- **Secrets Policy**:
  - Configuration is strictly non-secret.
  - No bearer tokens (`svd1_`), enrollment tokens (`sve1_`), passwords, private keys, Apple credentials, or real personal server credentials are stored in source/configuration.
- **.xcconfig Decision**:
  - `.xcconfig` files were evaluated and omitted for P011; immutable Swift configuration values are sufficient without introducing build-setting complexity.

## 3. Project & Xcode Membership
- **Production Files Added**:
  - `clients/ios/Domain/Configuration/EndpointValidationError.swift`
  - `clients/ios/Domain/Configuration/ServerEndpoint.swift`
  - `clients/ios/Application/Configuration/AppEnvironment.swift`
  - `clients/ios/Application/Configuration/AppConfiguration.swift`
- **Test Files Added**:
  - `clients/ios/Tests/SynveilTests/ServerEndpointTests.swift`
  - `clients/ios/Tests/SynveilTests/AppConfigurationTests.swift`
- **App Bootstrap Wiring**:
  - `clients/ios/App/SynveilApp.swift` constructs `AppConfiguration.load()` at the composition root without adding networking, auth, or persistence.
- **Xcode Target Membership**:
  - Production source files are in the `Synveil` target.
  - Configuration unit tests are in `SynveilTests`.

## 4. Test Summary & Linux Verification
- **Unit Tests Added**:
  - `ServerEndpointTests`: 15 test cases covering valid HTTPS/HTTP endpoints, explicit ports, subpaths, whitespace, slash normalization, case normalization, invalid input classes, equality and hashing.
  - `AppConfigurationTests`: 6 test cases covering safe defaults, explicit injection, process overrides, malformed URL handling, and equality semantics.
- **P011 Test Count**: 21 test cases across 2 new XCTest files.
- **Linux Validation**:
  - `python3 clients/ios/Support/validate_ios_sources.py` -> SUCCESS.
  - `python3 -m unittest discover -s clients/ios/Support/tests` -> SUCCESS (12 validator self-tests).
  - `git diff --check` -> clean.
  - Secret/personal endpoint/path leakage review -> passed.

## 5. CI Evidence
### Validated implementation head: `f9fce4830c9dc6631478fdd06001b9b3f25baed1`
- **iOS Static Validation**: run `37089569471` — SUCCESS.
- **iOS Build**: run `37089569568` — SUCCESS.
- **iOS Simulator Tests**: run `37089569451` — SUCCESS.
- **Linux AppImage**: failed on the same head; classified `UNRELATED_SUBSYSTEM_FAILURE` for the known Linux/Desktop AppImage path check.
- Other broad repository CI may still be running or failing in server/desktop/platform-specific subsystems and is evaluated using the P005 taxonomy.

### Evidence-only manifest correction
This metadata correction changes only `docs/ios/manifests/PROMPT011_MANIFEST.md`. GitHub may rerun PR workflows because the PR as a whole still changes `clients/ios/**`; if so, the final PR head must again pass the three iOS gates before merge.

## 6. Scope / Limitations
- P011 establishes configuration value models and endpoint validation only.
- No URLSession/network connection or reachability probe was introduced (P023).
- No server setup UI was introduced (P022).
- No auth/session/Keychain persistence was introduced (P024+).
- No Rust FFI or transfer/file behavior was introduced.
- P011 does not persist server profiles.

## 7. Merge Status
- **PR #36**: open / ready for final-head verification and merge.
- **Final `ios-app` SHA**: PENDING_MERGE

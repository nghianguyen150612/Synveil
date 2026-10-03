# Prompt011 Manifest — Configuration, Server URL & Safe Defaults

## 1. Metadata
- **Prompt**: `011`
- **Goal**: Establish the production-quality configuration model for the native Synveil iOS client (`v0.1`), providing strongly typed server base endpoint validation (`ServerEndpoint`), safe non-secret application configuration (`AppConfiguration`), test injection capabilities, and safe production defaults (`nil` server endpoint).
- **Starting Integration Branch**: `ios-app`
- **Starting SHA**: `a36be638337a7c8c75b9effc598e63cd0ea315d0`
- **Actual Work Branch**: `ios/p011-configuration`
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
  - `ServerEndpoint` (`Domain/Configuration/ServerEndpoint.swift`): Strongly typed immutable value object representing normalized Synveil server base endpoints.
  - `EndpointValidationError` (`Domain/Configuration/EndpointValidationError.swift`): Typed error taxonomy for endpoint parsing and safety validation failures.
  - `AppEnvironment` (`Application/Configuration/AppEnvironment.swift`): Execution environment identity enum (`.production`, `.development`, `.testing`).
  - `AppConfiguration` (`Application/Configuration/AppConfiguration.swift`): Non-secret application startup configuration struct with safe defaults and process override loader.
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
  - Explicit non-default ports preserved (`:8443`).
  - Path normalized: trailing slash removed for subpaths (e.g. `/api/v1/` -> `/api/v1`), single root slash preserved (`/`).
- **HTTP / HTTPS Policy**:
  - HTTPS is preferred for production transports.
  - HTTP is permitted for local development, home-LAN, or self-hosted testing servers.
  - `isSecureScheme` helper boolean property distinguishes transport policy.
- **Base-Path Policy**:
  - Explicit subpaths (e.g. `https://example.com/synveil`) are supported for reverse-proxy deployment setups.
- **Safe Production Default**:
  - In production, `AppConfiguration.serverEndpoint` defaults to `nil` (unconfigured server).
  - `localhost` and `127.0.0.1` are strictly avoided as production defaults.
- **Environment & Build Override Policy**:
  - Optional non-secret process environment variables (`SYNVEIL_SERVER_URL` and `SYNVEIL_APP_ENV`) are supported via `AppConfiguration.load(processEnvironment:)` for local developer testing without mutating global process state.
- **Secrets Policy**:
  - Configuration is strictly non-secret.
  - No bearer tokens (`svd1_`), enrollment tokens (`sve1_`), passwords, private keys, or personal server endpoints are stored in source code or configuration files.
- **.xcconfig Decision**:
  - `.xcconfig` files were evaluated and omitted for P011 to avoid file clutter, as Swift value types (`AppConfiguration` and `ServerEndpoint`) fully satisfy all runtime, bootstrap, and testing requirements without build-setting complexity.

## 3. Project & Xcode Membership
- **Production Files Added**:
  - `clients/ios/Domain/Configuration/EndpointValidationError.swift`
  - `clients/ios/Domain/Configuration/ServerEndpoint.swift`
  - `clients/ios/Application/Configuration/AppEnvironment.swift`
  - `clients/ios/Application/Configuration/AppConfiguration.swift`
- **Test Files Added**:
  - `clients/ios/Tests/SynveilTests/ServerEndpointTests.swift`
  - `clients/ios/Tests/SynveilTests/AppConfigurationTests.swift`
- **Xcode Target Membership**:
  - Added production source files to `Synveil` app target in `project.pbxproj`.
  - Added unit test files to `SynveilTests` bundle target in `project.pbxproj`.

## 4. Test Summary & Verification
- **Unit Tests Added**:
  - `ServerEndpointTests` (15 test cases): Valid HTTPS/HTTP endpoints, explicit ports, subpaths, whitespace trimming, trailing slash normalization, case lowercasing, invalid inputs (empty, missing scheme, unsupported scheme, missing host, userinfo, queries, fragments), equality, and hashing.
  - `AppConfigurationTests` (6 test cases): Safe production default (`nil` endpoint), explicit injection, process environment overrides, malformed URL handling, and equality semantics.
- **Test Count**: 21 unit test assertions across 2 test files.
- **Linux Static Validation Result**:
  - `python3 clients/ios/Support/validate_ios_sources.py` -> SUCCESS (0 violations)
  - `python3 -m unittest discover -s clients/ios/Support/tests` -> SUCCESS (12 validator self-tests passed)
  - Secret scan & path leakage checks -> PASSED

## 5. CI Gate Status (To be populated on final PR head)
- **`iOS Static Validation`**: PENDING_PR
- **`iOS Build`**: PENDING_PR
- **`iOS Simulator Tests`**: PENDING_PR
- **Unrelated CI Failures / Classifications**: None.
- **PR Metadata**:
  - Commit SHA: PENDING_COMMIT
  - PR Number / URL: PENDING_PR
  - PR Base: `ios-app`
  - Merge Status: PENDING_MERGE
  - Final `ios-app` SHA: PENDING_FINAL_SHA

## 6. Limitations & Deferred Scope
- P011 establishes configuration value models and endpoint validation only; it does NOT introduce network connections, URLSession transport, or server reachability checks (deferred to P023).
- Does NOT establish server setup UI (deferred to P022).
- Does NOT persist server profiles or credentials to Keychain/UserDefaults (deferred to P021+).

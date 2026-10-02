# Synveil iOS — Test Organization (`clients/ios/Tests/`)

## Purpose & Ownership
Establishes the future test suite structure for the Synveil native iOS client.

### Test Categories
- `Tests/UnitTests/`: Unit tests for pure Domain models, ViewModels, and helper utilities.
- `Tests/IntegrationTests/`: Service integration tests using in-memory SQLite (`GRDB`), mocked `URLSession` (`URLProtocol`), and simulated Keychain vaults.
- `Tests/ArchitectureTests/`: Static checks and boundary assertion tests enforcing layer isolation.
- `Tests/Mocks/`: Test doubles and fakes (`MockHTTPTransport`, `MockCredentialVault`, `MockLocalCacheStore`, `MockRustBridge`).

## Rules
- Prompt006 defines the test layout without creating actual Swift test targets or executable test files.
- Xcode test target creation and CI runner execution are owned by P007–P009.

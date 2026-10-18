# Synveil iOS v0.1 Known Limitations

This document describes the verified v0.1 release-candidate boundaries as of P060. The production release decision is `BLOCKED_RELEASE`; this list does not authorize distribution or change the product contract.

## Baseline parity still missing

- **Labeled server profiles (PAR-03 / CAP-13A):** iOS setup configures one active server endpoint. It does not provide a persistent human-readable list of multiple server profiles.
- **Multi-profile switching and isolation (PAR-04 / CAP-13B):** iOS does not provide saved-profile switching. Do not emulate it by replacing the endpoint while a session is active. Credentials, SQLite state, pending mutations, and sync generations must be migrated/partitioned and tested together before this behavior is exposed.
- These are baseline `BLOCKED` rows, not accepted deviations. Existing single-profile data has no P060 multi-profile migration.

## Deliberate security and platform restrictions

- **Authenticated background content transfer:** Remains disabled because redirect and credential-policy concerns from P046/P051 are unresolved. Use the foreground upload/download flows.
- **File Provider:** P055 PR #139 is open and unmerged. Production File Provider domain registration and P055 offline content materialization are not included in the v0.1 release claim. Use the host app's supported browsing and foreground file operations.
- **PhotoKit:** P056–P058 implement an optional foreground host-app flow for local asset discovery/resource transfer. Uploaded FileVersions do not prove PhotoAsset binding, complete Live Photo retention, BackupSnapshot membership, or long-term backup guarantees. If an asset disappears, the app must not delete server content.
- **Background metadata:** BGAppRefresh is permission-aware, bounded, and opt-in by default. iOS scheduling is best-effort; precise periodic execution and Android WorkManager semantics are not promised.
- **Key storage:** Device credentials use Keychain `AfterFirstUnlockThisDeviceOnly`. No Secure Enclave storage is claimed. Device-to-device credential migration is not supported; re-enroll on the new device.
- **Endpoint policy:** P059 canonical origin validation remains enforced. Production requires HTTPS. Debug cleartext is limited to numeric loopback test endpoints and is not a production configuration.
- **Conflicts and destructive work:** Do not automatically overwrite remote content, resolve conflicts without user input, or delete remote content as a cleanup shortcut.

## Validation and distribution limits

- A passing iOS Simulator test suite is not an iPhone hardware acceptance result.
- Rust Apple cross-compilation validates target artifacts and ABI/build integration, not physical ARM64 runtime behavior.
- No authorized live authenticated Synveil server was available for P060, so the backend acceptance suite was not executed.
- No signed archive, IPA, or TestFlight build was produced in the Linux task environment. An unsigned simulator product is not distribution-ready.
- The inspected project has an iOS 17.0 deployment target and expected app/extension bundle identifiers, but explicit release version/build values and a privacy-manifest review remain outstanding.
- See [device validation](IOS_V0_1_DEVICE_VALIDATION.md) for manual instructions and [the release report](IOS_V0_1_RELEASE_REPORT.md) for the gate decision and prerequisites.

## Workarounds

- Use a single configured server origin per app installation until CAP-13 is safely implemented.
- Perform content operations while the app is foregrounded.
- Treat background metadata refresh as opportunistic and manually refresh when current data is needed.
- Use host-app file browsing/import/export rather than P055 File Provider materialization.
- Re-enroll credentials on a replacement device and verify server authorization before resuming work.

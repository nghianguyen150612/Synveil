# Synveil iOS v0.1 Device Validation Matrix

This is the reproducible manual acceptance suite for release gates that cannot be established by portable source checks or the iOS Simulator alone. No device or authorized live server was available for this P060 run. All manual cases below are `NOT_EXECUTED`; expected results are acceptance criteria, not observed results.

## Evidence classes

- `LINUX_VERIFIABLE`: portable scripts, source checks, and repository/documentation validation.
- `MACOS_CI_VERIFIABLE`: Xcode project, Rust Apple targets, and clean macOS build checks.
- `SIMULATOR_VERIFIABLE`: XCTest and simulated UI/lifecycle behavior; does not establish iPhone hardware behavior.
- `PHYSICAL_DEVICE_ONLY`: behavior that requires a real iPhone and signed installation.
- `SIGNING_REQUIRED`: archive, install, or distribution behavior requiring approved Apple signing configuration.

## Test setup

1. Use a clean macOS checkout of the exact reviewed release candidate. Record the full source SHA, macOS version, Xcode version, iPhone model, iOS version, build number, and test-server fixture identity.
2. Use only an authorized Synveil staging server with test accounts, test libraries, and non-sensitive files. Record the server build/version and reset fixture state before each run. Never put real bearer/enrollment tokens, private user files, or private payloads in logs or screenshots.
3. Build the signed candidate with approved distribution settings. Keep the exported IPA and archive checksum in the controlled release evidence store, not in Git.
4. Capture each result as `PASS`, `FAIL`, or `NOT_EXECUTED`, link the relevant sanitized log/result bundle, and record recovery steps. A test without a physical device or fixture remains `NOT_EXECUTED`.

## Acceptance matrix

| Scenario | Class | Manual instruction | Expected result | P060 result |
|---|---|---|---|---|
| Fresh installation and first launch | `PHYSICAL_DEVICE_ONLY`, `SIGNING_REQUIRED` | Install the signed candidate on a supported iPhone with no prior Synveil container. Launch, deny optional notifications and Photos access initially, and complete server setup against the authorized fixture. | App opens without a stored session, explains required setup, requests only necessary permissions at the point of use, and makes no request to an implicit localhost/test endpoint. | `NOT_EXECUTED` |
| Upgrade from the existing v0.1 development baseline | `PHYSICAL_DEVICE_ONLY`, `SIGNING_REQUIRED` | Install the previous accepted development build, create test cache/queued state, then install the signed candidate over it without deleting the app. | App data remains readable; any migration is versioned, atomic, repeatable, and preserves or safely blocks pending work. No credential is sent to another origin. | `NOT_EXECUTED` |
| Saved-profile migration and restart recovery | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | After CAP-13 implementation, seed a single-profile v0.1 installation, launch the new build, interrupt at each migration boundary, force-quit, relaunch, and repeat the migration. | Legacy endpoint/session is mapped exactly once; interrupted work resumes safely; repeated launches do not duplicate or discard profiles, credentials, cache, queues, or generations. Current P060 contains no such implementation. | `NOT_EXECUTED` |
| Enrollment and session restoration | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Enroll with a one-time fixture token, terminate the app, relaunch online, then repeat with the fixture offline and with a revoked test device. | Token exchange occurs once; the DeviceBearer is scoped to its endpoint/device; authorized session restores; transient outage stays recoverable; revoked credentials fail closed and are removed only according to the documented cleanup contract. | `NOT_EXECUTED` |
| Keychain ThisDeviceOnly behavior | `PHYSICAL_DEVICE_ONLY` | Enroll on iPhone A, verify the Keychain item accessibility in a test build or sanitized diagnostic, restore a backup to iPhone B, then launch there. | Credential uses `AfterFirstUnlockThisDeviceOnly`; it is not migrated to another device. No Secure Enclave claim is made. Device B requires a fresh authorized enrollment. | `NOT_EXECUTED` |
| Profile isolation during switching | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | After CAP-13 implementation, seed profiles A and B with distinct fixture identities. Hold an authenticated request and sync operation on A, switch to B, then let the old callbacks return. | Switch invalidates prior lifecycle revisions; stale responses cannot update B; credentials, caches, mutation queues, and sync generations remain profile/identity scoped. | `NOT_EXECUTED` |
| Queued mutations and logout | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Queue an offline metadata mutation on profile A, switch to B, restore connectivity, and then log out/delete A with pending work present. | A's mutation never reaches B. Logout/deletion follows the product contract, removes only the intended profile credential/data, and surfaces blocked cleanup instead of pretending it succeeded. | `NOT_EXECUTED` |
| Revocation and stale callback rejection | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Start a delayed authenticated request, revoke the device on the fixture, trigger restoration/revocation handling, and release the old response. | Revocation closes authenticated access; stale callbacks cannot restore it; sanitized user-facing errors contain no token, credential, or response body. | `NOT_EXECUTED` |
| Foreground upload/download integrity | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Upload and download fixture files of zero, small, chunk-boundary, and multi-chunk sizes. Compare SHA-256 and byte lengths locally and on the fixture. | Transfers use authorized server state, verify exact digest/length, honor revision preconditions, and publish downloads atomically only after validation. | `NOT_EXECUTED` |
| Interrupted transfer recovery | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Interrupt network, force-quit, and reboot at upload/download boundaries. Resume from server-authoritative offsets/state and repeat the same idempotency key after uncertain responses. | No duplicate destructive operation, blind overwrite, or client-only offset assumption; partial staging remains private and recoverable or safely discarded. | `NOT_EXECUTED` |
| SQLite transaction and crash recovery | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Terminate the app during cache projection, mutation enqueue/drain, download metadata commit, and synchronization checkpoint updates. Relaunch and inspect through test diagnostics. | Transactions leave either the previous committed state or the complete new state. No acknowledged checkpoint advances ahead of durable projection/ACK state. | `NOT_EXECUTED` |
| Storage exhaustion | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Constrain device free space, then download/import/upload while monitoring app behavior. Recover space and retry. | App reports a recoverable storage error, preserves server-authoritative data and pending operation identity, and does not publish truncated files or delete remote data automatically. | `NOT_EXECUTED` |
| Notification permissions | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Exercise first-run allow/deny, later Settings changes, and logout while notifications are pending. | Notification work is permission-aware and private; denied authorization does not block core use; logged-out/stale-profile events are not navigated into authenticated content. | `NOT_EXECUTED` |
| PhotoKit permissions and source disappearance | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Exercise Photos denied, limited, and full access. Start a foreground backup, remove a selected source asset, and inspect the server fixture. | Access matches the system selection. Asset disappearance stops/records that local source; it does not delete server content or claim PhotoAsset/Live Photo/BackupSnapshot linkage. | `NOT_EXECUTED` |
| Background metadata refresh | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | With explicit opt-in, schedule BGAppRefresh, background/terminate the app, change notification/network conditions, and inspect sanitized fixture requests. | Refresh is best-effort, bounded, permission-aware, and limited to documented metadata work. No periodic timing guarantee or content transfer occurs. | `NOT_EXECUTED` |
| App foreground/background lifecycle | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Background and resume during enrollment, sync, transfer, export, and profile switch; also lock the screen and reboot. | Protected operations stop or resume only through documented durable recovery; callbacks from old lifecycle/session revisions are ignored. | `NOT_EXECUTED` |
| VoiceOver, Dynamic Type, and error recovery | `SIMULATOR_VERIFIABLE`, `PHYSICAL_DEVICE_ONLY` | Enable VoiceOver and the largest supported Dynamic Type size. Navigate setup, profiles, library/nodes, conflict/recovery screens, file import/export, and permission denial. | Focus order and labels communicate the active server/profile and state; text remains usable; errors expose safe, actionable recovery choices. | `NOT_EXECUTED` |
| File Data Protection | `PHYSICAL_DEVICE_ONLY` | Inspect files created in app containers with supported device diagnostics. Lock the device before first unlock, after first unlock, and during a transfer. | Private app data uses the intended platform protection; lock-state access failures are handled without weakening protection or losing durable state. | `NOT_EXECUTED` |
| Performance and memory stability | `PHYSICAL_DEVICE_ONLY` | Profile launch, browsing, large-file transfer, repeated foreground/background cycles, and representative long sessions on the minimum supported device. | No unbounded memory growth, UI hangs, or crash under documented workloads; record measurements and device conditions against an agreed budget. | `NOT_EXECUTED` |
| Release archive, install, and TestFlight upgrade | `SIGNING_REQUIRED`, `PHYSICAL_DEVICE_ONLY` | On macOS, archive with the approved team/profile, validate the IPA and embedded extension, install on a registered device, then distribute to an internal TestFlight group and upgrade from the previous accepted build. | Correct signed bundle IDs, entitlements, version/build, privacy metadata, Rust linkage, and upgrade path; no test server/debug configuration in the archive. | `NOT_EXECUTED` |

## P060 automated evidence

P060 simulator XCTest counts and workflow URLs are recorded in [the P060 manifest](manifests/PROMPT060_MANIFEST.md) after CI completes. Automated simulator tests cover mock/service behavior only; they do not change the manual rows above from `NOT_EXECUTED` without the required device and backend evidence.

## Result recording template

For each executed row, add:

```text
Result: PASS | FAIL
Source SHA:
Build number / IPA checksum:
Device model / iOS version:
Server fixture build / test identity:
Steps actually completed:
Observed result:
Sanitized evidence link:
Failure and recovery notes:
Tester / UTC timestamp:
```

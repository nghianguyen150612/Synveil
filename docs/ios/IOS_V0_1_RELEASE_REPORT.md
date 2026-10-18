# Synveil iOS v0.1 Release Report

**Decision: `BLOCKED_RELEASE`**

**Decision date:** 2026-10-18

**Release candidate:** P060 on `ios/p060-final-release-gate`

**P060 PR head:** `e55a19fbbc95c54cd3c1a1fc2d29b7bcf8e77f92`

**`ios-app` integration merge:** `8aba5f7ac90e7e56440f3c0542d7b0f86415cd8e` (PR #144; merge commit)

**App source base:** `40080e3e7ed17bafb4f5ee89b8637b1954bc626a`; P060 changed release documentation/metadata only.

## Executive decision

P060 closes the v0.1 release assessment with a blocked decision. No distribution artifact is approved. The two baseline parity requirements for saved labeled server profiles and isolated multi-profile switching remain `BLOCKED`. Required physical-device and live authenticated-server acceptance has not been performed, and this Linux environment cannot create an Xcode archive or signed IPA. The P060 engineering closeout is distinct from product release authorization.

The P059 capability totals remain unchanged because P060 did not implement or receive an owner-approved scope revision for CAP-13:

| Status | Count |
|---|---:|
| PASS | 18 |
| IOS_EQUIVALENT | 12 |
| NOT_APPLICABLE | 6 |
| INTENTIONALLY_DEFERRED | 4 |
| BLOCKED | 2 |
| **Total** | **42** |

## Release blockers

| Blocker | Why it matters | Evidence available | Resolution required |
|---|---|---|---|
| PAR-03 / CAP-13A: no persistent labeled server-profile list | Users cannot retain and identify multiple server origins. | P059 parity matrix and audit; inspection of the current onboarding and single-endpoint `SessionController`. | Implement versioned profile persistence and migration, then validate create, select, update, delete, and restart behavior. |
| PAR-04 / CAP-13B: no saved multi-profile switching with isolated sessions | Credentials, SQLite state, queued mutations, and synchronization generations must not cross server or device identities. A UI-only switch would be unsafe. | Current composition uses one active endpoint and one Keychain session; durable stores and background coordinators are composed around that session. | Implement and test coordinated session invalidation, storage partitioning, and no-wrong-server replay across requests, sync, queued mutations, logout, and restart. |
| Physical-device acceptance is missing | Simulator and static checks do not establish iPhone Keychain, permissions, lifecycle, storage-pressure, Data Protection, or performance behavior. | See [device validation matrix](IOS_V0_1_DEVICE_VALIDATION.md); every physical-only scenario is `NOT_EXECUTED`. | Run the documented manual suite on a supported iPhone with a release-signed build and record the device/OS and results. |
| Live authenticated-server acceptance is missing | Mocked transport tests cannot prove enrollment, revocation, authorization, or transfer behavior against the authorized service. | No authorized live fixture or authenticated session was available in this task environment. | Provide an approved Synveil test server and execute the documented authenticated scenarios without logging credentials or private payloads. |
| Signed archive and TestFlight prerequisites are missing | An unsigned simulator build is not distributable. | Host is Linux x86_64 with no `xcodebuild`, signing identity, provisioning profile, or App Store Connect credentials available to this task. No archive or IPA was produced. | Use macOS/Xcode with an approved Apple team, signing assets, bundle registration, and App Store Connect access; validate the exported IPA before TestFlight upload. |
| Release version/build metadata and privacy-manifest review are incomplete | Distribution settings must be explicit and reviewed for the submitted archive. | The checked-in project declares iOS 17.0 and bundle IDs, but no `MARKETING_VERSION` or `CURRENT_PROJECT_VERSION` setting or `PrivacyInfo.xcprivacy` file was found in the inspected iOS project paths. | Set release version/build values, review required privacy declarations for collected APIs/data, inspect the generated archive's Info.plist and entitlements, and rerun static checks. |

## P060 implementation scope and parity

P060 deliberately leaves PAR-03 and PAR-04 at `BLOCKED`. The current iOS architecture composes a single `serverEndpoint`, one active device credential, and long-lived SQLite and work coordinators around the active session. Introducing a profile picker without a versioned migration and cross-cutting identity invalidation would create a wrong-server replay and cross-profile cache risk. No product-scope revision was supplied, so the P059 parity classification is preserved.

P060 updates release documentation and the client README/roadmap only. It does not change the app source, SQLite schema, authorization policy, endpoint validation, background transfer policy, File Provider state, or PhotoKit behavior. See the manifest for the exact changed-file list and validation records.

## CI and reproducibility

All four mandatory iOS workflows passed on P060 PR head `e55a19f` (PR #144). The Simulator Tests run recorded 2,024 passed, 2 skipped, and 0 failed. These results are fresh P060 evidence; P059 results were not reused.

| Required workflow | Result | P060 run |
|---|---|---|
| iOS Static Validation | PASS | [39101765939](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765939) |
| iOS Build | PASS | [39101765842](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765842) |
| iOS Rust Apple Build | PASS | [39101765925](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765925) |
| iOS Simulator Tests | PASS — 2,024 passed, 2 skipped, 0 failed | [39101765997](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765997) |

The PR runs used a clean GitHub-hosted `macos-26-arm64` runner (macOS 26.6.2), Xcode 26.6 build 17F113, Swift 6.3.3, and iOS Simulator SDK 26.5. The simulator suite selected an iPhone 17 Pro simulator on iOS 26.5. The app and test targets used `CODE_SIGNING_ALLOWED=NO`. Simulator results do not count as physical-device acceptance.

The exact unsigned simulator build command was:

```bash
xcodebuild \
  -project clients/ios/Synveil.xcodeproj \
  -scheme Synveil \
  -configuration Debug \
  -sdk iphonesimulator \
  CODE_SIGNING_ALLOWED=NO \
  -derivedDataPath "$RUNNER_TEMP/SynveilDerivedData" \
  build
```

The temporary app bundle was `$RUNNER_TEMP/SynveilDerivedData/Build/Products/Debug-iphonesimulator/Synveil.app`; it was not exported or uploaded as a release product. The simulator test result bundle was `$RUNNER_TEMP/SynveilTests.xcresult` and was uploaded as `ios-simulator-test-results` with seven-day retention. The Rust Apple workflow passed and uploaded `synveil-ios-rust-staticlibs` from `target/ios-rust-artifacts/` with 14-day retention. That artifact cross-compiles `aarch64-apple-ios`, `aarch64-apple-ios-sim`, and `x86_64-apple-ios`; it does not establish physical-device runtime.

This Linux task host had Python 3 and GitHub CLI but no `xcodebuild`, `swift`, `swift-format`, `cargo`, or `rustc`. Local native builds and physical-device checks were therefore not run.

### Repository-wide failures

The full repository workflows were not reported as green. Their results and classifications are recorded in the [P060 manifest](manifests/PROMPT060_MANIFEST.md): Rust CI, Linux AppImage, Linux DEB/RPM, and PostgreSQL scheduled maintenance failed outside the iOS client checks. The four required iOS workflows above passed.

## Source-level correctness and security review

- Authentication review covered one-time enrollment exchange, DeviceBearer authorization, Keychain persistence/restoration, revocation, and local logout cleanup. Existing XCTest coverage includes enrollment, session restoration, Keychain, logout, and stale-session handling. The Keychain implementation uses `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; no Secure Enclave claim is made.
- Transfer and durability review covered foreground resumable uploads, authoritative server-offset reconciliation, download length/digest verification, redirect rejection, revision/idempotency preconditions, private staging, and SQLite queue persistence. The simulator suite passed upload, download, queue, recovery, sync, and interrupted-state tests. No live authenticated server was available, so server acceptance remains unexecuted.
- Rust/Swift FFI review covered ABI version/status mapping, length-delimited buffers, allocation/release ownership, bounded pointer handling, and panic containment. The bridge denies unsafe code by default and confines it to documented C ABI operations; ABI, memory, and concurrency XCTest suites ran in Simulator CI. Apple-target compilation is cross-compilation, not device runtime validation.
- UX/lifecycle review covered onboarding and recovery surfaces, permission state handling, background/foreground transitions, notifications, file import/export, accessibility labels, and Dynamic Type test coverage. Actual VoiceOver, permission, storage-pressure, and Data Protection behavior still require the physical-device matrix.
- The P060 tracked-file scan found no signing artifacts. A narrow production Swift logging scan found no `print`, `NSLog`, or `debugPrint` use and no token-bearing log call; the only bearer-shaped literal found was a synthetic `sve1_testtoken123` fixture under XCTest.

## Security assessment

- P059 canonical `ServerEndpoint` validation remains in place. HTTPS is required in production; the existing debug exception is restricted to numeric loopback test hosts.
- The active device credential is stored by the Keychain implementation with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; this report makes no Secure Enclave claim.
- Existing code and tests are structured to keep bearer credentials out of SQLite and user-facing errors. P060 did not alter credential handling or transport code. This is a source review, not a claim of a production penetration test.
- Error-message redaction, revocation behavior, Keychain restoration, and logout cleanup still require the device/server acceptance steps in the matrix.
- No production signing material was added by P060. The final tracked-file and secret scan results are recorded in the manifest.
- Background content transfers remain disabled pending the P046/P051 redirect and credential-policy gate. BGAppRefresh remains opt-in, bounded, permission-aware, and best-effort.
- P055 PR #139 remains separate and unmerged; no P055 materialization feature or production File Provider registration is represented as released.
- P056–P058 PhotoKit behavior remains an optional foreground host-app flow. It does not establish PhotoAsset binding, Live Photo completeness, BackupSnapshot association, or retained-backup guarantees. PhotoKit asset disappearance does not authorize server deletion.

## Artifact and distribution status

| Item | Status |
|---|---|
| iOS deployment target | iOS 17.0 declared in the project |
| App bundle identifier | `com.synveil.ios` |
| File Provider extension bundle identifier | `com.synveil.ios.fileprovider`; production domain registration remains disabled |
| Version/build values | Not explicitly configured in the inspected project settings; release blocker |
| Machine-readable release status | [`clients/ios/release-status.json`](../../clients/ios/release-status.json) |
| Privacy manifest | No `PrivacyInfo.xcprivacy` found under the inspected iOS project paths; review required |
| Simulator build | P060 CI status recorded in the manifest; not a distribution artifact |
| Rust Apple static libraries | P060 CI artifact status recorded in the manifest; cross-compilation does not prove physical-device runtime |
| `.xcarchive` / signed `.ipa` | Not produced; Xcode and signing assets unavailable |
| TestFlight | Not submitted; upload authorization and Apple distribution prerequisites are absent |

The configured unsigned build invocation is:

```bash
xcodebuild \
  -project clients/ios/Synveil.xcodeproj \
  -scheme Synveil \
  -configuration Debug \
  -sdk iphonesimulator \
  CODE_SIGNING_ALLOWED=NO \
  -derivedDataPath "$RUNNER_TEMP/SynveilDerivedData" \
  build
```

No release archive command was executed because the required macOS toolchain and signing configuration were unavailable. Do not treat the command above or a simulator bundle as an IPA.

## TestFlight rollout and rollback preparation

When the blockers are resolved, prepare a signed archive from the reviewed release tag on a clean macOS runner. Confirm app and extension bundle identifiers, version/build, privacy declarations, entitlements, embedded Rust libraries, and absence of test URLs/debug permissions. Export an IPA using the approved App Store Connect distribution profile, inspect the export and archive metadata, and upload to an internal TestFlight group only after the release owner authorizes distribution.

The initial TestFlight cohort should be internal testers on the supported iOS range. Record the build number, source tag, archive checksum, tester cohort, installation result, and upgrade-from-v0.1 result. Hold expansion until the device matrix passes. For rollback, stop phased rollout or remove the build from testing, communicate the affected build number, preserve diagnostic evidence without credentials or private files, and install the last accepted build through the approved channel. Do not delete server content or local evidence as part of rollback.

## Final recommendation

Keep the release blocked. Implement and test profile isolation/migration, execute the physical-device and authorized-server acceptance matrix, finish version/privacy/signing setup, and satisfy release review requirements before considering TestFlight. Do not integrate `ios-app` into `main` while these release gates remain open.

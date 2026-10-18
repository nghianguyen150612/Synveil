# Synveil iOS v0.1 Release Report

**Decision: `BLOCKED_RELEASE`**  
**Decision date:** 2026-10-18  
**Release candidate:** P060 on `ios/p060-final-release-gate`  
**Source revision:** P060 source changes are documentation-only; the iOS source remains based on `40080e3e7ed17bafb4f5ee89b8637b1954bc626a` until the P060 PR result is recorded below.

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
| P060 CI and independent review are required for integration | A local documentation check does not replace the four native macOS gates or repository review protections. | P060-specific run URLs and PR/review state are recorded in the [P060 manifest](manifests/PROMPT060_MANIFEST.md) when available. Branch-protection details were not readable through the available GitHub API credential. | Require successful P060 checks and satisfy the repository's normal review/protection rules before merging into `ios-app`. |

## P060 implementation scope and parity

P060 deliberately leaves PAR-03 and PAR-04 at `BLOCKED`. The current iOS architecture composes a single `serverEndpoint`, one active device credential, and long-lived SQLite and work coordinators around the active session. Introducing a profile picker without a versioned migration and cross-cutting identity invalidation would create a wrong-server replay and cross-profile cache risk. No product-scope revision was supplied, so the P059 parity classification is preserved.

P060 updates release documentation and the client README/roadmap only. It does not change the app source, SQLite schema, authorization policy, endpoint validation, background transfer policy, File Provider state, or PhotoKit behavior. See the manifest for the exact changed-file list and validation records.

## CI and reproducibility

The P059 historical evidence is not reused as P060 evidence. The P060 manifest records each fresh run against the P060 PR head, including exact workflow URLs and simulator test counts. Linux-local checks and failures are also listed separately from macOS-only gates.

The four required native workflows are:

1. iOS Static Validation
2. iOS Build
3. iOS Rust Apple Build
4. iOS Simulator Tests

The Xcode workflows use clean GitHub-hosted macOS checkouts and their commands are defined in `.github/workflows/ios-*.yml`. On this Linux task host, `xcodebuild`, `swift`, `swift-format`, `cargo`, and `rustc` are unavailable. A clean macOS CI run is therefore required for native reproducibility. Simulator results do not count as physical-device acceptance.

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

Keep the release blocked. Complete profile isolation and migration, run all required P060 engineering checks, execute the physical-device and authorized-server acceptance matrix, finish version/privacy/signing setup, and obtain required independent review before considering TestFlight. Do not integrate `ios-app` into `main` while these release gates remain open.

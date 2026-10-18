# PROMPT060 — Final Release Gate Manifest

## Starting state and history

- Repository: `https://github.com/nghianguyen150612/Synveil`
- Required branch: `ios/p060-final-release-gate`
- Starting `origin/ios-app` SHA: `40080e3e7ed17bafb4f5ee89b8637b1954bc626a`
- Starting `origin/main` SHA: `525560312c6350b69d766f48f607441de7d272ad`
- Local checkout before work: clean; initially on `work` at the main SHA.
- P060 branch was created directly from the fetched `origin/ios-app` SHA above. The P060 worktree was clean before edits.
- `origin/ios-app` is the P059 merge commit `40080e3`; its parents are `c0c4c42ed41f589f5c7782e697824d89c4f8e907` and P059 head `c7d671ddd7509321784a23bbb7521b163a8f4810`.
- P059 source-fix `373ae476d7045d8cbfb709b256085e6442a69163` and audit closeout `3c67e190bbab002c2609f748a1a52ef192be3190` are ancestors of the P060 starting point.
- `git merge-base origin/main origin/ios-app`: `179fd5f0c6bfd098cb10f29f38aea10a33711a2b`.
- At start, `origin/main` did not contain the iOS integration history from `origin/ios-app`. No P060-to-main integration was attempted; the verified release gates remain blocked.
- Outstanding PR #139 is P055 File Provider materialization, state `OPEN`, base `ios-app`; excluded from P060.
- PR #143 is `MERGED` into `ios-app` with merge commit `40080e3e7ed17bafb4f5ee89b8637b1954bc626a`.

## Scope and decision

P060 closes the engineering release gate with `BLOCKED_RELEASE`. CAP-13A/PAR-03 and CAP-13B/PAR-04 remain blocked. No scope revision was present, and a safe profile feature requires coordinated migration and identity isolation across Keychain restoration, SQLite state, pending mutations, lifecycle callbacks, and background work. P060 records these facts instead of introducing an unvalidated endpoint-only selector.

The release report, device validation matrix, known limitations, manifest, client README, machine-readable release status, and v0.1 roadmap are updated. P059 parity counts remain 18 PASS, 12 IOS_EQUIVALENT, 6 NOT_APPLICABLE, 4 INTENTIONALLY_DEFERRED, and 2 BLOCKED. P059 parity matrix/audit rows are not edited because P060 adds no fresh CAP-13 implementation evidence or approved product decision.

## Changed files

P060 PR #144 changed:

- `clients/ios/README.md`
- `clients/ios/release-status.json`
- `docs/ios/IOS_V0_1_DEVICE_VALIDATION.md`
- `docs/ios/IOS_V0_1_KNOWN_LIMITATIONS.md`
- `docs/ios/IOS_V0_1_RELEASE_REPORT.md`
- `docs/ios/IOS_V0_1_ROADMAP.md`
- `docs/ios/manifests/PROMPT060_MANIFEST.md`

No application source, SQLite schema, signing material, entitlement, or workflow implementation is in scope. A documentation-only follow-up PR records the completed CI and merge evidence.

## Validation

Commands and exact results will be appended after execution. Intended portable commands:

```bash
python3 -m unittest discover -s clients/ios/Support/tests
python3 clients/ios/Support/validate_ios_sources.py
bash scripts/validate-docs.sh
git diff --check
```

The host is Linux x86_64. At initial inspection it had Python 3 and GitHub CLI, but no `xcodebuild`, `swift`, `swift-format`, `cargo`, or `rustc`. Initial local results:

- `python3 -m unittest discover -s clients/ios/Support/tests`: **PASS**, 120 tests, 0 failures.
- `python3 clients/ios/Support/validate_ios_sources.py`: **PASS**, all iOS static source and architecture invariants passed.
- `bash scripts/validate-docs.sh`: **PASS**, documentation validation passed.
- `python3 -m json.tool clients/ios/release-status.json`: **PASS**.
- `git diff --check`: **PASS** before the primary P060 commit; rerun after closeout edits.
- Tracked signing-file scan: no `.mobileprovision`, `.provisionprofile`, `.p12`, `.p8`, `.pem`, `.key`, or `.cer` files found.
- Production Swift logging/token scan: no production `print`, `NSLog`, `debugPrint`, or token-bearing logging matches. The only bearer-shaped literal found by the narrow auth scan was a synthetic `sve1_testtoken123` fixture in `HTTPTransportContractTests.swift`.
- Rust/Xcode local gates: `NOT_AVAILABLE`; required toolchains are not installed on this host.

Native checks require the repository's four macOS workflows:

1. iOS Static Validation
2. iOS Build
3. iOS Rust Apple Build
4. iOS Simulator Tests

| Gate | P060 URL | Result | Simulator counts / artifact |
|---|---|---|---|
| iOS Static Validation | [PASS](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765939) | `e55a19fbbc95c54cd3c1a1fc2d29b7bcf8e77f92` | Clean macOS runner |
| iOS Build | [PASS](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765842) | `e55a19fbbc95c54cd3c1a1fc2d29b7bcf8e77f92` | Xcode 26.6; unsigned simulator app |
| iOS Rust Apple Build | [PASS](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765925) | `e55a19fbbc95c54cd3c1a1fc2d29b7bcf8e77f92` | Apple static-library artifact uploaded; no device runtime claim |
| iOS Simulator Tests | [PASS](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765997) | `e55a19fbbc95c54cd3c1a1fc2d29b7bcf8e77f92` | **2,024 passed, 2 skipped, 0 failed**; iPhone 17 Pro simulator, iOS 26.5 |

The PR runs used `macos-26-arm64` / macOS 26.6.2, Xcode 26.6 build 17F113, Swift 6.3.3, and iOS Simulator SDK 26.5. The CI checkout used the GitHub PR merge ref containing the P060 head; the P060 head SHA is shown in the table. The build command was the `xcodebuild -project clients/ios/Synveil.xcodeproj -scheme Synveil -configuration Debug -sdk iphonesimulator CODE_SIGNING_ALLOWED=NO -derivedDataPath "$RUNNER_TEMP/SynveilDerivedData" build` invocation from `.github/workflows/ios-build.yml`. The simulator app bundle was `$RUNNER_TEMP/SynveilDerivedData/Build/Products/Debug-iphonesimulator/Synveil.app`; it was not signed or uploaded as a release artifact. The XCTest result bundle was `$RUNNER_TEMP/SynveilTests.xcresult` and uploaded as `ios-simulator-test-results` for seven days. Rust Apple libraries were staged at `target/ios-rust-artifacts/` and uploaded as `synveil-ios-rust-staticlibs` for 14 days.

P059 runs remain historical only. The four rows above are fresh P060 evidence.

### Repository-wide workflow classification

These failures are recorded as failures, not passes. P060 changed no Rust workspace, desktop, Web, Linux package, or PostgreSQL source paths.

| Workflow | P060 result | P060 run | Base comparison / taxonomy |
|---|---|---|---|
| Rust CI | FAILURE; eight failed jobs match the base run, plus `Web quality` | [39101765900](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765900) | `PRE_EXISTING_BASELINE_FAILURE` for matching workspace/desktop/dependency jobs, compared with [base run 39095679231](https://github.com/nghianguyen150612/Synveil/actions/runs/39095679231). `Web quality` is `UNRELATED_SUBSYSTEM_FAILURE`; P060 did not touch Web paths. P059 records known shared-workspace causes, including unsupported Linux `systemd-tmpfiles --dry-run` and dependency policy advisories. |
| Linux AppImage | FAILURE | [39101765831](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765831) | `PRE_EXISTING_BASELINE_FAILURE`: same failed AppImage job as [base run 39095679265](https://github.com/nghianguyen150612/Synveil/actions/runs/39095679265); P059 recorded the `/home/` artifact-path scan failure. |
| Linux native packages (DEB + RPM) | FAILURE | [39101765881](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765881) | `UNRELATED_SUBSYSTEM_FAILURE`; same failed package job was present on [base run 39095679306](https://github.com/nghianguyen150612/Synveil/actions/runs/39095679306); P060 changed no package sources. |
| PostgreSQL 17 scheduled maintenance | FAILURE | [39101765821](https://github.com/nghianguyen150612/Synveil/actions/runs/39101765821) | `UNRELATED_SUBSYSTEM_FAILURE`; same failed maintenance job was present on [base run 39095679365](https://github.com/nghianguyen150612/Synveil/actions/runs/39095679365); P060 changed no server/database sources. |

## Security and acceptance evidence

- Keychain source inspection confirms `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; no Secure Enclave claim is made.
- Current endpoint validation and HTTPS policy are preserved.
- No P060 authenticated live-server or physical-device session was available. See `IOS_V0_1_DEVICE_VALIDATION.md`; manual cases are `NOT_EXECUTED` with expected results and repeatable instructions.
- No archive, signed IPA, or TestFlight submission was produced. The inspected project declares iOS 17.0 and bundle IDs, but explicit `MARKETING_VERSION`/`CURRENT_PROJECT_VERSION` and a `PrivacyInfo.xcprivacy` file were not found in the inspected iOS project paths.
- Background transfer, File Provider, PhotoKit, and BGAppRefresh boundaries are documented in the release report and known limitations.
- Final secret/unsafe logging scan: see local results above; repeat on the final tracked diff before merge.

## GitHub workflow and integration record

- P060 PR #144: [opened and merged](https://github.com/nghianguyen150612/Synveil/pull/144) using merge-commit method.
- P060 head commit SHA: `e55a19fbbc95c54cd3c1a1fc2d29b7bcf8e77f92`.
- P060 merge commit into `ios-app`: `8aba5f7ac90e7e56440f3c0542d7b0f86415cd8e`; parents are `40080e3e7ed17bafb4f5ee89b8637b1954bc626a` and `e55a19fbbc95c54cd3c1a1fc2d29b7bcf8e77f92`.
- `ios-app` to `main` PR/merge: not performed; blocked by unresolved baseline parity, missing physical/backend acceptance, and unsigned artifact.
- `gh pr checks 144 --required` reported no required checks for the head branch. The branch-protection API returned HTTP 403 (`Resource not accessible by integration`); PR #144 was merged through GitHub's normal merge operation and no override.
- Verified with `git merge-base --is-ancestor`: original `ios-app` head `40080e3`, P059 source-fix `373ae476`, P059 audit closeout `3c67e19`, and P060 head `e55a19f` are ancestors of `ios-app` merge `8aba5f7`. The merge has the expected two parents. `origin/main` remains `5255603`; no iOS integration into `main` was made.

## Final milestone status

P060 engineering closeout: complete with `BLOCKED_RELEASE`; P060 documentation and evidence are integrated into `ios-app`.
Production release decision: `BLOCKED_RELEASE`.


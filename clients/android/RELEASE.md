# Synveil Android release readiness

This document describes the production build boundary for the Android client.
It does not publish to a store or merge `android-app` into `main`.

## Release identity

- Application ID: `com.synveil.android`.
- Default version name: `0.1.0`.
- Default version code: `1`.
- Override version metadata only from Gradle properties or CI configuration:
  `-PsynveilVersionName=... -PsynveilVersionCode=...`.
- Debug builds use `com.synveil.android.debug` and a `-debug` version suffix;
  release builds retain the production application ID and are not debuggable.

The release build is unsigned unless external signing values are supplied. This
keeps signing identity and passwords out of the repository while preserving a
reproducible unsigned artifact for inspection. A distributable APK/AAB must be
built with the protected CI or release-engineering keystore.

## Build and inspect

From `clients/android`:

```bash
./gradlew --no-daemon clean assembleDebug assembleRelease bundleRelease
./gradlew --no-daemon test
./gradlew --no-daemon lint
./scripts/validate-release-artifact.sh \
  --apk=app/build/outputs/apk/release/app-release-unsigned.apk \
  --aab=app/build/outputs/bundle/release/app-release.aab
```

Release shrinking and resource optimization are enabled. R8 mapping and seed
files under `app/build/outputs/mapping/release` are generated artifacts; retain
them privately with the exact release version and never commit them.

The artifact validator checks package/version/target SDK, non-debuggable state,
the reviewed merged permission set, credential/signing-file absence, and
absence of embedded bearer/enrollment/private-key material. It must be rerun
against the exact APK that is signed or uploaded.

## External signing

Signing values are read from Gradle properties first and environment variables
second. The supported names are:

```text
synveilReleaseStoreFile       / SYNVEIL_RELEASE_STORE_FILE
synveilReleaseStorePassword  / SYNVEIL_RELEASE_STORE_PASSWORD
synveilReleaseKeyAlias       / SYNVEIL_RELEASE_KEY_ALIAS
synveilReleaseKeyPassword    / SYNVEIL_RELEASE_KEY_PASSWORD
```

All four values are required together. For a local disposable API 36 smoke,
create a temporary keystore outside the repository, export the four values in
the shell, and build `assembleRelease`; never reuse or commit that key as the
production signing identity. CI keeps the production keystore in protected
secrets and should upload the signed artifact and matching private mapping
files with restricted retention.

## Runtime gate

Install the signed release artifact on the dedicated API 36 device, then run
the primary onboarding/profile/library/settings flows, force-stop, relaunch,
and inspect the UI hierarchy and app-filtered logcat. Release profiles must
reject cleartext and loopback origins: numeric loopback HTTP is a debug-only
test policy and `BuildConfig.DEBUG` is false in release. No browser cookies or
CSRF values are accepted by DeviceBearer requests.

The release runtime does not require a live owner-server fixture for startup,
manifest, cleartext, lifecycle, or credential-redaction checks. Authenticated
enrollment, transfer, and owner/web conflict review smoke still requires a
deterministic server fixture and must not be replaced by invented server
behavior.

## CI, rollout, and rollback

The Android workflow always runs wrapper validation, clean debug/release host
builds, JVM tests, lint, and artifact inspection. API 36 instrumentation is a
manual workflow-dispatch opt-in named `run_api36`; its result is reported in a
separate emulator job so host gates are never hidden by emulator availability.

Before distribution, retain the signed artifact, version metadata, R8 mapping,
and validation output together. Use a staged rollout with a pause/rollback
decision window and keep the last known-good signed artifact. Rollback means
serving that prior application version; it does not claim to reverse server or
Room data migrations. A forward client fix is required when data has already
advanced. Store publication and merge approval remain explicit owner actions.

## Product boundaries

Conflict inspection and manual resolution remain BrowserSession/CSRF-only on
the current server contract. Android may show a local conflict summary and an
owner/web-review-required state, but it never sends a fabricated DeviceBearer
resolution request or chooses a winner. Server-side revocation and
re-enrollment remain in the trusted owner/browser workflow.

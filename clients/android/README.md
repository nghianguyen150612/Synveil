# Synveil Android client

## Status

Prompt 5 adds authenticated DeviceBearer library discovery on top of the
verified Prompt 4 enrollment vault. The app can run the canonical health
probes, enroll a device, derive a profile-bound authenticated context, and
list the owner's libraries. Synchronization, uploads, and file browsing remain
future milestones.

## Stack and targets

- Kotlin with Gradle Kotlin DSL and one `app` module.
- Jetpack Compose with Material 3, AndroidX lifecycle, and Navigation Compose.
- AndroidX Preferences DataStore with a versioned kotlinx.serialization JSON
  representation for small local application configuration.
- `compileSdk = 36` and `targetSdk = 36` for the Android 16 baseline.
- Samsung One UI 8.5 is a compatibility target, not a proprietary dependency.
- Pre-release namespace and application ID are both `com.synveil.android`.

Production publishing identity and signing are intentionally undecided. No
signing material, credentials, or server secrets belong in this project.

## Layout and architecture

```text
app/src/main/java/com/synveil/android/
├── app/                 # Activity and navigation composition root
├── core/model/          # Small app-facing value models
├── core/ui/             # Theme and shared Compose UI foundation
├── data/network/        # Profile-bound HTTP transport and auth protocol
├── data/library/        # Strict library catalog wire/domain models
├── data/session/        # Profile-bound DeviceBearer session coordinator
├── data/profile/        # DataStore-backed profile repository and schema
└── feature/
    ├── home/            # Startup/home surface
    ├── profile/         # Profile list, editor, and lifecycle-safe ViewModels
    └── startup/         # Lifecycle-safe presentation state and ViewModel
```

The intended dependency direction is Compose UI -> presentation state ->
future domain/use-case boundaries -> repository -> data adapter. Compose code
does not access DataStore directly and has no HTTP, credential, database,
protocol serialization, or filesystem responsibilities.

### Server profiles

Each profile stores an opaque locally generated UUIDv7, a human-readable label,
canonical server origin, transport policy, creation time, and a nullable future
connection timestamp. Profile IDs are unrelated to hostnames, owners, Devices,
or server database IDs and remain unchanged when a profile is edited. Changing
the origin is an explicit origin reconfiguration and clears any future
`lastConnectedAt` value.

Production profiles require HTTPS. The only HTTP exception is the explicit
`LOOPBACK_TEST_HTTP` policy enabled in debug builds for numeric `127.0.0.1` or
`[::1]` origins. Hostnames such as `localhost`, LAN addresses, and `.local`
names remain rejected. Origins cannot contain credentials, paths, queries,
fragments, backslashes, or malformed ports. Release cleartext traffic is
disabled; the debug manifest permits platform cleartext only so the transport
can exercise its tightly bounded numeric-loopback policy.

The DataStore representation is schema version 1 and contains only the active
profile ID plus non-secret profile records. It contains no password, session,
CSRF, enrollment token, bearer, API-key, or private-key fields. Corrupt or invalid
persisted values fail closed as a typed configuration error rather than being
silently repaired. Duplicate canonical origins and invalid active-profile
references are rejected atomically.

## Device enrollment and credential lifecycle

Device enrollment is implemented for a configured server profile through
`POST /api/v1/device-enrollment/exchange`. The enrollment token is accepted only
when it matches `sve1_` followed by 64 lowercase hexadecimal characters. The
exchange is once-only: OkHttp redirects and connection retries are disabled and
the client never automatically retries a timeout, disconnect, response loss, or
HTTP 503. An ambiguous result is surfaced as recovery required; the token is
never persisted or replayed automatically.

Before sending the exchange, the client preflights Android Keystore. The
returned `svd1_` credential is strictly validated, encrypted with AES-256-GCM
using an AES key that remains in Android Keystore, and stored with a fresh
12-byte IV. The encrypted versioned envelope is authenticated with profile,
origin, transport, owner, device, and credential scope. Only ciphertext, IV,
key alias, and non-secret scope metadata are kept in Preferences DataStore; the
plaintext bearer is never persisted, logged, included in navigation arguments,
or exposed by `toString`.

Enrollment finalization writes non-secret pending metadata, stores the secret,
reads it back and verifies its scope, then commits active metadata and clears
the pending marker. Recovery is evaluated when enrollment/authenticated
functionality is opened; process startup does not decrypt every profile's
credential merely to render Home.
Origin changes and profile deletion fence local credentials before committing,
and remain blocked if secure cleanup fails. “Forget on this device” removes the
local credential only and does not claim server revocation. Server-side revoke
and re-enrollment remain an owner/browser workflow requiring BrowserSession and
CSRF; DeviceBearer is never used for those endpoints.

`MainActivity` enables edge-to-edge and applies safe drawing insets rather than
hardcoding system bar sizes. Navigation Compose owns the initial route and
ordinary back-stack behavior; its AndroidX implementation remains compatible
with modern predictive-back dispatch. The theme follows system light/dark mode,
supports dynamic colors on supported Android versions, and uses standard Android
resources so it remains portable across Android devices and One UI.

## Prerequisites

- JDK 21.
- Android SDK Platform 36 and Build Tools 36.0.0.
- A working Android SDK license setup.

No Android Studio installation is required for the command-line build.

## Build and test

From this directory:

```bash
./gradlew --version
./gradlew tasks
./gradlew assembleDebug
./gradlew test
./gradlew lint
./gradlew connectedDebugAndroidTest
```

The debug APK is a generated local artifact and is not committed. No running
Synveil server or secret environment variable is required.

## Security and protocol boundary

The Android client is an untrusted/semi-trusted client. It must use reviewed
Synveil HTTP/API and authentication boundaries when those features are added.
It must never connect directly to PostgreSQL, the server filesystem, object
store paths, or private server credentials. `api/openapi.yaml` and normative
Synveil documentation remain authoritative; this client must not invent a
parallel protocol or duplicate server/domain business logic.

The existing Rust `synveil-client` and `synveil-client-sync` crates implement
desktop process/synchronization concerns and are not linked into Prompt 4.
JNI, UniFFI, native Rust libraries, and C/C++ bridges require a later explicit
portability and FFI decision.

## HTTP transport

`SynveilHttpTransport` is constructed from one validated `ServerProfile`; it
does not accept arbitrary caller URLs. It uses OkHttp with redirects,
connection retries, and cookies disabled. Requests use finite connect, read,
write, and call timeouts, `Accept: application/json`, identity encoding, and a
non-secret client user-agent. Release profiles are HTTPS-only and use standard
Android TLS verification with no trust-all or certificate-error bypass. Debug
numeric-loopback HTTP remains the only cleartext exception.

Health response bodies are bounded to 64 KiB, require compatible JSON content
types, and use strict kotlinx.serialization schemas. Valid `X-Request-Id`
values are retained only as bounded diagnostics. The full server check calls
`/health/live` before `/health/ready`; a valid readiness `503` is reported as
alive-but-not-ready. A successful ready result is the only event that updates
the profile's historical `lastConnectedAt` value. No current-online state is
inferred from that timestamp.

`AuthenticatedSynveilTransport` is created only by `DeviceSessionManager` after
the active profile's non-secret enrollment metadata and Android Keystore vault
record agree on profile, origin, transport, owner, Device, and credential
scope. It sends exactly one `Authorization: Bearer svd1_...` header on
profile-derived `/api/v1/libraries` requests. The bearer is never placed in a
Compose state object, navigation argument, DataStore value, log, cookie, CSRF
header, or process-wide OkHttp default. Kotlin/JVM strings cannot guarantee
zeroization, so the implementation limits secret copies and lifetime instead
of making a false zeroization claim.

Library pages use a strict kotlinx.serialization schema, a 1 MiB response
limit, a page size of 100, a 512-character opaque cursor bound, and finite
budgets of 64 pages and 4096 accumulated libraries. Unknown fields, statuses,
IDs, revisions, timestamps, names, and incoherent pagination fail closed.
Authenticated error states distinguish authentication failure, device
revocation, transient server unavailability, TLS failure, secure-store
failure, recovery-required enrollment, and protocol incompatibility. A 401 or
503 never deletes local enrollment and is never automatically retried or
re-enrolled.

The application-level device session means the currently usable enrolled
profile context; it is not a browser login session. Browser cookies and CSRF
are intentionally unsupported for this feature. The Libraries screen is
foreground-only and has an explicit refresh action. It displays library name
and status metadata only; node and file browsing are not implemented.

## Unsupported features and next milestone

Sync, uploads, backups, node/file browsing, background work, and production
signing remain unsupported. Prompt 4 includes JVM transport/lifecycle tests and a real
AndroidKeyStore instrumentation test for encrypted persistence, recreation,
scope fencing, malformed ciphertext, missing keys, and deletion. Runtime smoke
testing covers build/install/launch, enrollment navigation, secure token input,
local malformed-token validation, and the authenticated library navigation
shell; no external production grant or real credential is used.

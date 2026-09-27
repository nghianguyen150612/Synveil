# Synveil Android client

## Status

Prompt 2 adds durable local server-profile configuration on top of the native
Android foundation. The app can create, edit, select, and remove non-secret
server profiles without making network requests. Authentication, transport,
and synchronization remain future milestones.

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
fragments, backslashes, or malformed ports. No TLS bypass or cleartext network
policy is configured by Prompt 2.

The DataStore representation is schema version 1 and contains only the active
profile ID plus non-secret profile records. It contains no password, session,
CSRF, enrollment, bearer, API-key, or private-key fields. Corrupt or invalid
persisted values fail closed as a typed configuration error rather than being
silently repaired. Duplicate canonical origins and invalid active-profile
references are rejected atomically.

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
desktop process/synchronization concerns and are not linked into Prompt 1.
JNI, UniFFI, native Rust libraries, and C/C++ bridges require a later explicit
portability and FFI decision.

## Unsupported features and next milestone

Authentication, server enrollment, networking, health probes, sync, uploads,
backups, file browsing, background work, and production signing are not
implemented. The next Android milestone should add a reviewed HTTPS transport
that consumes the canonical origin produced here without adding credential
storage or API behavior prematurely.

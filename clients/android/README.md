# Synveil Android client

## Status

Prompt 1 establishes the native Android foundation only. The current build is a
real Compose/Material 3 application shell that identifies itself as Synveil and
reports which product capabilities are not implemented yet.

## Stack and targets

- Kotlin with Gradle Kotlin DSL and one `app` module.
- Jetpack Compose with Material 3, AndroidX lifecycle, and Navigation Compose.
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
└── feature/
    ├── home/            # Minimal startup/home surface
    └── startup/         # Lifecycle-safe presentation state and ViewModel
```

The intended dependency direction is Compose UI -> presentation state ->
future domain/use-case boundaries -> future repository/data adapters. Prompt 1
does not add a repository or persistence adapter because no network, database,
credential, or filesystem behavior is implemented yet. Compose code therefore
has no direct HTTP, storage, protocol serialization, or filesystem access.

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

Authentication, server profiles, networking, sync, uploads, backups, file
browsing, local persistence, background work, and production signing are not
implemented. The next Android milestone should review the server device/profile
and authentication protocol before adding a narrow client boundary and its
deterministic tests.

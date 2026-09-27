# Synveil clients

The native Android client now lives under `clients/android/`. Prompt 1 provides
the Kotlin, Jetpack Compose, Material 3, Android 16, and One UI 8.5-compatible
application foundation only. It consumes no server or storage credentials and
does not yet implement authentication, networking, synchronization, uploads,
backups, or file browsing.

Client implementations must consume reviewed API and protocol contracts without
direct access to PostgreSQL, the server filesystem, object-store paths, or
private server credentials. See `clients/android/README.md` for the Android
architecture, command-line gates, and current limitations.

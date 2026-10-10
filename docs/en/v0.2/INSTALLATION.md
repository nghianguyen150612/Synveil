# Install Synveil v0.2

> **Preview only. v0.2 has not been released.** v0.1.0 is the current released product. For an installation today, use the [v0.1 operations guide](../RELEASE_OPERATIONS.md) and [v0.1 package policy](../RELEASE_PACKAGING.md).

The steps below describe the planned v0.2 routes. Use them only after an official release page publishes the exact artifact and its authenticated verification instructions. Do not install CI artifacts, draft-release files, or a download that has no matching trusted verification instructions. No v0.2 public download channel or production signing key is published yet.

## Before you start

The v0.2 desktop targets x86_64. The named Linux qualification candidates are Ubuntu 24.04 x86_64 and Fedora 42 x86_64; P045 native acceptance is still open, so neither is qualified yet. Debian and its derivatives are not qualified just because they can install DEB packages. Other RPM distributions are not covered by Fedora's target. No exact Windows version or generic Linux compatibility baseline is approved for release claims yet. See [distribution readiness](DISTRIBUTION_READINESS.md).

If the official release page does not list your exact operating-system version and architecture, stop and use a listed environment. Do not assume ARM64/aarch64, 32-bit systems, macOS, iOS, Android, or every Linux distribution is supported.

## Windows

1. On a listed x86_64 Windows version, get `SynveilSetup.exe` only from the official release entry linked by Synveil's published release documentation.
2. Authenticate the download using the verification instructions published with that release. The filename alone does not establish authenticity. Stop if verification material is missing or does not match.
3. Start graphical Setup while signed in to your normal Windows account. The intended installation is per-user and does not require administrator elevation.
4. Read and accept the displayed terms, then review the available choices: starting Synveil when you sign in, creating a desktop shortcut, and opening Synveil after Setup. Keep or change each choice to match your preference. Setup selects the per-user location; changing installation scope or forcing a machine-wide install is unsupported.
5. Select Install and wait for Setup to finish. If you selected Open after installation, Synveil starts; otherwise open it from the Start menu or the optional desktop shortcut.
6. Continue with [first run](FIRST_RUN.md). Installation finishing means the app can launch; it does not mean a server is configured or files have synchronized.

The Windows v0.2 target is x86_64 `SynveilSetup.exe`. Windows ARM64 and other architectures are outside the v0.2 matrix. Native Windows qualification remains incomplete.

## Ubuntu and Debian-family systems

1. Use a DEB only when the official release page lists your exact OS version and architecture. The producer's naming pattern is `synveil_<version>_amd64.deb`; `<version>` is supplied by the release, not guessed.
2. Authenticate the package with that release's trusted manifest/signature instructions before opening it. Do not rely on a checksum hosted beside an unauthenticated package.
3. On a qualified desktop, open the DEB in the operating system's graphical package application, select Install, and approve the visible package-manager authorization prompt with an account allowed to install software.
4. After the package application reports success, open Synveil from the Applications menu as your signed-in user. Do not launch the desktop as root.
5. Continue with [first run](FIRST_RUN.md).

Ubuntu 24.04 x86_64 is a named qualification candidate, not a passed qualification. Debian itself and other Debian-family versions or derivatives have no v0.2 qualification claim. If your graphical package application is missing, the OS version is not listed, or installation reports an unsupported system, stop; do not substitute an unreviewed terminal command.

## Fedora and RPM

1. Use an RPM only when the official release page lists the exact Fedora version and architecture. The producer's naming pattern is `synveil-<version>-1.x86_64.rpm`; use the exact filename published for the release.
2. Authenticate the package through the release's trusted manifest/signature instructions before opening it.
3. Open the RPM in Fedora's graphical software application, choose Install, and approve its visible authorization prompt.
4. When the package application reports success, launch Synveil from the Applications menu as your signed-in user, then follow [first run](FIRST_RUN.md).

Fedora 42 x86_64 is a named qualification candidate, not a passed qualification. RPM format does not qualify every RPM-based distribution or version.

## Generic Linux AppImage

1. Use an AppImage only if the official release page lists your exact Linux compatibility baseline and x86_64 architecture. The producer's filename pattern is `Synveil-<version>-x86_64.AppImage`.
2. Authenticate the exact file using the release's trusted manifest/signature instructions. Stop if the file does not verify.
3. In the file manager, use the operating system's documented Trust and Run or executable permission control when the release instructions identify one. Then open the file as your normal user. A terminal permission workaround is not the qualified graphical route.
4. Continue with [first run](FIRST_RUN.md). If the file manager cannot provide the documented launch action, treat that environment as unsupported and seek help.

The v0.2 AppImage target is x86_64 only. Compatibility depends on the published kernel, glibc, desktop, DBus, Secret Service, and FUSE requirements. No broad generic-Linux baseline is qualified yet. See [Advanced installation](ADVANCED_INSTALLATION.md) for the runtime boundary.

## After installation

The next step is the in-app [first-run guide](FIRST_RUN.md). For an install problem, use [troubleshooting](TROUBLESHOOTING.md). Do not configure PostgreSQL manually to make an ordinary Host setup work; the managed Host journey is not yet production-qualified.

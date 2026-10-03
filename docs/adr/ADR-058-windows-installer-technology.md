# ADR-058: Use Inno Setup for the per-user Windows installer

## Status

Accepted for Synveil v0.2 (Prompt021).

## Context

Synveil already builds an x86_64 Windows runtime closure containing separate
Qt desktop and background-client executables. Credentials, profiles, sync state
and local control belong to the runtime; login startup belongs to its explicit
current-user Task Scheduler manager. Phase D needs a concise
`SynveilSetup.exe`, no-admin normal installation, repair/upgrade/uninstall and
native unattended acceptance without introducing a service or owning state.

WiX MSI/Burn, NSIS, MSIX, Inno Setup and a custom Rust bootstrapper were scored
against the same 24 product, lifecycle, security, CI and maintenance criteria
in the [decision record](../v0.2/WINDOWS_INSTALLER_TECHNOLOGY_DECISION.md).

## Decision

Use pinned **Inno Setup 6.7.3** for P022–P028. Build a single x86_64,
per-user, non-elevating `SynveilSetup.exe` whose default destination is
`{localappdata}\Programs\Synveil`. Keep one stable AppId and HKCU uninstall
registration. Use standard native wizard controls for Welcome, one bounded
terms/options page, progress and Finish.

Inno owns package files, shortcuts and uninstall registration. It consumes the
verified Windows runtime closure and verifies installed payload before success.
It neither links the common Rust engine nor duplicates P007–P010 policy. Thin
Windows hooks map native effects to that policy.

The existing client manager remains the sole Task Scheduler authority. Setup
may collect the explicit startup choice and hand it to a client-owned helper;
because a fresh install has no profile, first-run applies the preference after
one exists. Setup may not author a task, preference file, service or Run key.
Repair and upgrade preserve explicit disable. Ordinary uninstall removes
package-owned effects and requests removal of the product task but preserves
all configuration, credentials, sync state, libraries and server data.

Tool acquisition is immutable URL plus reviewed SHA-256, builds run through
native `ISCC.exe`, silent lifecycle commands use fixed logs and `/NORESTART`,
and P028 validates the installation as the ordinary runner account. Release
signing is a later external Authenticode step integrated through Inno SignTool;
unsigned CI remains explicitly unsigned.

## Consequences

The project gets an accessible native EXE wizard, declarative per-user
integration, automatic uninstall logging, stable noninteractive switches and a
small repository surface. Same-version repair is a verified reinstall rather
than MSI component repair. Any Pascal code must remain narrow and reviewed.
Byte reproducibility, interrupted mutation, locked files, signing and complete
native lifecycle behavior remain implementation/acceptance gates; this ADR is
not evidence that the installer exists.

Adding per-machine installation, MSIX, Burn/MSI or a second startup authority
requires a superseding ADR. P022 may not re-open the technology choice merely
to begin implementation.

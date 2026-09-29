# Linux package integration reconciliation

## Authority and engine boundary

DEB and RPM are native transactions: dpkg/APT and RPM/DNF exclusively own
dependency resolution, package database locks, payload replacement, obsolete
file removal, and native status. Synveil does not delete locks, repair a native
database, or emulate those managers. A busy manager is a safe, explicit
pre-mutation failure; a terminated transaction whose result cannot be observed
is `OutcomeUnknown`, never success.

The P007 engine remains the lifecycle coordinator. A native payload effect has
`NATIVE_PACKAGE_MANAGER` authority and `NATIVE_PACKAGE_STATE` or
`PACKAGE_OWNED` ownership. Synveil-specific verification/reconciliation has
`PLATFORM_INTEGRATION_ADAPTER` authority and
`PLATFORM_INTEGRATION_OWNED` ownership. Every operation requires final
verification; there is no command string or generic shell effect in the model.
The schema-1 machine-readable source is
`deploy/linux/package-integration-v1.json`; unknown versions and fields fail
closed.

## Owned and preserved surfaces

The package-neutral manifest supplies the actual payload. Every shipped file is
`PACKAGE_OWNED` and mutated only by the native manager: this includes the three
`/usr/bin/synveil-*` executables, immutable documentation/template files, the
desktop entry and icon, static user client unit, static administrator
maintenance units, sysusers, and tmpfiles definitions. Their semantic purpose
is platform integration, but that does not change their package-database file
ownership. `PLATFORM_INTEGRATION_OWNED` is reserved for non-payload semantic
effects such as scoped post-package reconciliation; none is invented merely to
populate that class. Installing a unit is not enabling or starting it.

Packages do **not** own application configuration, the XDG user profile, library content, client sync
metadata, SecretStore credentials, `/etc/synveil` administrator configuration
or credentials, `/var/lib/synveil` server state, databases, object data, or
external dependencies. Ordinary remove, DEB purge, RPM erase, upgrade, and
repair preserve those resources. Destructive cleanup remains a separate P009
authorization and is not implemented here.

Package hooks run as root and must not infer an interactive user from `HOME`,
`USER`, or `LOGNAME`, edit home directories, use a graphical session, download
code, or start the desktop/client. User-scoped integration must later run in an
explicit user context. Static hook validation rejects network fetches,
recursive deletion, home mutation, `sh -c`, and automatic enablement.
The contract also rejects unsafe lexical paths, a symlinked/non-directory root,
and symlink ancestors before inspecting a staged destination.

## Lifecycle mapping

| Intent | Native manager | Synveil integration | Preservation |
|---|---|---|---|
| Install | install declared payload/dependencies | verify static assets | all durable state |
| Upgrade | replace/remove package files | require P009 compatibility first; verify changed assets | all durable state |
| Repair | reinstall/reconcile package payload | verify missing/corrupt integration | all durable state |
| Uninstall | remove package-owned payload | remove package integration; stop only owned system runtime | all durable state |
| Verify | report native status | verify required assets without mutation | all durable state |

DEB remains package `synveil`, uses the existing architecture mapping and
declared Debian runtime libraries, and has minimal `postinst`, `prerm`, and
`postrm` hooks. RPM has the same identity and semantics with its existing
architecture mapping, requirements, and `%post`/`%preun`/`%postun` hooks. Both
install `/usr/bin/synveil-desktop`, application ID `synveil.desktop`, preserve
state, and prohibit automatic startup.

Schema 1 closes each format's metadata to the ordered architecture mappings,
hook sets, and exact runtime dependencies declared by `arch.sh`, `build.sh`
(`DEB_DEPENDS`), and the RPM spec (`Requires`). The focused drift gate compares
those sources directly. `arm64`/`aarch64` is declarative architecture support,
not native acceptance evidence.

## Phase-B reconciliation

P008 journals Synveil-owned mutable integration effects before mutation and
requires inspection before replay after interruption. It does not replace
dpkg/rpm databases, locks, or recovery. P009 remains compatibility,
repair/uninstall, preservation, and destructive-cleanup authority. P010's
existing pre-mutation, known-partial, unknown-outcome, verification, privilege,
and redacted-diagnostic categories apply without a new error taxonomy.

P005 remains artifact identity/version/source authority. P006 verifies exact
download bytes and P011 selects an authenticated compatible release. This
contract starts only after a trusted native artifact is selected; it adds no
origin, download, updater, or quick-install behavior.

## Evidence and deferrals

Evidence is deterministic contract parsing, unit planning/ownership tests, and
static package-source/hook inspection. The existing separate native-package CI
continues to build and inspect unsigned DEB/RPM fixtures. This is not native
clean-machine or release-acceptance evidence. P013 Debian UX, P014 RPM UX,
P015–P016 AppImage, P017 quick install, P018 distro detection, P019 first
launch, and P020 clean-machine acceptance remain pending.

The hosted native-package workflow currently fails its shell-syntax step on the
pre-existing, untouched `scripts/verify-release-build-reproducibility.sh:58`
ShellCheck `SC2155` finding. That failure is reported separately and is not
masked by the passing Prompt012 focused gate.


## Contract drift closure

Schema v1 is intentionally exact for the current native package surface. In addition to format-specific architecture, dependency, and hook parity, the Rust contract pins the current product/binary/desktop identities and the complete ordered package-shipped surface set. CI compares PACKAGE/TEMPLATE records from the package-neutral MANIFEST in both directions, so newly added or omitted native package payload must be reconciled explicitly.

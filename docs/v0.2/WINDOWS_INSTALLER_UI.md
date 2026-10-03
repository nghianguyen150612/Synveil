# Windows installer UI

## Ordinary flow

Prompt023 keeps the Inno Setup 6.7.3 technology and presents exactly four
ordinary pages: **Welcome → Terms/options → Installing → Finish**. Destination,
program-group, component, task, and ready pages are disabled. Welcome explains
that setup is for the current Windows account. The standard Inno license page
displays the repository MIT `LICENSE` and retains its required acceptance
gate; three native check boxes and a local third-party-notice button share that
page. Installation uses Inno's native progress and rollback behavior, and the
standard successful Finish page is not reached before package mutation ends.

The controls have stable names, native focus and keyboard behavior, explicit
tab order, DPI-scaled positions, and a mnemonic on the notice button. They do
not convey information by color or require pointer input. P028 still owns
screen-reader, scaling, screenshot, and full native interaction acceptance.

## Option state

`StartupRequested`, `DesktopIconRequested`, and
`LaunchRequested` are independent booleans. A fresh interactive run
selects all three. `/STARTUP=0|1`, `/DESKTOPICON=0|1`, and `/LAUNCH=0|1` feed
the same state; silent mode defaults all absent choices to disabled, and any
present value other than exactly `0` or `1` aborts before mutation.

The desktop choice creates one `{userdesktop}\Synveil` shortcut to the absolute
installed desktop executable with `{app}` as its working directory. The launch
choice is the sole post-install launch authority and is explicitly disabled in
silent mode. The current-user Start Menu shortcut remains unconditional and
its folder chooser remains hidden.

`StartupRequested` is an intentionally non-mutating handoff boundary. P023
parses and retains the choice but creates no task, service, Run key, or other
persistent startup integration. P026 exclusively owns that integration. Full
upgrade/repair preference persistence remains P027 scope; P023 does not invent
a competing persistence store.

## Security and build behavior

The stable AppId, `{localappdata}\Programs\Synveil` location, `lowest`
privilege, and disabled privilege override are unchanged. License and notice
content is local; setup does not fetch UI content. Cancellation and package
rollback remain native Inno operations and do not delete Synveil application
state.

Windows reproducible builds transport Cargo flags through
`CARGO_ENCODED_RUSTFLAGS`. Each remap remains one argument even when a native
path contains a drive colon, backslashes, or spaces. The direct rustc call used
to build the Qt tool wrapper receives the same remaps as a Bash array, while
the authenticated MSVC linker remains an explicit separate argument.

## Evidence boundary

The network-free validator enforces topology, options, authority exclusions,
identity, privilege, and location. A dedicated regression test exercises
representative Windows paths for Cargo and direct-rustc argument boundaries.
The focused hosted workflow retains its real runtime, two-build comparison,
silent all-zero install, installed-product smoke, uninstall, and preservation
checks. Native interactive screenshots and accessibility evidence are not
available from the local Linux environment and are not claimed as passing.

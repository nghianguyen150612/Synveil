# Prompt023 manifest — Windows installer UI

| Item | Value |
|---|---|
| starting main SHA | `9d7e926b7b38f0362dfd4761e152d060724d02ea` |
| branch | `codex/p023-windows-installer-ui` |
| inherited failure | run `37098846470`: MSYS split/rewrote native Windows `RUSTFLAGS` remaps during runtime build |
| fix | Cargo unit-separator encoded flags plus array-based direct-rustc remaps |
| UI source | `deploy/windows/installer/Synveil.iss` |
| topology | Welcome → combined MIT terms/options → Installing → Finish |
| fresh interactive defaults | STARTUP=1, DESKTOPICON=1, LAUNCH=1 |
| silent defaults | fail closed to 0; hosted smoke passes explicit all-zero values |
| identity | stable AppId `{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}` |
| location / privilege | `{localappdata}\Programs\Synveil`; lowest; no override |

## Behavior and ownership

The standard required-acceptance license page reads the current repository MIT
license. It also contains three independently keyboard-operable native check
boxes and a button opening the packaged local `NOTICE`. Destination, Start Menu
folder, Components, Tasks, and Ready pages are absent from the ordinary path.

DESKTOPICON controls exactly one current-user desktop shortcut whose target is
`{app}\synveil-desktop.exe` and working directory is `{app}`. LAUNCH controls
the sole post-install direct execution boundary; that action is always skipped
in silent mode and does not add a second Finish-page checkbox. STARTUP is parsed and retained only. It deliberately has no persistent
effect until P026 provides the reviewed Task Scheduler handoff. Complete
preference preservation across upgrade and repair remains P027 work.

Native controls, explicit tab order, DPI scaling, standard focus behavior, and
concise labels form the P023 accessibility approach. P028 retains full native
automation, screen-reader/scaling qualification, and screenshot evidence.

## Evidence and limitations

Local validation covers installer source invariants, malformed Boolean
rejection by construction, encoded Windows remaps containing spaces/drive
letters/backslashes, the direct Qt-wrapper rustc argv boundary, Python/Bash
syntax, Rust workspace checks, release-manifest tests, and diff hygiene. This
Linux environment cannot compile Inno Setup or execute/capture its native UI.
The workspace check was attempted but is locally blocked by the missing
system `dbus-1` development package; it is not recorded as PASS.

No final-head hosted result exists at authoring time. Consequently runtime
build, staging, Inno verification, Setup bytes, manifest, silent lifecycle,
interactive pages, keyboard traversal, and screenshots remain **pending hosted
Windows execution**, not PASS. Installer byte reproducibility continues to be
tested as exact equality; no normalization or fabricated equality was added.

The Setup artifact identity remains `windows-x86_64-installer` /
`windows_installer` / `SynveilSetup.exe` / `primary_installer`. Signing remains
deferred. P024 runtime maturity, P025 per-user qualification, P026 startup,
P027 lifecycle completeness, and P028 final native acceptance are not claimed.

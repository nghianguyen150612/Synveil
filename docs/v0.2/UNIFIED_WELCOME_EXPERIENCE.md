# Unified Welcome experience

## Purpose and product language

The native first screen asks “Choose how you want to get started” and gives two
equivalent actions: **Host Synveil** and **Connect to Synveil**. Welcome contains
no server-address, credential, database, service, or client-folder input.
Rendering it has no setup side effect.

## Authoritative routing

Rust composes the initialized client snapshot (`profile_configured`) with the
P036 `HostSetupState`. A fresh client with no Host intent sees Welcome. Any
configured profile enters the existing desktop, including signed-out and
authenticated/zero-library states. Partial Host stages resume; `Ready` shows
hosted status without provisioning; `NeedsRepair` shows basic attention; and an
unsupported or failed inspection leaves Welcome and Connect usable. There is no
durable Welcome-completed flag. Restart and GUI/tray reopen reconstruct from
the owners rather than a QML page index.

Host storage is **server data**; it is never described as the local client
library or synchronized folder that P040 will select.

## Intent boundaries

Host activation is bounded and calls P036 `begin_host()` exactly at explicit
user intent. QML performs no machine operation. This stacked baseline has no
production desktop `CanonicalOwners` adapter, so it truthfully renders Hosting
as unavailable and never fakes Ready. Connect only reveals the existing
profile form; configuration remains QML → `DesktopUiBridge` →
`DesktopController` → local IPC → `synveil-client`, retaining canonical HTTPS,
TLS, credential fencing, and unknown-outcome behavior. Back before mutation
returns to Welcome.

## Presentation, sequencing, and accessibility

`Main.qml` remains the RCC-bundled entrypoint. Welcome, bounded initializing,
Host status, and existing content are mutually routed surfaces. The user-level
sign-in startup preference remains owned as before but cannot cover Welcome.
Both full-width native Buttons have stable object names, accessible names and
descriptions, keyboard activation, visible native focus, logical tab order,
and a non-color Host availability message. The layout is bounded at the
existing 860×560 minimum.

## Scope, evidence, and stacked dependency

Presentation unit tests cover fresh/configured/partial/Ready/repair/blocked
routing and configured signed-out/zero-library behavior. The network-free
validator checks labels, accessibility, owner delegation, and forbidden state.
Offscreen Qt evidence is limited to available CI tooling and does not qualify
P036 native artifacts, platforms, restart, or power-loss behavior. P038 owns
connection simplification, P039 authentication polish, P040 local-library
onboarding, P041 overall progress, and P042 recovery polish.

This work is stacked on P036 PR #62 at
`14e0c2800f2093b8d55062c9979b0782f4f51d65`. Its Phase-E status remains
`BLOCKED_BY_ENVIRONMENT_AND_PRODUCTION_ARTIFACTS`. After P036 merges, this branch
must be rebased onto that merged main, adapted to the final coordinator API,
revalidated, and retargeted to main. P038–P042 retain connection simplification,
authentication polish, local-library onboarding, overall progress, and repair.

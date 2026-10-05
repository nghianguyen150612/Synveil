# Prompt037 manifest — unified Welcome experience

- Stack parent: `codex/implement-end-to-end-self-host-wizard`.
- Stack starting SHA / P036 PR #62 head: `14e0c2800f2093b8d55062c9979b0782f4f51d65`.
- Main SHA at branch creation: `f649d1dc5088f28a7602722008612b11ac4e7365`.
- P036 gate: `BLOCKED_BY_ENVIRONMENT_AND_PRODUCTION_ARTIFACTS`; its readiness
  marker remains withheld.

## Source composition

Changed desktop presentation/bridge/QML and dependency metadata; added ADR-067,
the Welcome contract, this manifest, a static validator, docs integration, and
a focused workflow. `WelcomeDestination` composes initialized client profile
configuration with P036 Host state. A configured profile wins and bypasses
Welcome; partial Host stages resume, Ready cannot reprovision, repair receives
basic attention, and unsupported Host preserves Connect.

Host activation alone calls `begin_host()` and the current desktop adapter is
effect-denying because P036 has no qualified production adapter. Connect only
routes to the existing controller-owned form. Welcome display, Connect, and
Back create no Host/profile/auth/library mutation. The startup-choice popup is
deferred until existing client content. `Main.qml` remains the sole bundled
entrypoint; no runtime source-tree QML dependency was added.

Accessibility evidence includes native Button semantics, accessible names and
descriptions, stable object names, focus assignment/order, keyboard submission,
and textual unavailable status. Presentation tests and static validation cover
routing and no-side-effect boundaries. Qt offscreen/build results are recorded
in CI; they are not P036 artifact, VM, reboot, or power-interruption evidence.

P038–P042 respectively retain connection simplification, authentication,
client-library choice, cross-stage progress, and repair. After P036 merges,
rebase/update onto its exact merged main, preserve its final evidence, retarget
the PR to `main`, and rerun Prompt037 gates before merge.

# Prompt036 manifest — end-to-end self-host wizard

- Starting main SHA: `f649d1dc5088f28a7602722008612b11ac4e7365`.
- Branch: `codex/p036-end-to-end-self-host-wizard`.
- Inherited P029--P035 source markers are present. P028 remains separately
  withheld.
- P034 focused hosted result: `NOT_AVAILABLE`; no remote is configured, so an
  exact run/head identity could not be queried. Source head is
  `2e4809f77c788f7cebc276cd2479119347366210` (P035 baseline parent).
- P035 focused hosted result: `NOT_AVAILABLE`; no remote is configured. Head is
  `f649d1dc5088f28a7602722008612b11ac4e7365`.

## Composition delivered

The `synveil-server-bootstrap` crate provides one typed state machine,
generation-fenced reviewed-plan binding, purpose-specific privileged effects,
fresh owner reconciliation, full readiness derivation, and a secret-free Ready
handoff. Stages and ordinary choices follow the product contract. It depends
toward P031--P035 and has no client/sync ownership. ADR-066 locks the P037,
P038/P039, and P040 handoffs.

## Evidence and disposition

Production PG17 artifact identity: **missing**. Production edge artifact
identity: **missing**. Ubuntu native Host: **NOT_AVAILABLE**. Fedora native
Host: **NOT_AVAILABLE**. LocalOnly/PrivateLan native HTTPS, systemd restart and
reboot, repair/reinstall, physical interruption, admin response-loss,
preservation, and clean-machine evidence: **NOT_AVAILABLE**. Component source
tests are not substituted for these results. FIRST-RUN-2 remains
`IMPLEMENTATION_PENDING`.

Phase-E gate result: **BLOCKED_BY_ENVIRONMENT_AND_PRODUCTION_ARTIFACTS**. The
readiness marker is intentionally absent. No secret values are recorded.

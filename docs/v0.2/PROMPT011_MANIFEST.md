# Prompt011 manifest

## Provenance and versions

- Starting accepted main: `6a1743a1113538bc1044300a132368c330d2ae4d` (`feat: add installer error model`).
- Task branch: `prompt011-release-channel-version-selection`.
- Preferred commit/PR/squash subject: `feat: add release channel selection`.
- Channel schema: 1; channel-authentication schema: 1; supported channels: `stable` only.
- Fixed filenames: `SYNVEIL-RELEASE-CHANNEL.json`, `SYNVEIL-RELEASE-CHANNEL-AUTH.json`, and selected `SYNVEIL-RELEASE-MANIFEST.json`.
- Limits: 1,048,576 channel bytes, 256 releases, 256 upgrade sources, generation 1 through 2^63-1.
- Product remains 0.1.0. P005–P010 schema versions, 36 server migrations, 7 client migrations, and `LOCAL_SCHEMA_VERSION = 7` are unchanged.

## Trust and selection

Exact bytes are authenticated by a trusted local SHA-256 pin or a deterministic
locally eligible detached-signature verifier. There is no TOFU. A local minimum
generation rejects bootstrap rollback. Caller-owned `(generation,
channel_sha256)` high-water accepts exact replay and advancement, rejects lower
generation (`CHANNEL_ROLLBACK`), and rejects another digest at the same
generation (`CHANNEL_EQUIVOCATION`).

Outcomes are `SELECTED`, `NO_RELEASE_AVAILABLE`, and
`NO_NEWER_COMPATIBLE_RELEASE`. Internal errors include `INVALID_CHANNEL`,
`UNSUPPORTED_CHANNEL_SCHEMA`, `UNSUPPORTED_CHANNEL`, `CHANNEL_AUTH_FAILED`,
`UNSUPPORTED_CHANNEL_AUTHENTICATION`, `CHANNEL_TOO_LARGE`, `CHANNEL_ROLLBACK`,
`CHANNEL_EQUIVOCATION`, `INVALID_VERSION`, `INVALID_RELEASE_POLICY`,
`SELECTED_RELEASE_DETACHED`, and `MANIFEST_BINDING_MISMATCH`. P006 retains its
network errors (`NETWORK_ERROR`, `UNSAFE_REDIRECT`, `UNTRUSTED_ORIGIN`). Unknown
states fail closed.

Fresh install selects the highest eligible numeric stable version. Upgrade
selects the highest newer target explicitly listing the exact current stable
version. There is no downgrade or repair recommendation. P009 independently
authorizes mutation.

## Evidence and validation

Prompt011 adds 89 focused, network-free Python tests covering schemas,
authentication, signature rotation, binding, rollback/equivocation, numeric
ordering, selection, deterministic construction, and the P006 manifest/artifact
bridge. The retained expected install-engine inventory is 381 tests: P007 81,
P008/P008A 78, P009/P009A 107, P010 Rust 115; P010 Python remains 4. Validation
records are produced by the task workflow; no unrelated CI failure is claimed
or repaired here.

The PR workflow is accepted main → one task commit → PR to `main` → review →
squash merge → live-main verification. This manifest intentionally does not
predict the squash SHA.

## Boundary

This closes the P005–P011 common distribution contracts only. It does not claim
Linux easy installation, Windows Setup, AppImage, server setup, or a v0.2.0 tag
or release. P048 retains final release ownership.


## Post-review corrections

Hosted review hardened three release-critical boundaries before merge:

- channel authentication evidence is re-bound to the supplied local trust policy during parse;
- fresh-install and upgrade selection require explicit high-water input and perform rollback/equivocation evaluation before selection;
- `SelectedRelease` validation rechecks fresh-install/upgrade eligibility, including exact current-version compatibility and no downgrade.

The focused Prompt011 suite now contains 101 tests.

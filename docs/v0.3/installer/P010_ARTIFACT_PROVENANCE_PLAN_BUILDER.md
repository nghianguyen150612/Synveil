# P010 — Artifact Provenance and Plan Builder

**Status: provisional, fail-closed contract increment.** Production artifact
verification and installation plan preview remain unavailable. P010 performs no
acquisition, installation, authorization, journal creation, or other effect.
The exact starting revision is `1ac67076e9e1e8c475b4a69578273d3f777a6ae9`.

## Scope and qualification baseline

The named `Synveil-v0.3-Context-Roadmap-P001-P060.md` was not found in the
repository or workspace. Existing v0.3 security records also call it absent.
This P010 is therefore a provisional scoped increment; it does not replace or
claim authority over an unavailable roadmap.

Remote preflight on 2026-10-27 confirmed P006 #165, formatting repair #167,
P007 #166, P008 #168 and P009 #169 merged. The actual `origin/develop/v0.3`
was the expected base SHA above. `main` was
`604499fce0dfd134d82da3d5bb5d4a055c5eea8d` and was not modified. P009's merged
head has failed exact-head hosted checks: Rust formatting/clippy, native Linux
Qt 6, and Windows `standard-user-native`; the latter failed while recording
required unavailable surfaces honestly. Windows Qt 6 passed. The P009 artifact
identity, standard-user, and native Linux/Windows outcomes are not treated as
qualification passes. P006's joined PostgreSQL metadata-to-ObjectStore owner
and byte integration case remains missing. P028 Windows standard-user native
acceptance remains failed/withheld. P036 production PG17 and edge artifact
identity evidence remains missing; the P036 workflow's clean-machine and Phase
C gates were skipped. No inherited exception or skipped job is converted into
a pass.

P004-D02 (installer privilege), D11 (presentation/effect ownership), and D14
(publisher signing and downgrade policy) remain
`RECOMMENDED_PENDING_APPROVAL`. P010 does not approve them.

## Existing release-artifact system

P010 reuses the existing trust contracts by defining matching typed evidence
categories and preserving their fail-closed meaning. It does not copy the
Python verifier into Rust and does not invoke Python or a shell from the
desktop. The inspected production-capable components are:

- `scripts/release_channel.py`: bounded stable-channel parsing, local pinned
  digest or detached-signature authentication, generation floor, deterministic
  selection, and high-water anti-rollback support.
- `scripts/release_download.py`: authenticated manifest binding, HTTPS origin
  allowlists, redirect checks, bounded metadata/download size, exact size and
  digest checks, private staging, safe path checks, cleanup, and later-use
  evidence revalidation.
- `scripts/release_manifest.py` and
  `deploy/release/release-manifest-v1.schema.json`: strict artifact inventory
  validation, including identity/type/platform/architecture/role/version,
  source revision, size, digest and package metadata. A manifest is inventory;
  it becomes trusted only when its exact bytes are authenticated by the release
  trust contract.
- `scripts/release_signature.py`: Ed25519 verification against a locally
  provisioned public policy whose key ID is derived from public-key bytes.
  `scripts/sign_release_metadata.py` is a release-side signer, not installer
  code, and must never be run by an installer or supplied a private key there.
- `scripts/materialize-release-trust-policy.py` and
  `scripts/materialize-p036-trust-keyring.py`: CI-side materializers for
  externally provisioned public roots. Their inputs are absent from the desktop
  runtime and are not embedded here.
- `scripts/validate-release-artifacts.sh` validates a local generated Linux
  build inventory against its source tree. That inventory and its computed
  hashes are not publisher authentication.
- `deploy/release/windows-signing-policy-v1.json` permits Authenticode but has
  no approved timestamp hosts. It does not establish a publisher certificate
  root. `acceptance/p036/production-artifacts.schema.json` validates inventory
  shape; its schema does not authenticate the listed producer or artifacts.

Production release CI can use detached Ed25519 evidence only when the
independently provisioned public trust policy exists. P036's production
keyring, Windows signing roots, channel pins/high-water state, and an approved
runtime source for this wizard are unavailable. Test-generated keys and
signatures exercise verifier behavior only. No fixture/test key is a production
root.

## Evidence and fail-closed policy

`ArtifactSourceEvidence` distinguishes authenticated-channel observation,
authenticated-manifest observation, digest-matched bytes, package-manager
identity, untrusted local input, missing/unknown input, and invalid/stale/
revoked/unsupported release. These are observations, not capabilities: a
caller cannot promote a report, path, filename, extension, URL, HTTPS
connection, local checksum, adjacent checksum, unsigned CI candidate, or
publisher string to `VerifiedReleaseArtifact`.

`build_installation_plan_preview` accepts only bounded platform/channel/scope
observations and evidence categories; it accepts no effects, commands, local
paths, publisher strings, or trust roots. It rejects ambiguous selection,
missing or untrusted input, invalid releases, unknown platform/architecture,
platform/channel mismatch, and channel/scope mismatch. For otherwise supported
inputs it returns the concrete blockers: production trust root unavailable,
approved effect catalog unavailable, and P004 policy pending. It never creates
`VerifiedArtifactEvidence` or a complete plan. The result is intentionally
unavailable until the authenticated trust source and effect policy are wired.
The existing read-only preflight maps its bounded platform, channel, scope, and
artifact observations into this API and passes the returned blocker categories
to the Wizard view.

The existing trust package covers exact-byte signatures, key/scheme checks,
descriptor binding, version and generation floors, strict manifest validation,
size/digest matching, download staging and use-time revalidation. Production
revocation and downgrade policy are not approved for this desktop integration;
P010 reports that gate as unavailable instead of selecting permissive policy.
There is no new network behavior in P010.

## Platform and channel matrix

The typed matrix recognizes expected platform/channel pairs to reject
substitution. Recognition is not availability. All production channels remain
blocked because the authenticated source and approved effect catalog are not
connected.

| Channel | Expected platform and architecture | Scope | Production preview |
|---|---|---|---|
| Ubuntu 24.04 DEB | Ubuntu 24.04, x86-64 | NativePackage | Blocked: trust root, effect catalog and P004 decisions |
| Fedora 42 RPM | Fedora 42, x86-64 | NativePackage | Blocked: trust root, effect catalog and P004 decisions |
| Linux AppImage | Ubuntu 24.04 or Fedora 42, x86-64 | PortableUser | Blocked; no package-manager adapter mapping |
| Windows per-user Setup | Windows 11, x86-64 | CurrentUser | Blocked; system scope/elevation is rejected |
| Windows portable ZIP | Windows 11, x86-64 | PortableUser | Blocked; portable execution is not native package installation |
| Existing installation | Exact observed platform required | Channel-specific | Blocked pending identity and maintenance catalog |

No platform is implicitly granted Host support. No Linux native package effect
is routed through the AppImage adapter.

## Plan ownership, validation, and P009

The plan boundary accepts no QML-supplied `InstallationEffect`. A future
platform-owned effect catalog must define stable effect identities, owning
adapter/resource class, scope, preconditions, privilege, verification, retry,
reconciliation, preservation, and reversibility. Missing catalog entries block
the channel; partial catalogs cannot produce complete `InstallationPlan`s.

The narrow crate-private `validate_plan_for_review` handoff requires
`VerifiedReleaseArtifact`, calls `InstallerEngine::validate`, and then delegates
to the existing crate-private P009 snapshot constructor. Desktop/QML cannot
construct a snapshot. No current production source reaches this handoff.
Existing P009 fingerprinting binds ordered effects, artifact identity,
channel/platform/architecture context, scope/intent, ownership, privilege,
preservation, findings, policies, and capability generation; local paths and
redacted diagnostic text are excluded. Fingerprints remain change-detection
commitments, not authorization tokens. The P009 generation/cancel/supersession
rules remain the freshness model; any later execution phase must reacquire and
revalidate trust and platform state immediately before effects.

## Wizard behavior and side-effect boundary

The third step continues to show **“Preview only — installation is not
started.”** It now explains that independently authenticated artifact evidence,
the production publisher trust root, and an approved channel effect catalog are
unavailable and names the pending P004 decisions. The acknowledgement remains
disabled; no Install button or effect summary is fabricated. The presentation
does not expose artifact paths, keys, environment variables, raw package output,
OS diagnostics, or commands. P010 does not call `execute`, mutate a journal,
run a package manager, download files, or request OS authorization.

## Test and validation record

Focused tests prove observation categories cannot promote artifacts to trusted
evidence, absent artifacts/local untrusted input/ambiguous selections fail
closed, platform/architecture/channel/scope mismatch is blocked, and a
synthetic-only bounded plan fixture passes the existing engine validator. The
existing P009 tests continue to cover fingerprint changes, stale async result
rejection, cancellation, duplicate acknowledgement, and path redaction. Existing
release-channel, release-manifest, release-signature, release-download, and
installer-security suites remain the source of detailed negative signature,
key substitution, byte mutation, size, path, redirect, and staging tests.

Final exact commands and outcomes are recorded in
[`PROMPT010_MANIFEST.md`](../manifests/PROMPT010_MANIFEST.md). Hosted Linux and
Windows Qt/native acceptance is required for P010 and is not inferred from
P009's merged status. No installation effect or journal mutation is used in
these tests.

## P011 prerequisites

Before a production plan can be shown, P011 must obtain the authoritative v0.3
roadmap or keep the sequence explicitly provisional; approved publisher root,
rotation/revocation and anti-downgrade policy; exact artifact identity and
platform/channel policy; approved per-channel effect catalogs and owners; P004
D02/D11/D14 decisions; a trusted runtime provisioning path; complete P009
freshness wiring from artifact verification through the UI; and successful
exact-head Linux/Windows desktop and relevant native acceptance. P028 and P036
gates must be reported using their actual evidence, not CI fixture artifacts.

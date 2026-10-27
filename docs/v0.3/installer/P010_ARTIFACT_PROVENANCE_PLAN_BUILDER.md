# P010 artifact provenance and plan builder

## Status and scope

P010 is a **provisional scoped increment**. No authoritative v0.3 P001–P060
roadmap was present in the repository or found in the GitHub repository. The
authoritative v0.2 roadmap uses P010 for the installer error model, and the
v0.2 completion inventory records that work as implemented but untested. This
prompt's v0.3 P010 numbering therefore conflicts with the older roadmap; this
document does not replace or renumber it.

The verified source base is `origin/develop/v0.3` at
`1ac67076e9e1e8c475b4a69578273d3f777a6ae9`. The branch's starting worktree was
clean. Main was `604499fce0dfd134d82da3d5bb5d4a055c5eea8d`. PR #169 is merged,
but its hosted qualification was not green: native Linux Qt, source-contract,
desktop/QML, quality, docs/quality and standard-user-native jobs failed. Native
Windows Qt passed. Other checks passed, including scoped release-acquisition,
engine, AppImage, package, and some installer security checks. Failures and
passes are recorded separately; merge is not treated as qualification.

P010 is read-only. It adds no network fetcher, installer adapter, journal
mutation, execution call, native authorization request, or package operation.

## Reused release-artifact infrastructure

The existing `scripts/release_channel.py`, `release_manifest.py`,
`release_signature.py`, and `release_download.py` form the P006 verification
boundary. They authenticate exact channel bytes against a locally established
pin or a locally provisioned Ed25519 public trust policy; bind the selected
release to exact manifest bytes; validate the manifest as an inventory; bind
the selected artifact's identity, role, platform, architecture, size and
digest; and stream bounded bytes into private staging with origin, redirect,
path, symlink, and no-clobber protections. Signatures authenticate metadata;
SHA-256 and size verify byte identity and integrity. A checksum by itself does
not authenticate a publisher.

P010 does not translate the Python validation stack into Rust and does not
invoke signing scripts. P006's public-key policy loader requires the exact
`production-release-verification` purpose and binds key IDs to public-key
bytes. Its detached-signature verifier fails closed for missing keys, unknown
schemes, invalid signatures, and an unavailable crypto backend. The
`materialize-release-trust-policy.py` and `materialize-p036-trust-keyring.py`
scripts require separately provisioned inputs. Those inputs are absent in the
current repository/CI evidence. No embedded test key, fixture signature,
unsigned CI artifact, GitHub URL, filename, HTTPS connection, local path,
publisher string, or locally computed digest is an approved production root.

`release-manifest-v1.schema.json` and the P036 production artifact schema
describe inventories. Neither schema authenticates itself. The P036 production
inventory/signature is absent; its artifact-identity workflow failed while
materializing the approved public keyring. P028's Windows native acceptance
did not establish the complete standard-user/GUI/logon/IPC journey. No
production publisher trust policy or final signed release artifact set is
available.

## Trust and plan boundary

`crates/install-engine/src/artifact_plan.rs` defines bounded evidence classes
for authenticated channel metadata, authenticated manifests, digest-verified
bytes, native package-manager identity, local untrusted bytes, missing evidence,
and distinct invalid, stale, revoked, unsupported, unknown-key, unsupported
scheme, downgrade, incomplete-provenance, ambiguous-selection, identity-mismatch
and platform/architecture failure states. It defines the separate
platform/channel identities and a read-only artifact-source interface that
does not accept URLs or paths. No acquisition implementation is registered.

The production builder always returns a typed blocked result while the
production trust root, approved effect catalog, and P004 policy prerequisites
are unavailable. One source observation is not a complete authenticated
channel+manifest+artifact identity, so digest-only, metadata-only, native
identity, or synthetic evidence cannot bypass the evidence blocker. A
mismatched platform/channel and unsupported architecture are reported
explicitly. The builder never accepts operations from QML and never
manufactures an `InstallationPlan`; there is no production plan to pass to
`InstallerEngine::validate()` or publish as a P009 review candidate. A
test-only Ubuntu DEB fixture demonstrates one bounded native-package operation
and calls `InstallerEngine::validate()`. Existing synthetic P009 fixtures also
exercise review validation and fingerprint freshness; these fixtures are not
production evidence and do not call `execute()`.

| Channel | Expected identity/scope | P010 production result |
|---|---|---|
| Ubuntu 24.04 x86-64 DEB | Native package / `NativePackage` | Blocked: no approved trust root or native-package effect catalog |
| Fedora 42 x86-64 RPM | Native package / `NativePackage` | Blocked: no approved trust root or native-package effect catalog |
| Linux AppImage | Current-user integration / `PortableUser` | Blocked: no approved trust root or AppImage effect catalog |
| Windows per-user Setup | Current user / `CurrentUser` | Blocked: no approved trust root or Setup effect catalog |
| Windows portable ZIP | Portable user / `PortableUser` | Blocked: no approved trust root or portable effect catalog |
| Existing installation maintenance | Existing owned installation only | Blocked: ownership/preservation observations and approved maintenance catalog unavailable |

These channel entries are separate types. Windows per-user Setup is not
system-wide; portable ZIP is not a native package; Linux package effects do not
use the AppImage adapter. No Host support is inferred from generic client
compatibility. No channel currently has an approved P010 effect catalog.

## P009 review and freshness

P009 keeps `PlanReviewSnapshot` construction crate-private and validates a plan
with `InstallerEngine::validate()` before producing a testable review snapshot.
P010 does not make that constructor public or provide an untrusted desktop
construction path. The review fingerprint now also binds the authenticated
manifest generation (including explicit absence) and the existing-installation
observation generation, in addition to P009's plan, ordered effects, artifact
identity, scope, intent, platform, architecture, channel, policy IDs, findings,
and capability generation. A change to either generation changes the
fingerprint. This is an in-memory freshness commitment, not an authorization
token.

No acquisition or review snapshot is currently published to the desktop. The
Wizard's third step continues to say “Preview only — installation is not
started.” Its unavailable message now identifies the missing authenticated
release evidence and approved production trust policy. It exposes no artifact
path or raw release data and has no Install action.

A future effectful phase must reacquire/revalidate the policy, metadata, exact
artifact identity and bytes, current platform/channel, existing installation,
capabilities, and preflight state immediately before each effect. P010 does
not implement that handoff or any effect.

## Inherited gates and limitations

- P006 #165 merged with an explicit historical CI/review exception; formatting
  repair #167 merged. The focused authorization and PostgreSQL 17 maintenance
  checks passed on the repair head, but P006's joined PostgreSQL/ObjectStore
  owner-to-byte integration case remains absent.
- P007 #166 and P008 #168 merged with exceptions. P009's exact merged-head
  hosted status is mixed as described above and does not qualify the wizard.
- P028 Windows standard-user/native acceptance remains blocked/incomplete.
- P036 production artifact identity and independently provisioned trust
  keyring remain blocked.
- P004-D02, D03, D11 and D14 remain pending. P010 does not approve them.
- Revocation and anti-downgrade policy are not established as production
  policy here; no permissive fallback is selected.
- Native Linux Qt and Windows desktop commands must be rerun for this branch.
  Their status is recorded in the manifest after validation.

P011 prerequisites are an approved independent publisher-root/bootstrap and
rotation policy, production revocation and anti-downgrade decisions, an
approved channel-specific effect catalog with native owners, complete
preservation and verification contracts, native Linux/Windows acceptance, and
a validated bridge from authentic verifier output into the private P009 review
builder. Until then, production plan preview remains unavailable.

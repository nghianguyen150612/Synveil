# P012 — Last-use artifact verification and native adapter contracts

**Status: provisional, read-only contract increment. Production execution and
publisher verification remain unavailable.** P012 was implemented from
`4e3833044522e167c138c0f272f804511619d52f` on `develop/v0.3`. The authoritative
`Synveil-v0.3-Context-Roadmap-P001-P060.md` is not present in the repository;
this document records the requested P012 scope without treating it as an
approved security policy.

## Qualification context

Remote preflight on 2026-10-27 confirmed `develop/v0.3` remained at the
expected base SHA and `main` remained at
`604499fce0dfd134d82da3d5bb5d4a055c5eea8d`. P006 #165 (including formatting
repair #167), P007 #166, P008 #168, P009 #169, P010 #170, and P011 #171 were
merged. P011 #171 merged at the expected base SHA. This integration history
does not certify production trust or installation.

On the exact P011 merge commit, hosted Rust CI, quality, native Linux Qt 6,
native Windows Qt 6, Linux package, AppImage, Windows installer smoke, and the
later PG17 live suite passed. The Windows `standard-user-native` job failed
while accurately recording that the hosted runner has no qualified
interactive desktop or genuine new-logon session and that named-pipe
end-to-end acceptance is not wired. The successful Windows candidate build
does not replace that P028 evidence. P011's Linux/Windows Qt passes are
inherited evidence only; P012 must have its own exact-head hosted compilation
and UI checks before qualification.

PR #171 was integrated after a maintainer-requested expedited exception with
no independent reviews submitted. The pre-merge head had 88 checks: 80
successes, 2 failures, 2 in progress, 2 cancelled, and 2 skipped. The two
failures were both P028 `standard-user-native` outcomes; in-progress, cancelled,
and skipped checks were not converted into passes. On the merge commit, the
latest check rollup had 20 successes and one failure, the same P028
`standard-user-native` failure. This exception is not security certification
or approval of installer policy.

No authoritative v0.3 roadmap was found. P004-D02 (privilege and effect
ownership), D03 (optional Tailscale transport), D11 (presentation versus
engine authority), and D14 (publisher signatures, revocation, and downgrade
policy) remain `RECOMMENDED_PENDING_APPROVAL`.

## Verification architecture and trust boundary

`crates/install-engine/src/last_use.rs` defines the versioned
`ArtifactIdentity`, `ArtifactVerificationContext`,
`LastUseVerificationRequest`, `VerifiedArtifactHandle`, typed blockers and
outcomes, plus a generation-bound request state machine. The identity binds
artifact ID/type/version/source revision, expected SHA-256 and exact size,
manifest identity and generation, trust-policy identity, and provenance
reference. The review context binds platform, architecture, channel, full plan
fingerprint, capability generation, installation-state generation, catalog
version, and policy version. Missing or stale bindings block.

The only expected-evidence handoff is crate-private and accepts an
`AuthenticatedManifestArtifact`. It currently has no production caller. The
existing release authentication and acquisition implementation lives in the
Python release tooling (`release_channel.py`, `release_manifest.py`,
`release_signature.py`, `release_download.py`, and its policy materializers).
P010 does not expose those authenticated staged bytes through a Rust runtime
bridge. P012 does not duplicate or invoke that implementation and does not
embed a publisher key. The checked-in repository has no approved runtime
production keyring/policy for this Wizard path. As a result, the new Rust
fixtures are synthetic byte-verification tests only; they do not assert
publisher authenticity, channel freshness, revocation, or downgrade
authorization.

The installed release-security tests still exercise the existing trust
implementation, including test-key substitution, unknown/removed keys,
signature failure, exact manifest binding, size/digest mismatch, staging
races, and rollback/equivocation. Test keys remain test inputs. P036's
production artifact identity and approved trust policy remain unavailable;
unsigned CI artifacts and synthetic fixtures cannot fill that gap.

## Byte verification and platform guarantees

On Linux, the verifier opens absolute paths component-by-component with
`openat`, `O_NOFOLLOW`, directory-only ancestor opens, and a nonblocking leaf
open. It rejects unsafe path components, symlinks, non-regular files, unsafe
ancestor/file ownership or write permissions, and files over 2 GiB. It hashes
in fixed 64 KiB chunks, enforces exact expected size and SHA-256, compares
device/inode/mode/owner/group/size/mtime/ctime before and after reading, then
reopens the leaf and compares its object and parent-directory identities.
Open descriptors are closed by Rust ownership on every return path.

Linux returns a non-serializable handle holding the verified open file
description. It can detect a path replacement when current plan binding is
rechecked. There is no P012 consumer adapter that accepts this same file
description as its install input, and later writes cannot be excluded by this
read-only verifier. The handle therefore reports
`UseTimeIdentityNotGuaranteed`; the execution gate remains blocked even when
the synthetic digest matches. This is a check of bytes at verification time,
not a claim that a later path-based consumer uses the same bytes.

On Windows and other non-Linux targets, P012 does not yet use a native handle
that rejects reparse points and preserves file identity through consumption.
Verification returns `PlatformFileIdentityUnavailable` and blocks. Linux and
Windows identity guarantees are not treated as equivalent. The Linux tests
cover replacement, symlink substitution, FIFO rejection, truncation, appended
bytes, same-size content change, wrong digest, exact size, and stale context.
The existing Windows release-download security tests independently cover
ancestor reparse-point rejection; they do not qualify a P012 Windows handle.

## Adapter capability matrix

`crates/install-engine/src/native_adapter.rs` classifies capabilities and
checks plan effects against P011's closed `EffectId` catalog. It stores no
command, process, shell string, OS prompt result, or execution method.

| Channel | Typed owner contract | Allowed scope | Authorization contract | P012 production status |
|---|---|---|---|---|
| Ubuntu DEB / APT-dpkg | Ubuntu native package manager | `NativePackage` | Native package-manager authorization | Contract only; approved catalog and authorization are absent |
| Fedora RPM / DNF-rpm | Fedora native package manager | `NativePackage` | Native package-manager authorization | Contract only; approved catalog and authorization are absent |
| Linux AppImage | Current-user AppImage integration | `PortableUser` | Current-user operation contract | Contract only; no effect execution |
| Managed Linux Host action | Existing bounded Host helper | `System` for a scoped helper action | Bounded helper policy | Policy unavailable; no elevation |
| Windows per-user Setup | Current-user Setup adapter | `CurrentUser` | Current-user operation contract | Contract only; no UAC/elevation |
| Windows portable ZIP | Installer-engine-owned portable payload | `PortableUser` | Current-user operation contract | Contract only; no effect execution |
| Existing-installation maintenance | Must be identified and reconciled | Channel-specific | Policy unavailable | Blocked until identity and maintenance catalog exist |
| Unsupported elevated/system operation | No owner | None | Unsupported | Rejected |

Validation requires platform/channel/architecture/scope agreement, exact
adapter-owned effect IDs, a one-to-one catalog match, effect-owner and
resource-class consistency, privilege, verification, retry, recovery,
reconciliation, inverse, and preservation contracts. Unknown IDs, unknown
owners, cross-adapter effects, unsupported elevation, stale plans, and
missing generations are rejected. A structurally matching caller-supplied
test catalog remains `ClosedCatalogUnavailable`; P012 has no approved
production catalog.

## P011 readiness and recovery integration

`ExecutionGateInput` can receive a typed last-use result and adapter
validation. `ExecutionReadinessReport` exposes byte-integrity, publisher-trust,
last-use, and adapter status. It rejects stale plan fingerprints, artifact
identity/size/digest changes, manifest/capability/installation-state
generation changes, mismatched adapter results, unavailable publisher policy,
and use-time identity that cannot be guaranteed. The gate always retains
`ProductionExecutionCapabilityDisabled`, pending P004 approvals, missing
catalog/authorization and recovery blockers. Review acknowledgement is not
native authorization.

The existing read-only journal inspection and recovery classification remain
independent from artifact verification. P012 does not open or create a
production journal, mutate installation records, resume unknown transactions,
or retry effects. A verified byte result cannot clear journal corruption,
unknown prior effect outcome, missing compensation, final verification, or
reconciliation requirements. Existing journal schema and transaction model
are unchanged.

The Review page adds bounded availability/status categories and adapter
capability labels. It exposes no path, file descriptor, private staging data,
key, or command. It does not display “Verified publisher” or “Ready to
install”, has no functioning Install button, and preserves **“Preview only —
installation is not started.”**

## Tests and current qualification gaps

P012 adds isolated verifier and adapter fixtures. Tests cover exact digest and
size, wrong digest, missing digest/size, truncation, append, same-size content
change, symlink, FIFO, unsupported size, absent trust/manifest evidence, stale
manifest context, path replacement, duplicate/cancelled/stale completion,
unknown effects, owner mismatch, scope mismatch, unsupported Windows system
and Linux elevated scopes, stale capability bindings, synthetic closed
catalogs, and the permanent production-disable gate. P009 acknowledgement
expiry/replay tests and P011 journal unknown/corrupt/recovery tests remain in
the engine suite.

Local installer-engine tests, strict clippy, release-channel, release-manifest,
release-download, installer-security, installer-engine validation, journal
recovery, and installer-security-hardening checks passed. Local desktop Rust
test/clippy builds stop because `dbus-1.pc` is missing. The release-artifact
validator has no generated
`target/packages/SYNVEIL-LINUX-ARTIFACT-MANIFEST.txt` to inspect. After staging
the P012 modules, the desktop UI script passed its source preflight but stopped
because the available qmake shim points to missing `/usr/lib/qt5/bin/qmake`.
These local outcomes do not replace hosted P012 Linux/Windows results. Exact
P012 hosted status is recorded in
[`PROMPT012_MANIFEST.md`](../manifests/PROMPT012_MANIFEST.md) and must not count
queued, skipped, or cancelled jobs as passing.

## Independent gates and P013 prerequisites

P006's joined PostgreSQL metadata-to-ObjectStore owner-to-byte coverage remains
an independent missing gate. P028 requires a qualified Windows run with a
genuine interactive desktop, a genuine new-logon session, and named-pipe
end-to-end evidence; the hosted-runner blocker record is not acceptance.
P036 requires production-signed artifacts, independently approved trust
roots/policy, exact producer-to-artifact identity, and clean-machine/Phase C
evidence. These are not waived by P012 tests.

P013 must not start until the roadmap and applicable P004 approvals are
authoritative, an approved production trust source is bridged into the
engine, platform file identity can be preserved through the actual consumer,
closed per-channel catalogs and native authorization owners are approved, and
P006/P028/P036 evidence required by the proposed channels is available.
Journal recovery remains a separately qualified prerequisite. P012 does not
authorize a production installation path.

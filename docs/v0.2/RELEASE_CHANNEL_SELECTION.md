# Release channel and version selection v1

## Scope and closed metadata

`SYNVEIL-RELEASE-CHANNEL.json` is the stable-only recommendation document. Its
schema is `deploy/release/release-channel-v1.schema.json`: schema version 1,
product `Synveil`, channel `stable`, a positive generation, and at most 256
release entries. Entries contain only product version, source commit, the fixed
`SYNVEIL-RELEASE-MANIFEST.json` filename, exact manifest size/digest,
`fresh_install`, and at most 256 exact `upgrade_from` versions. URLs, origins,
keys, timestamps, display text, and artifact inventories are forbidden.

Versions are exact stable `MAJOR.MINOR.PATCH` values without leading zeroes,
prerelease, build, or `v` prefixes. Releases and compatibility sources are
strictly ascending numerically and unique. The dependency-free implementation
is `scripts/release_channel.py`; deterministic builders validate actual P005
manifest bytes and derive version, commit, size, and SHA-256. Output is sorted,
two-space UTF-8 JSON with a terminal newline and no environmental data.

## Authentication and rollback resistance

Remote channel bytes are unusable until authenticated by trusted local
`ChannelTrustPolicy`. Pin mode compares a locally trusted SHA-256 against the
exact raw bytes. Detached-signature mode uses
`SYNVEIL-RELEASE-CHANNEL-AUTH.json` and its v1 schema, but keys, allowed schemes,
and the verifier remain local. There is no TOFU or repository private key. All
locally eligible signatures are sorted and tried; successful evidence records
the actual key. Unknown keys/schemes and failed verification fail closed.

The 1 MiB bound is applied before parsing. `AuthenticatedChannel` binds raw
bytes, parsed document, and authentication evidence; every operation rechecks
that binding. The trusted local `minimum_generation` rejects older metadata.
Caller-owned `ChannelHighWater` is never persisted here: lower generation is
`CHANNEL_ROLLBACK`, equal generation with another digest is
`CHANNEL_EQUIVOCATION`, exact equality is accepted replay/no-op, and a higher
generation yields the new candidate. No wall clock is trusted.

Origins, redirect allowlists, and fixed channel paths are local configuration.
P011 adds no fetcher; callers reusing P006 transport must retain HTTPS-only,
credential-free, per-hop origin validation and bounded explicit redirects.

## Selection and evidence

Fresh installation chooses the numerically highest entry with
`fresh_install=true`, otherwise `NO_RELEASE_AVAILABLE`. Upgrade from exact `X`
chooses the highest target greater than `X` whose sorted `upgrade_from` contains
`X`, otherwise `NO_NEWER_COMPATIBLE_RELEASE`. Malformed current versions fail;
selection never normalizes, downgrades, treats repair as upgrade, or polls in
the background. Selection occurs only when explicitly invoked.

`SelectedRelease` is immutable and binds channel context, release index, and
entry. Substitution from another channel is rejected. Evidence includes channel
schema/name/generation/digest/authentication method and key, selection mode and
current version, and exact selected version/commit/manifest filename/size/hash.
It contains no URL or secret.

## P006 and P009 boundaries

The selected manifest size and digest first authenticate exact P005 manifest
bytes through P006; its product version and source commit are then checked
before P006 artifact selection. Thus a release selection is **not** proof that a
platform artifact exists. P006 remains authority for artifact identity,
transport, byte verification, and staging.

Likewise, P011 compatibility selection is **not** P009 mutation authorization.
P009 must independently validate installed source identity, target identity,
artifact identity, and lifecycle state before mutation. Repair remains an
exact same-version P009 flow. No application/client/database migration decision
moves here.

P011 errors remain internal tooling errors. Later integration should translate
absence to `ArtifactUnavailable`, trust/binding failure to
`IntegrityVerificationFailed`, lifecycle incompatibility to
`UnsupportedLifecycle`, transport failure to `DownloadFailed`, and unknown
states to `InternalFailure`, without changing P010 schema version 1.

## Explicit deferrals

There is no automatic/background update, download, installation, restart,
login task, UI popup, beta/nightly channel, rollout, telemetry, distro or
architecture qualification, installer technology, or v0.2.0 release. Future
channels and production key provisioning require separately reviewed work.


## Post-review hardening

Selection APIs require explicit caller-owned high-water input and evaluate rollback/equivocation before choosing any release. A signed/authenticated channel cannot be selected when its generation is below the supplied stored high-water or when the same generation has different authenticated bytes.

Channel authentication evidence is also bound back to the supplied local trust policy during authenticated parsing: pinned evidence must match the local pin, and detached-signature evidence must carry a locally trusted key ID and allowed signature scheme.

Selected-release validation rechecks selection semantics. Fresh-install selections must reference a release with `fresh_install=true` and carry no current version. Upgrade selections must carry an exact stable current version, target a numerically newer release, and have that exact source listed in `upgrade_from`. A manually reconstructed or semantically detached selection cannot drive the P006 manifest bridge.

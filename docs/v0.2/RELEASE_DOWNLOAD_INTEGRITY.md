# Release download and integrity contract

Status: **Prompt006 contract; acquisition only, not installation**.

## Trust and threat model

SHA-256 metadata is integrity evidence, not publisher authenticity: an attacker
who replaces both an artifact and an unauthenticated manifest can publish a
matching checksum. The mandatory chain is therefore trusted bootstrap input →
authenticate the exact manifest bytes → validate the P005 manifest → select one
exact artifact → download over trusted HTTPS → verify exact size and SHA-256 →
atomically stage. Only `AUTHENTICATED_PINNED_DIGEST` and
`AUTHENTICATED_SIGNATURE` authorize artifact consumption; all other states fail
closed.

`ReleaseTrustPolicy` is local trusted bootstrap configuration. Its origins,
manifest pin, limits, signature schemes, and key IDs come only from application
configuration, reviewed release tooling, or explicit trusted local input.
Remote metadata cannot enlarge these sets. There is no trust-on-first-use: a
key downloaded beside a release is not trusted.

The contract mitigates modified manifests/artifacts, replacement of artifact
and checksum together, downgrade and cross-origin redirects, credentials in
URLs, path traversal and symlink escape, oversized/truncated/partial responses,
wrong release identity, unsupported platforms/architectures, ambiguous
selection, untrusted keys/algorithms, destination collision, and stale cache
files. It does not solve compromise of an authorized signing key or trusted
origin, or intentionally malicious code signed by the authorized publisher.

## Manifest authentication

Pinned authentication requires a trusted caller-provided, lowercase 64-hex
`expected_manifest_sha256`. The digest covers the exact bounded bytes received;
JSON is neither parsed nor canonicalized first. The pin is secure only if its
own source is trusted. The raw manifest limit is **1 MiB**, inclusive.
After authentication, the implementation reuses the P005 semantic validator.
Optional expected product version and full source commit are equality checks;
there is no version ordering, recommendation, or latest-release behavior.
Parsing produces an immutable `AuthenticatedManifest` context containing the
exact bytes, validated document, and authentication evidence. Selection produces
a `SelectedArtifact` tied to that context. URL construction, download, staging,
and result creation revalidate that binding, so independently sourced
authentication, manifest, and artifact values cannot be combined.

The version-1 authentication descriptor schema binds the manifest filename,
size, SHA-256 and detached signature entries (`scheme`, stable `key_id`, and
signature filename). It contains no key and is not a trust root. The verifier
interface receives an already locally trusted key identity, scheme, exact
manifest bytes, and detached signature metadata. Unknown schemes/keys and an
unavailable verifier fail as `UNSUPPORTED_AUTHENTICATION`; failed verification
fails authentication. No production cryptographic backend or signing key is
provisioned by Prompt006.

Key rotation requires an explicit trusted-key set and an overlap period. A new
key must be installed in trusted local policy before releases rely exclusively
on it. A revoked key cannot authenticate new metadata; unknown keys fail. Test
keys remain isolated from production policy. Private keys must never be
committed, logged, or generated in source-controlled paths. Production signing
key provisioning is pending.

All descriptor signatures whose scheme and key ID are locally allowed are
considered in deterministic order. Authentication succeeds on the first one
that verifies and records that entry's actual key ID; no eligible entries, an
unavailable verifier, or failure of every eligible entry fails closed. Thus an
old/new overlap is independent of descriptor ordering.

## Network and selection policy

Origins are normalized as HTTPS scheme, lowercase host, and effective port.
Production acquisition accepts HTTPS with normal certificate validation only;
HTTP, FTP, file URLs, and URL userinfo are rejected. Every manifest/artifact
URL must match its caller-supplied allowlist. Redirects are handled explicitly,
are limited to five, and each target must independently be trusted HTTPS;
downgrades, untrusted targets, loops, and excessive redirects fail. No remote
metadata may supply origins, keys, arbitrary request headers, or credentials.
P005 filenames are joined to a trusted base URL; manifests never provide URLs.
The injectable boundary accepts only a `RawHttpsTransport`, whose contract is
to expose redirect responses. A normal potentially auto-following urllib opener
is rejected, so redirect policy cannot silently be bypassed by test or product
injection. Artifact URLs and requests are not constructed until the manifest
and selected-artifact binding have been validated.

Selection requires platform, architecture, artifact type, role, and exact
product version and returns exactly one record. Zero and multiple matches fail;
the first record is never chosen implicitly. Linux and Windows normalize to
`linux` and `windows`. `x86_64`, `amd64`, and `x64` normalize to `x86_64`;
`aarch64` and `arm64` normalize to `aarch64`. Unknown values fail, and a known
architecture does not imply that an artifact exists. Artifact type comes from
P005's closed enum, never a filename extension. Distro detection remains P018.

## Safe staging and handoff

The originally supplied destination root is rejected if it is a symlink before
resolution or creation. An owned unpredictable temporary file is then securely
created inside the resolved destination root. The authenticated artifact size
is a streaming hard limit; bounded chunks move directly from the HTTP response
to disk, early EOF and any extra byte fail, and memory use does not scale with
artifact size. SHA-256 is computed incrementally while writing, the file is
flushed and fsynced, and only exact size and digest permit atomic same-directory
promotion. Promotion uses a same-filesystem hard-link creation, whose atomic
create-if-absent semantics never replace a destination created concurrently;
platforms/filesystems without that primitive fail closed with `STAGING_ERROR`.
Failures remove the owned temporary file, never the final path.
Unsafe names and destination symlinks fail. An existing target is rehashed: an
exact match is `NOOP_ALREADY_VERIFIED`, while a mismatch is
`DESTINATION_CONFLICT` and is never overwritten. This target check happens
before opening the artifact response, so either existing-target outcome makes
zero artifact network requests. Invalid or unbound authentication likewise
makes zero artifact requests and writes.

The machine-readable result binds artifact ID/type/role/platform/architecture,
product version and source commit, artifact size/digest, exact manifest
size/digest, authentication state/method/key ID, final staged path, and redacted
diagnostics. This bridges P004/P005 evidence to later P047/P048 evidence and can
be journaled by P008 without recording secrets.

A verified download is **not** an installed application. This primitive never
executes EXE, AppImage, package manager, or script. P007+ owns installation.
P011 owns channel endpoints, latest/recommended selection, and ordering. P017
may consume this primitive only in authenticate → verify → install order; a
future one-line flow must not execute an unauthenticated `curl ... | sh`
payload.

## Current readiness

- Pinned trusted-manifest digest authentication: implemented.
- Detached-signature descriptor and verifier boundary: defined and fail-closed.
- Production detached-signature backend: pending approved implementation.
- Production signing key provisioning: pending.
- Current produced artifacts remain DEB, RPM, and Windows portable ZIP;
  AppImage and Windows installer are schema-supported but not yet produced.

# Synveil File Provider foundation (P053)

This target is an isolated, fail-closed foundation for a later Files integration. It uses Apple's
replicated File Provider model (`NSFileProviderReplicatedExtension` and
`NSFileProviderEnumerator`), available since iOS 16 and compatible with this project's iOS 17
deployment target. Apple documentation:

- [File Provider](https://developer.apple.com/documentation/fileprovider)
- [Replicated file provider extension](https://developer.apple.com/documentation/fileprovider/replicated-file-provider-extension)
- [`NSFileProviderReplicatedExtension`](https://developer.apple.com/documentation/fileprovider/nsfileproviderreplicatedextension)
- [`NSFileProviderEnumerating`](https://developer.apple.com/documentation/fileprovider/nsfileproviderenumerating)
- [`NSFileProviderItemVersion`](https://developer.apple.com/documentation/fileprovider/nsfileprovideritemversion)
- [`NSFileProviderManager`](https://developer.apple.com/documentation/fileprovider/nsfileprovidermanager)
- [`NSFileProviderDomain`](https://developer.apple.com/documentation/fileprovider/nsfileproviderdomain)

The replicated model is the current Apple API for a provider whose hierarchy is mirrored by the
system. It offers explicit item lookup, enumeration, content fetch, mutation, version, and lifecycle
callbacks. P053 implements only the framework entry point and bounded error behavior. The old
`NSFileProviderExtension` API is not used.

## Isolation and disabled capabilities

- The extension has its own principal class and does not load `SynveilApp`, SwiftUI, the app
  dependency container, Rust FFI, the existing SQLite database, a Keychain store, or URLSession.
- Initialization only records a process-local lifecycle state. It does not inspect the App Group,
  restore a session, register a domain, submit a transfer, or contact a server.
- Production domain registration, metadata publication, authenticated network access, and remote
  mutation are all disabled in `FileProviderCapability.production`.
- Enumeration and item lookup return `notAuthenticated`; no unavailable Library is presented as an
  empty Library. Content fetch returns no URL. Create, modify, and delete fail with Apple's
  `cannotSynchronize` error.
- No File Provider domain is registered by app startup. P053 does not advertise an empty provider.

## App Group boundary

Both targets' entitlement files declare `group.com.synveil.ios`; the extension's
`NSExtensionFileProviderDocumentGroup` agrees with that identifier. It shares the reverse-DNS stem
of the app bundle identifier `com.synveil.ios`, but actual entitlement availability still depends
on the Apple Developer Team's App Group registration and provisioning profile. Unsigned Simulator
builds verify target wiring only; they do not establish that the entitlement is active.

`FileProviderSharedContainer` resolves the group only through
`FileManager.containerURL(forSecurityApplicationGroupIdentifier:)`. A missing entitlement or
container is an error. It never falls back to the app sandbox or relocates SQLite. Files are scoped
under `Synveil/FileProvider/v1`, protected until first unlock, excluded from backup, bounded to
8 MiB per snapshot and 64 MiB total provider storage, and rejected if a component is unsafe or a
symlink is encountered. Snapshot publication writes and synchronizes a private same-directory
staging file before replacing the current file atomically. P053 does not wire publication into app
startup or the extension.

The current v0.1 database stays app-private at SQLite v9. If a future milestone needs shared
metadata, the host app must publish a validated immutable snapshot through this boundary. The
extension remains a read-only snapshot consumer; it must not open the app database as a second
writer. Any later database migration requires an independent design and compatibility review.

## Identity and projection

One domain represents one server/account/device credential generation and contains multiple
Library roots. The opaque domain identifier hashes the canonical endpoint, account identity, device
identity, and non-secret credential-generation identity. Credential replacement therefore changes
the domain identity; it cannot silently reassign the old domain. Domain display names must not
contain endpoints or account identifiers.

Item IDs are versioned and bind the opaque domain fingerprint, Library fingerprint, item kind, and
losslessly encoded validated Synveil ID. Node rename and move preserve identity. FileVersion is
separate from Node identity and is carried as immutable content-version evidence. A metadata
revision is used only when the server supplies it. For a later `NSFileProviderItemVersion` adapter,
the content and metadata components are 32-byte SHA-256 digests of the immutable FileVersion
identity and server metadata revision, respectively; an absent revision remains absent. Names and
local paths are never identifiers.

The `FileProviderMetadataSnapshot` schema is version 1 and records domain and owner-scope
fingerprints, Library identity, projection generation/completeness, stable item and parent IDs,
kind, display name, optional authoritative size/timestamps, immutable FileVersion and digest,
metadata revision, tombstone state, and source provenance. Validation rejects partial projections,
future schema versions, wrong ownership or Library, corrupt and colliding IDs, invalid parentage,
oversized payloads, and unresolved local mutations or conflicts. Canonical P050 conflict evidence
stays in the app and is never projected as a server-confirmed rename.

## Authorization and lifecycle boundary

The app's DeviceBearer credential remains in its existing default Keychain access group. P053 adds no
Keychain access-group entitlement and performs no credential migration. The extension cannot read
that credential. `FileProviderAuthorizationEvidence` is a versioned, non-secret interface for a
future app-mediated authorization protocol; P053 publishes no marker. A future implementation must
fence account/device/credential-generation replacement, logout, revocation, protected-data loss,
and extension relaunch before allowing metadata reads.

The extension lifecycle is independent of SwiftUI and P052 app recovery. Invalidation stops future
work, and a relaunched process starts without session or authorization state. P046 background
uploads and P051 background downloads remain disabled. No P052 transfer recovery is called.

P054 must implement authorized remote enumeration, item lookup and metadata change signaling over a
validated projection, while keeping domain registration disabled until those paths are qualified.
P055 requires a separately reviewed immutable FileVersion materialization design, exact byte-length
and SHA-256 verification, session/account/credential fencing, owned staging, and verified-only
publication. Files-originated mutations require a further authorized design with server revision
preconditions, durable intents, conflict preservation, idempotency, and unknown-outcome handling.

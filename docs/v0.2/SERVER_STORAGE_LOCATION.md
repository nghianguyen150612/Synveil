# Server Storage Location (Prompt032)

> Prompt033 performs only an identity-checked, exact-path service ownership handoff and never recursively changes an arbitrary selected tree. See [server services](SERVER_SERVICE_INSTALLATION.md).

Status: **implemented managed-storage selection and bootstrap foundation**

This contract concerns `SERVER_OBJECT_DATA`: opaque server-side objects owned
by Synveil. It is separate from a user's `CLIENT_LIBRARY`, the visible files and
folders synchronized by a client. The product asks: **Where should Synveil
store server data?** It does not ask the user to choose a synced folder.

The managed Personal/Home Host profile initially supports a local filesystem
backend. ObjectStore remains the canonical byte-storage adapter; this feature
adds server ownership, setup, and runtime verification above it.

## Location and review

The recommended Linux server-data root is `/var/lib/synveil/storage`. It is a
suggestion, not consent or initialization. PostgreSQL data remains at
`/var/lib/synveil/postgresql/17`; server configuration and credentials remain
under the P031 protected configuration locations. Choosing server object data
does not relocate either.

A user may select another suitable local filesystem path. A mount root is not
claimed as a whole: the wizard proposes a dedicated `Synveil` child. A new
selection must be either one missing leaf below an already existing safe
parent or an existing empty dedicated directory. Setup does not recursively
create a path chain. It rejects filesystem roots, home/profile roots, current
working directories, source/package/config/credential/PostgreSQL/runtime roots,
unsafe shared locations, symlinks, redirected paths, non-directories, unsafe
permissions, and overlaps with known client-library roots. Overlap uses path
components, so sibling names such as `/data/Synveil` and `/data/Synveil2` are
distinct.

External or removable filesystems are supported only while available and
qualified by the platform adapter. The wizard reports observed capacity and
availability. Its current Linux view reports removability as `Unknown`; it
does not infer internal or removable media from a path or mount label, promise
drive health, or claim durability beyond tested filesystem behavior. Disk
formatting, partitioning, mounting, encryption, RAID, filesystem creation,
and mount configuration are outside this feature.

## Inspect, review, confirm

Browsing and inspection are read-only. They do not create a directory, marker,
ObjectStore layout, or storage ID. The UI-neutral controller returns finite
classification and product status values, selected display location, measured
capacity, and a dedicated-child proposal where needed. Technical capability
evidence stays in typed configuration rather than the ordinary UI view. The
controller does not enumerate unknown directory contents.

On Linux, capacity comes from native `statvfs` data, including total bytes,
available bytes, available file count where reported, and read-only state. No
marketing minimum is assumed. A caller may supply a concrete `required_bytes`
value. Unknown capacity is surfaced as attention and setup fails closed for
the managed Host target. Read-only storage is rejected before initialization.

The review plan binds the configuration fingerprint/generation, server
installation identity and storage state, selected canonical path and native
directory/parent identity, classification, and displayed capacity snapshot.
Confirmation repeats native capacity inspection and rejects a changed
classification or a capacity that no longer satisfies the caller's concrete
requirement. The available-byte counter may move as other filesystem users
write, so a harmless counter change does not stale the plan by itself. A
parent-directory setup lock and P031 compare-and-swap make concurrent
selections converge on at most one authoritative root.

After confirmation, setup runs a bounded disposable write/durability probe in
the managed root. It checks exclusive create, write, file sync, same-filesystem
no-replace rename, directory sync, reread, and exact probe-file removal. The
canonical LocalFilesystemObjectStore also reports its tested capabilities.
Optional accelerators such as reflink, hardlink promotion, snapshots, and
compression are not correctness requirements.

## Durable state and identity

P031's typed V1 configuration storage state is:

```text
NotConfigured
PreparingLocal { root, storage_id, root_identity }
ConfiguredLocal { root, storage_id, root_identity, capabilities }
```

`capabilities` is the existing LocalFilesystemObjectStore report observed by
its adapter probe and validated before the ready transition. It is stored in
the typed configuration so an API or worker restart can restore the same
evidence without probing or changing the filesystem. Current upload setup
requires `AtomicPromotion`, checksumming, read-after-write, and `DurableFlush`;
the local adapter currently relies on a tested same-root hard-link promotion
and has no portable promotion fallback. A location that does not support this
required behavior is rejected. Reflink, snapshots, compression, and other
optional accelerators remain unnecessary.

The first confirmed operation creates one UUIDv7 `storage_id`, distinct from
the server installation ID, object IDs, client library IDs, filesystem UUIDs,
and mount IDs. P031's typed compare-and-swap writer durably stores
`PreparingLocal` before any storage initialization. Existing server identity,
database mode, network settings, credential references, and unrelated fields
are preserved.

`root_identity` binds setup to either the selected missing leaf's existing
parent device/inode or an existing directory device/inode. Once a new leaf is
atomically published with its identity marker, setup records the final
directory device/inode before initializing the ObjectStore. Configured runtime
requires the same directory identity as well as matching markers. Reusing the
same text path for a different filesystem or directory is not sufficient.

The P032 marker is a bounded (4096-byte maximum), canonical, newline-terminated
JSON document named `.synveil-server-storage.json`. Version 1 uses strict
`deny_unknown_fields` parsing and contains `schema_version`, `kind`,
`server_installation_id`, `storage_id`, `backend`, and `object_layout_version`.
It contains no secrets, administrator identity, tokens, or user filenames.
Unknown versions, fields, malformed content, symlinks, and conflicting
identities fail closed and are never rewritten.

The marker identifies managed server ownership. It does not replace or
overload the LocalFilesystemObjectStore marker `.synveil-storage-root`; that
adapter retains authority over its own marker and `objects/`, `objects/v1/`,
and `staging/` layout. Both markers must be present and valid for a ready
managed root.

## Initialization and recovery

After `PreparingLocal` is durable, setup creates only the exact selected leaf
with restrictive permissions when needed, writes and verifies the server
identity, initializes the existing ObjectStore adapter, verifies required
capabilities and the write probe, and only then commits `ConfiguredLocal` by
CAS. It reopens the exact configured identity before reporting ready.
The existing-only managed runtime opener restores the persisted capability
report; it does not rerun bootstrap probes.

`PreparingLocal` is durable recovery state. Retry keeps the same path, server
installation ID, and storage ID. Known partial P032 artifacts may be completed
only when configuration and marker identity establish ownership. Unknown
artifacts require repair and are preserved. A lost acknowledgement is
reconciled by reading authoritative configuration and root identity; it is not
blindly replayed. Cancellation after `PreparingLocal` leaves resumable setup,
not an unconfigured claim.

Recognized states include a new candidate, empty directory, matching ready
storage, matching interrupted setup, foreign managed storage, legacy
ObjectStore without server identity, non-empty unknown directory, unavailable,
read-only, wrong type, redirected path, unsafe permission, insufficient or
unknown capacity, malformed marker, root identity mismatch, and incomplete
ObjectStore layout.

A non-empty unknown directory is never initialized or cleaned. A legacy
`.synveil-storage-root` without the P032 marker is not automatically adopted.
Foreign server/storage identity is never overwritten. A configured root that
disappears remains configured and is reported disconnected; setup does not
create a replacement. If another directory or filesystem later occupies the
same path, its native root identity and P032 marker must still match.

There is no ordinary storage relocation in P032. Selecting a different root
when one is configured returns a migration-required result. Moving data needs
future coordinated replication, verification, cutover, rollback, and
reconciliation design.

## Runtime and ownership boundary

Managed API and worker startup use an existing-only opener. It validates the
configured root identity, both identity markers, all required layout
directories, and the typed capability report without creating, probing,
repairing, or replacing storage. It does not rerun the bootstrap capability
probe. It also rechecks native capacity availability and the filesystem
read-only flag, and fails closed when that inspection is unknown. A missing or
mismatched root prevents managed runtime use. Legacy operator mode with an
explicit `SYNVEIL_OBJECT_ROOT` retains its existing ObjectStore behavior and
is not silently converted to managed storage.

Setup never recursively changes ownership or permissions. A selected location
must be dedicated so later service ownership can be applied without taking
over unrelated files. Exact service identity and ownership handoff belong to
P033. Storage selection does not install/start services, initialize
PostgreSQL, configure networking, create the first administrator, or report
whole-server readiness.

## Handoffs and limits

- P033 owns service installation, privilege boundaries, and exact service
  ownership.
- P034 owns server networking and reachability.
- P035 owns first-administrator bootstrap.
- P036 composes the complete Host wizard and clean-machine acceptance.
- P037 owns the unified Welcome experience; P040 owns client-library first-run
  selection.

Managed Host execution is currently qualified for Linux, targeting Ubuntu
24.04 x86_64 and Fedora 42 x86_64. Other platform adapters return
`UnsupportedPlatform`/not-yet-qualified while the shared state model remains
portable. Native test fixtures prove local behavior only; they do not establish
clean-machine Host readiness or physical power-cut behavior.

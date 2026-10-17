# PROMPT057 Manifest: PhotoKit Resource Extraction and Foreground Upload

**Status:** Implemented on the feature branch; native iOS validation and review are pending.

## Repository and dependency status

- Integration branch: `ios-app`.
- Verified starting SHA and P056 merge: `b8744a079e2b73fb6ef6b37f24e4bdb538a43ed5` (PR #140).
- P055: PR #139 remains open and unmerged. P057 does not use its File Provider sources.
- Feature branch: `ios/p057-photokit-resource-transfer`.
- Feature SHA: recorded after the final implementation commit.
- PR and independent review: recorded after opening the single P057 pull request.

## Resource selection and extraction

`PhotoResourceSelection` records the PhotoKit local identifier, resource ordinal and type,
role, rendition, UTI, and source modification date. The ordinal disambiguates same-role
resources in the current asset resource list; the original filename is not an identity.
Original photo/video and Live Photo motion components are represented independently.
Full-size adjusted resources are labeled edited. Adjustment-base still and video resources
are treated as original representations. Alternate, proxy, adjustment-data, and audio
resources are visible as auxiliary selections and are never silently promoted to originals.

`PhotoKitResourceExtractionSource` re-fetches the asset under current full or limited
PhotoKit permission, re-resolves the selected resource, and uses
`PHAssetResourceManager.requestData`. `isNetworkAccessAllowed` is always false. A missing
local original returns `sourceUnavailable`; it does not mark the observation deleted or
substitute a preview. Request cancellation is retained through the PhotoKit request ID.

Extraction is explicitly user initiated, checks the current authenticated profile, owner,
device, discovery opt-in, video policy, PhotoKit permission, observation generation, and
selected resource before and after streaming. Only one extraction can run at a time. The
maximum per-resource size is bounded by the smaller of the configured request size and half
the shared staging quota. PhotoKit callback chunks are capped at 1 MiB and synchronously
consumed in 256 KiB pieces to keep application-owned buffers bounded.

## Staging, integrity, and durable state

PhotoKit stages into the existing P043/P045 private transfer root through
`PhotoResourceStagingCoordinator`; it does not create a second storage engine or use the File
Provider App Group. Files use opaque UUIDs, restrictive permissions, device file protection,
backup exclusion, no-follow descriptor operations, the shared disk quota, and free-space
reserve. Temporary files are renamed atomically only after streaming SHA-256 and byte length
match the open file. The exact final file is rehashed before READY and before P045 handoff.
The digest is `sha256:` plus 64 lowercase hexadecimal digits. Raw media bytes and credentials
are not stored in SQLite.

The isolated PhotoObservation SQLite store advances from schema version 1 to 2 in a
transaction. Version 2 adds one durable active authorization row and resource extraction
records. The central application SQLite schema remains v9. READY and handoff state changes
are committed inside transactions that verify the active authorization generation. Reads
filter by that same generation, preventing a new session for the same profile from reusing an
old READY record. Logout synchronously drains the PhotoKit SQLite queue and revokes its active
authorization before session teardown; a failed barrier blocks later authorization writes.
Every observation start carries the authenticated `SessionController` lifecycle revision, so a
delayed pre-logout task cannot reactivate PhotoKit authorization after the synchronous barrier.

The extraction lifecycle includes discovered, eligible, extracting, verifying, verifiedReady,
handoffPending, handedOff, uploadedConfirmed, sourceUnavailable, permissionRevoked, cancelled,
failedRecoverable, and failedTerminal. Records persist the policy version, maximum byte policy,
and explicit user-initiated flag. READY and handoff-pending transactions also check current
discovery consent, video policy, and the durable observation generation. An unresolved exact
resource identity is unique within its owner/device/authorization generation, preventing a new
P045 operation from being created while an earlier one is unresolved. Startup recovery rehashes
READY files and removes unreferenced partial/orphan stages. Handoff-pending operations retain a
stable P045 operation ID and can be explicitly retried. A P045 row owns the independent upload
staging after durable handoff; PhotoKit never reports upload success based only on local staging.

## Discovery and transfer behavior

PhotoKit discovery now exposes a deterministic bounded page API with snapshot ID, generation,
offset, and total count. A scan commits each page under the durable authorization generation,
checks the PhotoKit change revision and permission before and after writes, and restarts under
a new generation if the library changes. It stops as partial if a legacy adapter has no
continuation; it never loops the first page or infers deletion from incomplete results.
Observation upserts refresh owner scope when the same profile is used under a different
authenticated owner.

The UI keeps metadata discovery opt-in separate from the per-resource prepare and upload
actions. The user selects an existing active Library, parent, and filename. Handoff uses the
existing `FileUploadCoordinator`, `UploadTarget`, `UploadExpectedContent`, and P045 immutable
intent/idempotency flow. Background upload is explicitly false; no new HTTP client, URLSession
background task, server PhotoAsset binding, or remote deletion path is introduced. A handed
off Live Photo still or motion component has its own stage, hash, P045 operation, and outcome.
Complete Live Photo group binding remains a P058 backend prerequisite.

## Tests and validation

New deterministic XCTest coverage is registered in `PhotoResourceTransferTests.swift` for:

- Multi-chunk source data becoming a private stage with exact SHA-256 and length.
- Same-owner authorization-generation replacement hiding READY rows and stage IDs.
- Logout queued behind an in-flight SQLite commit, followed by rejection of old-token writes.
- A delayed pre-logout authorization activation is rejected while a newer authenticated revision can start.
- Logout between final staged hash verification and P045 upload start.
- Source-unavailable extraction preserves its PhotoKit observation and removes partial bytes.
- Exact P045 foreground adapter arguments, background disabled, and stable retry identity.
- More than 1,000 observations traversed across two bounded pages without duplicates.
- Version 1 PhotoObservation schema migration preserves rows and creates the version 2 extraction table.

The existing P001–P056 XCTest sources remain registered. The iOS source-registration and
architecture validator passed. Portable validation passed with 120 support tests; docs
validation and `git diff --check` also passed. The local execution environment does not contain
`xcodebuild`, `swift`, or `swift-format`, so native build, simulator XCTest, strict formatting,
and the four required iOS workflow results are pending GitHub CI on the final branch head.

Physical iPhone and controlled live-upload validation are deferred with the exact states:

- `DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`
- `DEFERRED_LIVE_UPLOAD_DEPLOYMENT_NOT_AVAILABLE`

## Review, merge, and P058 prerequisites

- Independent review: pending PR creation and reviewer assignment.
- PR/merge status: pending; this branch has not been merged.
- P058 prerequisites: reviewed server-side Live Photo group-binding contract, a separately
  consented iCloud retrieval policy if desired, and a reviewed PhotoAsset binding contract.
  Automatic camera-roll upload, background URLSession upload, remote deletion, and P058 work
  remain disabled and unauthorized.

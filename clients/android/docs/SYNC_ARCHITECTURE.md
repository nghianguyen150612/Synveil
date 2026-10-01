# Android synchronization architecture

The Android client keeps a non-authoritative Room projection of libraries and
logical nodes. Authentication remains split: server profiles and preferences
use DataStore, while DeviceBearer credentials remain in Android Keystore-backed
storage. Room contains no credential, cookie, CSRF value, or Keystore key.

## Bidirectional cycle

`SyncCoordinator` owns one in-process lock for each profile/device/library
scope. A cycle recovers inbound ACK and rebaseline obligations, performs a
bounded inbound refresh, drains a bounded number of immutable outbound
mutation records, then performs another inbound refresh. WorkManager and the
foreground Sync now action use this same coordinator and unique scope keys.

A queued metadata mutation retains its UUIDv7 identity, base epoch/sequence,
typed payload, optimistic revisions, and local integrity fingerprint. A lost
response moves the record to `OUTCOME_UNKNOWN`; retry sends the exact same
request. The server's durable mutation replay decides the outcome. Android does
not silently rebase stale user intent or advance the inbound checkpoint from a
mutation response.

## Content replacement

Replace-content sources are streamed from SAF into an application-private
`filesDir/transfer-staging` file while calculating exact byte length and
SHA-256. Room stores only staging metadata and the path. Upload offsets are
server-authoritative and ambiguous chunk outcomes are reconciled by reading the
upload session. Staging is retained until a confirmed commit or safe terminal
cleanup; staging is not exposed through FileProvider or external storage.

## Conflicts

The current server intentionally keeps conflict list/detail/resolve routes
BrowserSession-only. Android persists a conflict result and reports that owner
review is required; it does not invent a DeviceBearer resolution route or
choose a winner automatically. Historical conflict evidence must not be used as
fresh canonical revisions.

## Background policy and limitations

WorkManager uses unique per-scope work, connected/unmetered constraints,
optional battery-not-low constraints, exponential retry for transient failures,
and a platform minimum periodic interval of 15 minutes. Scheduling is
approximate. There is no always-running foreground daemon, automatic conflict
resolution, arbitrary filesystem mirror, or silent revision rebasing.

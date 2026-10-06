# Installation-to-sync progress

Prompt041 composes the existing desktop owners into one bounded first-run
journey. It is a projection, not a new installer, setup state machine, or
sync engine.

## Stages

The Rust presentation model exposes exactly five stages:

| ID | Product title | Evidence owner |
| --- | --- | --- |
| `app_ready` | Synveil ready | desktop process/control snapshot |
| `server_ready` | Server ready | canonical configured profile and current connection state |
| `signed_in` | Signed in | canonical authenticated-device state |
| `library_ready` | Library ready | authoritative configured-library snapshot |
| `first_sync` | First sync / Up to date | client-sync runtime plus durable quiescent evidence |

Each stage has one of `pending`, `active`, `waiting`, `complete`, or
`action_required`, with bounded product detail and an optional action owned by
an existing surface. QML renders the model and does not infer state from raw
labels.

The desktop has no reliable native installer telemetry. Accordingly, the
first stage says **Synveil ready** when the desktop process is ready; it does
not claim that an Inno Setup, DEB/RPM, AppImage, or shared installer stage was
observed. Native installers and package managers retain their existing
ownership.

## First-sync truth condition

`first_sync_completed` is durable local evidence introduced with schema
migration 0008. A new library starts false. Legacy configured replicas are
treated as existing users and default true during migration, so they are not
forced through first-run onboarding again.

The sync runtime promotes the marker only through its existing bounded cycle
supervisor, after all of these are true:

- the cycle result is `SyncCycleResult::is_idle()`;
- no follow-up wake is pending;
- the library root is available and the authenticated device is ready;
- no unresolved conflict fence blocks convergence;
- the runtime is no longer running or scheduled;
- the safe controller snapshot is fresh and current.

`Progress`, `more_work_likely()`, scheduled work, backoff, and a requested
sync are not completion evidence. The UI uses an indeterminate message such
as **Syncing your files…** and never invents a percentage or item total.

## Waiting and action required

Transient network unavailability, server backoff, scheduled work, root
reconciliation, and a running cycle are waiting states. Authentication
requirements, an unavailable root, unresolved conflicts, user pause, and
fatal local runtime states are action-required states only where the existing
desktop action is supported. Prompt041 does not add a repair console, root
relocation, reset, or destructive recovery action; those belong to P042.

## Restart and stale state

The GUI stores no completion flag. After restart it reconstructs the model from
the current client/control snapshot and durable library evidence. The bridge
rejects snapshots from an older connection generation or older revision, and
stale/unavailable snapshots cannot advance first-sync progress. Existing
configured libraries continue through the normal application surface after
their first-sync evidence is complete.

The projection contains only bounded product codes, labels, state, and safe
actions. It does not expose credentials, absolute roots, file names/content,
hashes, database rows, HTTP bodies, or internal sync terminology.

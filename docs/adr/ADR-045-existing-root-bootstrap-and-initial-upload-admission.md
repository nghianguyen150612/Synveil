# ADR-045: Safe existing-root bootstrap and initial upload admission

Status: Accepted — LOCKED Prompt 104
Date: 2026-09-19
Owners: Synveil client-sync, client, desktop, metadata maintainers

## Decision

The first-library setup flow may create a new remote library and initialize it
from a user-selected existing local directory. Ordinary existing files and
directories are admitted as local observations and converge through the
existing durable observer, namespace-mutation, staged-content, and upload
session pipeline. The flow does not add a bulk uploader, a second sync engine,
or an attach/import path for a remote library that already exists.

Prompt 103's empty-directory admission rule is superseded only for this
new-remote-library flow. Its root safety, marker identity, profile binding,
durability ordering, root-loss fencing, path validation, and deletion-safety
rules remain in force.

## Admission and identity proof

The client continues to generate one UUIDv7 `LibraryId`, persist the pending
non-secret manifest entry before remote mutation, and reconcile an uncertain
`POST /api/v1/libraries` response with the authoritative library list. The
server create remains idempotent by caller-supplied library UUID and returns
the canonical remote `root_node_id`.

Before host/runtime or watcher registration, the client durably commits all of
the following:

1. the profile-bound `.synveil/root-id` marker and owned control directories;
2. the `replicas` binding with the exact owner/device/library/profile scope;
3. a local root `Node` projection using the authoritative remote root NodeId;
4. the existing zero journal/applied/acknowledged starting position.

Repeating the same UUID, root binding, profile, and remote root is idempotent.
A different marker, profile, scope, root NodeId, or pre-existing incompatible
local state fails closed. The local absolute path is never sent to the server
or exposed through the safe UI/IPC model.

## Existing-root safety boundary

Root validation still rejects relative paths, missing paths, files, filesystem
roots, HOME/USERPROFILE, the process current directory, read-only roots,
symlink/junction/reparse redirects, and overlapping configured roots. Ordinary
non-empty content is not itself an error when this flow creates the new remote
library.

The `.synveil` name remains reserved. An absent control directory may be
created atomically. An existing control directory is accepted only when it
contains a bounded regular `root-id` marker that the later scoped marker
validation accepts. A control-tree collision, marker symlink/reparse point,
marker directory, incomplete marker, or incompatible staging/quarantine entry
is rejected; unrelated data is never overwritten or silently adopted as a
managed control tree.

The marker is published durably before recoverable control-layout completion.
If the process stops between those operations, a retry reopens the exact
marker and completes only the owned `staging` and `quarantine` directories.
The observer excludes `.synveil` from the root's managed namespace.

## Initial observation and convergence

The seeded remote root is the only pre-existing known local node. The bounded
observer therefore treats ordinary root children as new facts and creates
durable `CREATE_DIRECTORY` or `CREATE_FILE` intents. It never generates a
delete intent for an unknown path. A child intent whose physical parent has not
yet received a server NodeId remains durable but is not selectable for
submission. The normal directory mutation assigns that parent identity, and
the client then hydrates the pending child intent before staged, hashed,
verified upload-session work begins.

Remote initial state is the newly created root-only library. Normal inbound
initial/rebaseline processing remains responsible for convergence and does not
replace or delete ordinary local entries merely because they were not in the
remote snapshot. Existing local files are never overwritten or deleted solely
to make metadata match.

Root disappearance remains unavailable/deferred and fenced. A failed root
probe, watcher overflow, local scan limit, unstable file, unsupported special
entry, or ambiguous response does not authorize mass deletion. Durable pending
manifest and UUID identity remain the recovery boundary for restart and
`OutcomeUnknown` handling.

## Scope exclusions

This ADR does not implement or imply:

- attach/import of an existing remote library;
- remote path enumeration or a new bulk-upload endpoint;
- a setup secret, OAuth/OIDC, MFA, device-credential redesign, or sharing;
- a new PostgreSQL or client schema migration;
- a separate upload worker, sync engine, or UI conflict wizard;
- silent handling of symlink/junction/reparse redirects or special filesystem
  entries.

## Validation record

The implementation must report source, runtime, test, database, GUI, and
platform evidence separately. The Prompt 104 readiness token is emitted only
after all mandatory live acceptance gates pass; a partial local run must not
claim that token.

The server migration count remains 36 and the client migration count remains
7. This change is contract and orchestration work over the existing API and
SQLite schema.

## Quyết định / Vietnamese summary

Flow setup library đầu tiên được phép tạo remote library mới và bind vào một
directory local đã có file/directory ordinary. Các entry có sẵn đi qua observer
durable, namespace mutation, staged-content và upload-session hiện có. Không
thêm bulk uploader, sync engine thứ hai hoặc attach/import cho remote library đã
tồn tại.

Quy tắc root safety vẫn giữ nguyên: reject path relative/missing/file/root/home/
current directory/read-only/redirect/overlap và reject `.synveil` control tree
conflict. Directory không rỗng không còn bị reject chỉ vì có dữ liệu ordinary.
Client phải seed `root_node_id` authoritative vào `replicas` và root
`local_nodes` trong local state trước khi register runtime/watcher. Entry chưa
biết sẽ thành create intent; parent directory được submit trước, nested scan
tiếp tục khi server cấp NodeId. Root mất không bao giờ được hiểu là empty tree
hay permission để mass-delete.

Remote attach/import, bulk endpoint, migration mới, setup secret, flow UI
riêng, và các phạm vi auth/sync ngoài onboarding vẫn nằm ngoài ADR này.

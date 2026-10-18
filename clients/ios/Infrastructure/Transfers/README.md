# Synveil iOS — Transfer Engine Infrastructure (`clients/ios/Infrastructure/Transfers/`)

## Purpose & Ownership
Implements the foreground file download and 4 MiB resumable upload flows. Authenticated background file transfers remain capability-off under the P051 redirect security gate.

### Responsibilities
- Managing transfer job queues (`queued`, `preparing`, `running`, `paused`, `completed`, `failed`, `cancelled`).
- Creating immutable create/replace operations with `POST /api/v1/upload-sessions`, then slicing files into 4 MiB chunks and sending `PATCH /api/v1/upload-sessions/{upload_session_id}` requests.
- Querying `GET /api/v1/upload-sessions/{upload_session_id}` for authoritative offset reconciliation.
- Reconciling remote server `Upload-Offset` after network interruptions.
- Emitting progress events independent of SwiftUI view lifetimes.

## Prohibited
- Cancelling active transfers merely because a SwiftUI view was dismissed.
- Reporting transfer success before receiving authoritative server 200/201 response.

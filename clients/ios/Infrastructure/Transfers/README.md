# Synveil iOS — Transfer Engine Infrastructure (`clients/ios/Infrastructure/Transfers/`)

## Purpose & Ownership
Implements `TransferEngineProtocol` to manage background file downloads and 4 MiB resumable uploads.

### Responsibilities
- Managing transfer job queues (`queued`, `preparing`, `running`, `paused`, `completed`, `failed`, `cancelled`).
- Slicing files into 4 MiB chunks and sending `PATCH /api/v1/uploads/{session_id}` requests.
- Reconciling remote server `Upload-Offset` after network interruptions.
- Emitting progress events independent of SwiftUI view lifetimes.

## Prohibited
- Cancelling active transfers merely because a SwiftUI view was dismissed.
- Reporting transfer success before receiving authoritative server 200/201 response.

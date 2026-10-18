# Synveil iOS — Files & Staging Infrastructure (`clients/ios/Infrastructure/Files/`)

## Purpose & Ownership
Manages local application sandbox file staging (`Library/Caches/staging/`) and document import/export interactions via `FileManager`.

### Responsibilities
- Staging 4 MiB upload chunks and temporary file downloads.
- Enforcing transfer storage quota (maximum staging limit 512 MiB, free space floor reserve 64 MiB).
- Clearing expired or completed transfer staging files.
- Exporting downloaded files to user-selected destinations or Storage Access / Share Sheet.

## Prohibited
- Accessing paths outside the iOS application sandbox.
- Storing unencrypted secrets in temporary staging files.

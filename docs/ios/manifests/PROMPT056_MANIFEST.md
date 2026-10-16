# PROMPT056 MANIFEST: PhotoKit Camera Roll Foundation, Permission Lifecycle & Durable Asset Discovery

## 1. Executive Summary

PROMPT056 establishes a privacy-first, permission-aware, and durable architectural foundation for automatic camera-roll backup in Synveil iOS. P056 is the first milestone of the P056–P058 PhotoKit workstream.

P056 implements native PhotoKit authorization, bounded asset discovery, change observation, versioned SQLite local observation records, source availability classification, a safe opt-in SwiftUI configuration UI, and non-executing TransferEngine candidate handoff contracts.

**Primary Invariant:** PhotoKit discovery is an observation of the user's currently authorized local media, not proof that content has been uploaded, backed up, or deleted remotely. Automatic photo uploads, original byte streaming, and background transfer scheduling remain strictly disabled.

---

## 2. Integration Baseline & Prerequisite Status

- **Integration Baseline:** `b0a977a15878f04dbe5c351f352996de68f86211` (`origin/ios-app`, P054 merged).
- **Feature Branch:** `ios/p056-photokit-foundation`.
- **P055 Dependency Status:** Independent. P055 (File Provider materialization) was unmerged at the start of P056 development. P056 contains no unmerged P055 commits or dependencies.

---

## 3. Key Design Decisions & Architecture Boundaries

### 3.1 Permission Lifecycle
- Typed state enum (`PhotoPermissionState`): `notDetermined`, `authorizedFull`, `authorizedLimited`, `denied`, `restricted`, `unavailable`, `unknownFutureStatus`.
- Evaluated using `PHPhotoLibrary.authorizationStatus(for: .readWrite)` and requested via `PHPhotoLibrary.requestAuthorization(for: .readWrite)` strictly following explicit user gesture in `PhotoBackupSettingsView`. No automatic prompts on startup or background initialization.
- Limited access is handled as a supported, restricted view of the library.

### 3.2 Asset Discovery & Classification
- Bounded asset discovery via `PhotoLibraryDiscoveryService` and `PhotoLibraryObservationCoordinator`.
- Asset kinds (`PhotoAssetKind`): `stillPhoto`, `video`, `livePhoto`, `screenshot`, `unknown`.
- Resource roles (`PhotoResourceRole`): `primaryImage`, `primaryVideo`, `motionVideo`, `auxiliary`.
- Media metadata captured: Local identifier, creation date, modification date, pixel dimensions, favorite status, hidden status, discovery generation, permission scope, and source availability.
- Privacy boundary: GPS, EXIF, faces, OCR, captions, and full-resolution image bytes are **never** extracted during discovery.

### 3.3 Durable Observation Persistence
- Versioned, isolated SQLite store (`PhotoObservationSQLiteStore` and `PhotoObservationSQLiteSchema`, version 1) under app sandbox.
- Separate from `SyncCursor`, `BackupSnapshot`, and File Provider metadata.
- Tracks `complete` vs. `partial` vs. `interrupted` scan results. Interrupted or limited scans never imply local asset deletion or remote cleanup.

### 3.4 Change Observation & Session Fencing
- Native `PHPhotoLibraryChangeObserver` wrapped in `PhotoLibraryChangeObserverAdapter`.
- Thread-safe callback coalescing and generation increments.
- Integrated with `SessionController`: Upon logout or account switch, `PhotoLibraryObservationCoordinator.stopObservation()` unregisters observers and fences stale callbacks.

### 3.5 User-Facing Configuration
- SwiftUI `PhotoBackupSettingsView` and `@Observable` `PhotoBackupSettingsViewModel` in `Features/Photos/`.
- Opt-in camera roll discovery toggle (default **OFF**).
- Includes accessibility identifiers (`photo_backup_opt_in_toggle`, `grant_photos_permission_button`, etc.) and Dynamic Type support.
- Clear disclosures stating P056 only prepares metadata discovery and does not upload files or run background tasks.

### 3.6 Future TransferEngine Handoff Contract
- Non-executing `PhotoTransferCandidateDescriptor` type linking device identity, profile ID, local identifier, asset kind, resource role, proposed filename, and observation generation.
- Candidate descriptors prepare P057–P058 without invoking transfers or creating remote sessions.

---

## 4. Privacy Audit

- **No Logging:** Filenames, local identifiers, GPS, EXIF, thumbnails, album names, or raw media bytes are never logged.
- **No Uploads:** Discovery inventory is stored strictly locally in SQLite and never transmitted to any server during P056.
- **No Background Scanner:** Background execution remains disabled (`P046` background upload OFF, `P051` background download OFF).

---

## 5. Physical Device Deferral Statement

`DEFERRED_PHYSICAL_DEVICE_NOT_AVAILABLE`

Hardware physical device qualification of real PhotoKit permissions and camera roll enumeration is deferred due to headless Linux build sandbox environment constraints. Simulator and mock-based unit tests cover permission mapping, change callbacks, and persistence contracts.

---

## 6. Verification Evidence

1. `python3 clients/ios/Support/validate_ios_sources.py` -> Passed.
2. `python3 -m unittest discover -s clients/ios/Support/tests` -> Passed 120/120 tests.
3. `bash scripts/validate-docs.sh` -> Passed.
4. `git diff --check` -> Passed cleanly.

---

## 7. Next Steps & P057 Prerequisites

P057 will implement resource extraction, staging, and TransferEngine integration for camera roll media upload. P057 must remain disabled until authorized.

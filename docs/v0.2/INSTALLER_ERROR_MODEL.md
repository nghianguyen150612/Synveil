# Installer error model

Status: **Prompt010 contract; presentation and safety mapping only**.

Prompt010 defines the finite platform-neutral boundary between installer internals and later user interfaces. It maps P006 acquisition failures, P007 engine failures, P008 journal/recovery states, P009 lifecycle/purge failures, and typed future platform/preflight failures into stable user categories, safe actions, retry semantics, and bounded diagnostics. It does not render UI, execute retries, select release channels, upload diagnostics, or mutate installation state.

## Finite presentation model

Schema version: `ERROR_MODEL_SCHEMA_VERSION = 1`.

The eight source families are Engine, Journal, Recovery, Lifecycle, Purge, Acquisition, Preflight, and Platform. The nineteen user categories are UnsupportedPlatform, UnsupportedArchitecture, ArtifactUnavailable, InsufficientDiskSpace, PermissionRequired, DownloadFailed, IntegrityVerificationFailed, StoragePreparationFailed, PackageInstallFailed, RuntimeDependencyFailed, IntegrationFailed, VerificationFailed, Interrupted, RecoveryRequired, ConcurrentOperation, UnsupportedLifecycle, ProtectedStateBlocked, InvalidSetupState, and InternalFailure.

The seventeen actions are closed and intentionally omit force/ignore/bypass/reset operations. The six retry policies are SafeImmediate, AfterUserAction, ReconcileFirst, FreshEvidenceRequired, NotRetryable, and NotApplicable. Retry safety is independent of category classification.

The presentation stage model is separate from P007's mutation stages and covers Preflight, Plan, Acquire, VerifyArtifact, Install, Integrate, VerifyInstallation, Complete, Recovery, and LifecyclePolicy.

## Retry safety

`OutcomeUnknown`, `StillUnknown`, reconciliation-required states, failed compensation, missing resume evidence, and recovery inspection map to `RecoveryRequired` with `ReconcileFirst`. They never expose an immediate retry.

`NETWORK_ERROR` is the sole P006 acquisition failure mapped to `SafeImmediate` and `RetryDownload`, because it is an acquisition failure before installer mutation.

Trust and integrity failures require `FreshEvidenceRequired` and `GetFreshOfficialDownload`. There is no bypass-verification action. Ownership, preservation, and purge-scope failures fail closed and expose no destructive workaround.

Concurrent transaction states never start a second mutation transaction. Stale plans require a replan. Native authorization remains scoped to the platform-owned permission mechanism rather than instructing users to run the whole application elevated.

## P006 acquisition bridge

The exact Prompt006 codes are:

```text
AMBIGUOUS_ARTIFACT
ARTIFACT_DIGEST_MISMATCH
ARTIFACT_TOO_LARGE
ARTIFACT_TRUNCATED
DESTINATION_CONFLICT
INSUFFICIENT_DISK_SPACE
INVALID_MANIFEST
MANIFEST_AUTH_FAILED
MANIFEST_TOO_LARGE
NETWORK_ERROR
NO_MATCHING_ARTIFACT
SOURCE_COMMIT_MISMATCH
STAGING_ERROR
UNSAFE_PATH
UNSAFE_REDIRECT
UNSUPPORTED_ARCHITECTURE
UNSUPPORTED_AUTHENTICATION
UNSUPPORTED_PLATFORM
UNTRUSTED_ORIGIN
VERSION_MISMATCH
```

`tests/install-error-model/p006-acquisition-codes.json` is the machine-readable bridge. The Python validator parses `scripts/release_download.py` with the standard-library AST and rejects dynamic/non-literal `AcquisitionError` codes. Rust tests require every fixture code to parse into one closed `AcquisitionFailureCode` variant and every variant to occur in the fixture. Unknown external codes fail closed to a finite internal diagnostic without echoing the untrusted value.

## Source mappings

P007 `EngineErrorCode` is exhaustively matched. Generic `EffectFailed` gains package or integration precision only when typed stage/resource context proves that classification; otherwise it remains InternalFailure. `PreflightBlocked` requires a typed preflight reason for specific user copy.

P008 `JournalErrorCode` is exhaustively matched. Journal corruption, missing resume evidence, plan mismatch, limit failures, and inspection-required states preserve evidence and require recovery inspection. No action deletes a journal to force progress.

P008 recovery success dispositions Fresh, ResumeReady, ReconciledApplied, and AlreadyCompleted produce no error. ReplanRequired, InspectionRequired, StillUnknown, JournalBusy, and JournalCorrupt have explicit finite presentation behavior.

P009 `LifecycleError` is exhaustively matched. Unsupported source/target/downgrade/intermediate/version paths become UnsupportedLifecycle. Artifact identity mismatch becomes an integrity failure requiring fresh evidence. Protected mutation, unknown ownership, preservation changes, and startup preference drift become ProtectedStateBlocked.

Every P009 `PurgeError` maps fail closed to ProtectedStateBlocked. No purge error can manufacture a force-delete action.

## Platform/preflight bridge

Prompt010 defines typed failures for UnsupportedPlatform, UnsupportedArchitecture, InsufficientDiskSpace, NativePackageTransactionFailed, RuntimeDependencyFailed, IntegrationFailed, and LaunchFailed. P044 emits InsufficientDiskSpace for proven acquisition/journal disk exhaustion before mutation. If journal state may already follow mutation, the same disk-full cause is presented as RecoveryRequired with ReconcileFirst. Later Linux and Windows prompts may emit these facts without adding raw strings to the presentation boundary.

## Diagnostics

The Details diagnostic is typed and bounded:

```text
MAX_DIAGNOSTIC_SERIALIZED_BYTES = 4096
MAX_SAFE_IDENTIFIER_BYTES       = 96
MAX_SAFE_CODE_BYTES             = 64
MAX_SECONDARY_ACTIONS           = 2
```

Allowed fields are a deterministic support reference, category, presentation stage, finite source family/code, optional intent/target scope/resource class, validated effect/artifact IDs, and finite recovery disposition.

The model has no fields for filesystem paths, URLs, commands, exception text, stdout/stderr, environment dumps, credentials, authentication headers, cookies, connection strings, private keys, or legacy free-form evidence. Existing P007 `redacted_evidence` strings are not copied into Prompt010 diagnostics. Unsafe identifiers are omitted rather than echoed.

Support references are deterministic finite values such as `SVE-ENGINE-OUTCOME_UNKNOWN` and `SVE-ACQ-MANIFEST_AUTH_FAILED`. No hostname, username, timestamp, path, or secret is embedded.

There is no telemetry, crash reporting, automatic diagnostic submission, or upload behavior.

## Ordinary reference copy

| Category | Key | English | Tiếng Việt |
|---|---|---|---|
| UnsupportedPlatform | `installer.error.unsupported_platform` | This operating system is not supported for this Synveil installer. | Hệ điều hành này chưa được hỗ trợ bởi bộ cài Synveil này. |
| UnsupportedArchitecture | `installer.error.unsupported_architecture` | This Synveil installer does not match this computer. | Bộ cài Synveil này không phù hợp với kiến trúc của máy. |
| ArtifactUnavailable | `installer.error.artifact_unavailable` | A compatible Synveil package is not available for this setup. | Hiện không có gói Synveil tương thích với cấu hình cài đặt này. |
| InsufficientDiskSpace | `installer.error.insufficient_disk_space` | There is not enough free space to continue installation. | Không đủ dung lượng trống để tiếp tục cài đặt. |
| PermissionRequired | `installer.error.permission_required` | Synveil needs permission from this computer to continue this step. | Synveil cần quyền từ máy tính để tiếp tục bước này. |
| DownloadFailed | `installer.error.download_failed` | The Synveil download did not complete. Check the connection and try the download again. | Tải Synveil chưa hoàn tất. Hãy kiểm tra kết nối rồi thử tải lại. |
| IntegrityVerificationFailed | `installer.error.integrity_verification_failed` | Synveil could not verify this download. Get a fresh official download before continuing. | Synveil không thể xác minh bản tải xuống này. Hãy lấy bản tải chính thức mới trước khi tiếp tục. |
| StoragePreparationFailed | `installer.error.storage_preparation_failed` | Synveil could not safely prepare the installation files. | Synveil không thể chuẩn bị tệp cài đặt một cách an toàn. |
| PackageInstallFailed | `installer.error.package_install_failed` | The Synveil package could not be installed safely. | Gói Synveil không thể được cài đặt an toàn. |
| RuntimeDependencyFailed | `installer.error.runtime_dependency_failed` | A required Synveil component is unavailable. Repair Synveil to continue. | Một thành phần cần thiết của Synveil chưa sẵn sàng. Hãy sửa chữa Synveil để tiếp tục. |
| IntegrationFailed | `installer.error.integration_failed` | Synveil was installed but could not finish computer integration. | Synveil đã được cài nhưng chưa thể hoàn tất tích hợp với máy tính. |
| VerificationFailed | `installer.error.verification_failed` | Synveil could not verify that installation completed correctly. Repair Synveil before using it. | Synveil không thể xác minh việc cài đặt đã hoàn tất đúng cách. Hãy sửa chữa Synveil trước khi sử dụng. |
| Interrupted | `installer.error.interrupted` | Setup was interrupted and needs recovery before continuing. | Quá trình thiết lập đã bị gián đoạn và cần được phục hồi trước khi tiếp tục. |
| RecoveryRequired | `installer.error.recovery_required` | Synveil needs to check the previous setup state before anything is repeated. | Synveil cần kiểm tra trạng thái thiết lập trước đó trước khi lặp lại bất kỳ thao tác nào. |
| ConcurrentOperation | `installer.error.concurrent_operation` | Another Synveil setup or recovery operation is already active. | Một thao tác cài đặt hoặc phục hồi Synveil khác đang hoạt động. |
| UnsupportedLifecycle | `installer.error.unsupported_lifecycle` | This install, repair, upgrade, or removal path is not supported for the current state. | Quy trình cài đặt, sửa chữa, nâng cấp hoặc gỡ cài đặt này không được hỗ trợ với trạng thái hiện tại. |
| ProtectedStateBlocked | `installer.error.protected_state_blocked` | Synveil stopped to protect existing data or settings. | Synveil đã dừng để bảo vệ dữ liệu hoặc cài đặt hiện có. |
| InvalidSetupState | `installer.error.invalid_setup_state` | The current setup state cannot be used safely. Replan this operation before continuing. | Trạng thái thiết lập hiện tại không thể được sử dụng an toàn. Hãy lập lại kế hoạch thao tác trước khi tiếp tục. |
| InternalFailure | `installer.error.internal_failure` | Synveil could not safely continue this setup operation. | Synveil không thể tiếp tục thao tác thiết lập này một cách an toàn. |

The Rust reference-copy table is normative for message-key completeness. Later UI/localization work may improve wording while preserving condition, risk, and safe next action.

## Boundaries

P011 owns stable-channel metadata and compatible version selection. Prompt010 may recommend UseSupportedVersion or ChooseSupportedArtifact but does not choose latest, stable, recommended, or beta releases.

Later platform and first-run prompts own dialog layout, button rendering, icons, accessibility presentation, and localization infrastructure. Prompt010 introduces no Qt dependency.

Product version remains 0.1.0. Server migrations remain 36, client migrations remain 7, and `LOCAL_SCHEMA_VERSION` remains 7.

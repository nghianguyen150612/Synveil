# Synveil v0.2 installer error model

Status: **Normative Prompt010 contract.** This is a platform-neutral policy and
presentation contract, not installer UI. `ERROR_MODEL_SCHEMA_VERSION` is 1.

## Boundary and source families

`synveil-install-engine` maps eight finite families: P007 engine, P008 journal,
P008 recovery disposition, P009 lifecycle, P009 purge, P006 acquisition,
typed preflight, and typed platform failure. Source errors remain authoritative;
the mapper does not rename or replace them. Closed Rust matches are exhaustive.
The P006 bridge contains exactly 19 codes in the sorted machine-readable fixture
`tests/install-error-model/p006-acquisition-codes.json`; AST validation rejects
dynamic Python error codes, and strict Rust parsing rejects unknown strings.

The presentation stage model is `Preflight`, `Plan`, `Acquire`,
`VerifyArtifact`, `Install`, `Integrate`, `VerifyInstallation`, `Complete`,
`Recovery`, and `LifecyclePolicy`. It deliberately does not alter P007 stages.
Typed context may carry intent, scope, resource class, recovery disposition, and
validated effect/artifact identifiers. It cannot carry commands, exceptions,
stderr, environment dumps, paths, or URLs.

## Categories, retry safety, and actions

The 19 categories are UnsupportedPlatform, UnsupportedArchitecture,
ArtifactUnavailable, InsufficientDiskSpace, PermissionRequired, DownloadFailed,
IntegrityVerificationFailed, StoragePreparationFailed, PackageInstallFailed,
RuntimeDependencyFailed, IntegrationFailed, VerificationFailed, Interrupted,
RecoveryRequired, ConcurrentOperation, UnsupportedLifecycle,
ProtectedStateBlocked, InvalidSetupState, and InternalFailure.

Retry safety is independent: `SafeImmediate`, `AfterUserAction`,
`ReconcileFirst`, `FreshEvidenceRequired`, `NotRetryable`, or `NotApplicable`.
Only a known pre-mutation acquisition network failure is immediately retryable.
An unknown mutation outcome, recovery inspection, failed compensation, or
missing resume journal reconciles first; classification never grants replay.

The 17 actions are ChooseSupportedSystem, ChooseSupportedArtifact,
UseSupportedVersion, FreeDiskSpace, RequestPermission, RetryDownload,
GetFreshOfficialDownload, ChooseDifferentDestination,
UseNativePackageRecovery, RepairInstallation, ResumeRecovery, InspectRecovery,
CloseOtherInstaller, Replan, ShowDetails, ContactSupport, and Cancel. There is
no bypass, ignore, force, destructive reset, broad elevation, or blind retry.
Each error has zero or one primary action and at most two secondary actions.

## Exhaustive mapping rules

- **P007:** invalid/unsupported plans replan or use a supported version;
  ownership is protected; privilege requests scoped native authorization;
  stale plans replan. `EffectFailed` becomes package or integration failure only
  with matching typed stage/resource evidence, otherwise InternalFailure.
  verification failures repair. OutcomeUnknown, reconciliation, and failed
  compensation enter recovery and never replay. A preflight block uses a typed
  reason or fails conservatively.
- **P008 journal:** busy/active is ConcurrentOperation. Missing resume state,
  corruption, mismatch, I/O, limits, and inspection preserve evidence and enter
  recovery. Exact unsupported schema uses UnsupportedLifecycle. The journal is
  never deleted automatically.
- **P008 recovery:** Fresh, ResumeReady, ReconciledApplied, and AlreadyCompleted
  produce no error. ReplanRequired, InspectionRequired, StillUnknown,
  JournalBusy, and JournalCorrupt produce bounded recovery guidance.
- **P009 lifecycle:** unsupported source/target, downgrade, intermediate version,
  and version mismatch use UnsupportedLifecycle. Artifact identity mismatch
  requires fresh verified evidence. Ownership, protected mutation, startup, and
  preservation failures are ProtectedStateBlocked. Active/unknown state enters
  concurrency/recovery. No automatic downgrade or data reset exists.
- **P009 purge:** all eight failures are ProtectedStateBlocked and not
  retryable. They offer no deletion, override, or destructive workaround.
- **P006:** NETWORK_ERROR alone permits RetryDownload. Authentication, trust,
  redirect, manifest, digest, truncation, size, commit, and version failures
  require a fresh official download. Unsupported platform/architecture remain
  distinct. No match is unavailable; ambiguity fails internally; destination
  conflict chooses another destination; unsafe path and staging fail closed.
- **Platform/preflight:** unsupported system/architecture and disk space are
  distinct. Native transaction failure uses native recovery; runtime,
  integration, and launch failures use repair without exposing tool output.

## Diagnostics and serialization

`UserFacingError` is a schema-1, deny-unknown-fields record containing category,
stage, stable message key, retry policy, primary/secondary actions, and a typed
Details record. The Details fields are support reference, category, stage,
source family/code, optional intent/scope/resource class, safe effect/artifact
IDs, and finite recovery disposition. Serialization fails if Details exceed
`MAX_DIAGNOSTIC_SERIALIZED_BYTES = 4096`.

Identifiers are at most `MAX_SAFE_IDENTIFIER_BYTES = 96` ASCII bytes and use
only letters, digits, `.`, `-`, `_`, or `:`. Source codes are at most
`MAX_SAFE_CODE_BYTES = 64`; secondary actions are at most
`MAX_SECONDARY_ACTIONS = 2`. Unsafe optional identifiers are omitted by callers,
not truncated. Raw paths, URLs, commands, exception messages, stdout/stderr,
environment data, credentials, headers, cookies, connection strings, private
keys, and pre-existing free-form `redacted_evidence` cannot enter this API.

Support references are deterministic finite values such as
`SVE-ENGINE-OUTCOME_UNKNOWN`, `SVE-ACQ-MANIFEST_AUTH_FAILED`, and
`SVE-JOURNAL-CORRUPT`. They contain no host, user, time, path, URL, or secret.
Diagnostics stay local: there is no telemetry, upload, crash report, or support
submission.

## Ordinary reference copy (English / Vietnamese)

These semantic references feed later localization. Every row communicates the
same condition, risk, and safe action in both languages. Details are a separate
bounded surface.

<!-- ORDINARY_COPY_START -->
| Message key | English | Tiếng Việt |
| --- | --- | --- |
| `installer.error.unsupported_platform` | This system is not supported. Choose a supported system. | Hệ thống này không được hỗ trợ. Hãy chọn hệ thống được hỗ trợ. |
| `installer.error.unsupported_architecture` | This device architecture is not supported. Choose the matching artifact. | Kiến trúc thiết bị này không được hỗ trợ. Hãy chọn gói cài đặt phù hợp. |
| `installer.error.artifact_unavailable` | The requested installer is unavailable. Choose a supported artifact. | Không có bộ cài đặt được yêu cầu. Hãy chọn gói cài đặt được hỗ trợ. |
| `installer.error.insufficient_disk_space` | There is not enough free space. Free space before continuing. | Không đủ dung lượng trống. Hãy giải phóng dung lượng trước khi tiếp tục. |
| `installer.error.permission_required` | The computer needs permission for this installation step. Use the scoped system prompt. | Máy tính cần quyền cho bước cài đặt này. Hãy dùng lời nhắc cấp quyền có giới hạn của hệ thống. |
| `installer.error.download_failed` | Download failed. Check the connection and try the download again. | Tải xuống thất bại. Hãy kiểm tra kết nối và thử tải lại. |
| `installer.error.integrity_verification_failed` | The download could not be verified. Get a fresh official download; do not use these bytes. | Không thể xác minh bản tải xuống. Hãy lấy bản tải chính thức mới; không sử dụng dữ liệu này. |
| `installer.error.storage_preparation_failed` | The installer could not safely prepare storage. Choose another destination or view details. | Bộ cài đặt không thể chuẩn bị vùng lưu trữ an toàn. Hãy chọn đích khác hoặc xem chi tiết. |
| `installer.error.package_install_failed` | Package installation did not finish. Use system package recovery or repair Synveil. | Cài đặt gói chưa hoàn tất. Hãy dùng phục hồi gói của hệ thống hoặc sửa chữa Synveil. |
| `installer.error.runtime_dependency_failed` | A required runtime component is unavailable. Repair Synveil. | Một thành phần chạy bắt buộc chưa sẵn sàng. Hãy sửa chữa Synveil. |
| `installer.error.integration_failed` | System integration did not finish. Repair Synveil without changing unrelated preferences. | Tích hợp hệ thống chưa hoàn tất. Hãy sửa chữa Synveil mà không đổi tùy chọn không liên quan. |
| `installer.error.verification_failed` | Installation is incomplete because verification failed. Repair Synveil. | Cài đặt chưa hoàn tất vì xác minh thất bại. Hãy sửa chữa Synveil. |
| `installer.error.interrupted` | Setup was interrupted. Inspect the saved setup state before continuing. | Thiết lập đã bị gián đoạn. Hãy kiểm tra trạng thái đã lưu trước khi tiếp tục. |
| `installer.error.recovery_required` | Synveil must inspect or recover setup state before another attempt. Do not repeat the operation blindly. | Synveil phải kiểm tra hoặc phục hồi trạng thái thiết lập trước lần thử khác. Không lặp lại thao tác một cách mù quáng. |
| `installer.error.concurrent_operation` | Another setup operation is active. Close the other installer before continuing. | Một thao tác thiết lập khác đang hoạt động. Hãy đóng bộ cài đặt kia trước khi tiếp tục. |
| `installer.error.unsupported_lifecycle` | This install, upgrade, repair, or removal path is unsupported. Use a supported version. | Quy trình cài đặt, nâng cấp, sửa chữa hoặc gỡ bỏ này không được hỗ trợ. Hãy dùng phiên bản được hỗ trợ. |
| `installer.error.protected_state_blocked` | Setup stopped to protect data or settings it cannot safely change. Contact support. | Thiết lập đã dừng để bảo vệ dữ liệu hoặc cài đặt không thể thay đổi an toàn. Hãy liên hệ hỗ trợ. |
| `installer.error.invalid_setup_state` | Setup state is no longer valid. Create a new plan or view details. | Trạng thái thiết lập không còn hợp lệ. Hãy tạo kế hoạch mới hoặc xem chi tiết. |
| `installer.error.internal_failure` | Setup stopped safely because the failure could not be classified more precisely. View details or contact support. | Thiết lập đã dừng an toàn vì không thể phân loại lỗi chính xác hơn. Hãy xem chi tiết hoặc liên hệ hỗ trợ. |
<!-- ORDINARY_COPY_END -->

## Safety validator and later boundaries

The central validator rejects wrong schema, excess actions, immediate integrity
retry, non-reconciling recovery, recovery download retry, incorrect permission
action, and oversized diagnostics. P011 alone selects a channel or decides
which version is current, stable, beta, or recommended. Later platform and
first-run prompts render dialogs, buttons, icons, accessibility, and localization.
This contract performs no retry, package recovery, launch, mutation, or UI work.

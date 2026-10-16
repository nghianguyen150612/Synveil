import Foundation

enum ClientRecoveryClassification: String, Equatable, Sendable {
    case temporarilyOffline, serverUnavailable, authenticationRequired, deviceRevoked
    case permissionDenied, protocolInvalid, cachedDataIncomplete, cachedDataStale
    case reconciliationRequired, conflictRequiresReview, rebaselineRequired
    case localStorageFailure, integrityFailure, cancelled, sessionInvalidated
    case transferInterrupted, verificationRequired

    var title: String {
        switch self {
        case .temporarilyOffline: "Connection interrupted"
        case .serverUnavailable: "Server temporarily unavailable"
        case .authenticationRequired: "Authorization recovery required"
        case .deviceRevoked: "Device access revoked"
        case .permissionDenied: "Access denied"
        case .protocolInvalid: "Response could not be verified"
        case .cachedDataIncomplete: "Partial saved metadata"
        case .cachedDataStale: "Saved metadata — not verified current"
        case .reconciliationRequired: "Outcome requires reconciliation"
        case .conflictRequiresReview: "Conflict requires review"
        case .rebaselineRequired: "Saved metadata recovery required"
        case .localStorageFailure: "Local storage recovery required"
        case .integrityFailure: "Integrity verification failed"
        case .cancelled: "Operation cancelled"
        case .sessionInvalidated: "Session changed"
        case .transferInterrupted: "Download interrupted"
        case .verificationRequired: "Verification required"
        }
    }

    var symbol: String {
        switch self {
        case .temporarilyOffline, .serverUnavailable: "wifi.exclamationmark"
        case .cachedDataIncomplete, .cachedDataStale: "externaldrive"
        case .authenticationRequired, .deviceRevoked, .permissionDenied, .sessionInvalidated:
            "lock.shield"
        case .cancelled, .transferInterrupted: "pause.circle"
        case .verificationRequired: "checkmark.shield"
        default: "exclamationmark.triangle"
        }
    }
}

enum ClientRecoveryEvidence: Equatable, Sendable {
    case transientRequest, durableOperation, committedProjection, snapshotProjection,
        previouslyLoaded
}

/// Presentation suggestions only. Feature coordinators revalidate session, scope, durable state,
/// locks, limits and confirmations on every invocation. No generic retry executor exists.
enum ClientRecoveryAction: Equatable, Sendable {
    case retryMetadataRead, resumeDownload, checkUploadStatus, resumeUpload
    case waitForBackgroundTask, reconcileOriginalMutation, recoverOriginalAcknowledgement
    case reviewConflict, confirmedRebaseline, recoverSession, inspectLocalStatus
}

struct ClientRecoveryPresentation: Equatable, Sendable {
    let classification: ClientRecoveryClassification
    let evidence: ClientRecoveryEvidence
    let message: String
    let actions: [ClientRecoveryAction]

    static func classification(_ failure: NodeFailure) -> ClientRecoveryClassification {
        switch failure {
        case .offline, .dnsFailure, .timeout: .temporarilyOffline
        case .serverUnavailable: .serverUnavailable
        case .unauthenticated, .credentialUnavailable, .invalidCredential, .authenticationRejected:
            .authenticationRequired
        case .deviceRevoked: .deviceRevoked
        case .staleSession, .originMismatch: .sessionInvalidated
        case .cancelled: .cancelled
        case .httpFailure(let code):
            code == 403
                ? .permissionDenied
                : ((500...599).contains(code) ? .serverUnavailable : .protocolInvalid)
        case .tlsFailure, .redirectRejected, .unexpectedContentType, .protocolFailure,
            .resourceLimit, .repeatedCursor:
            .protocolInvalid
        }
    }

    static func saved(_ saved: NodeCachedDirectoryPresentation) -> Self {
        let recovery =
            saved.knowledge == .staleKnown || saved.projection.completeness == .rebaselineRequired
        let partial = saved.knowledge != .complete || saved.hasMore
        let source: ClientRecoveryEvidence =
            saved.snapshotNodes.isEmpty && saved.projection.completeness != .complete
            ? .committedProjection : .snapshotProjection
        return Self(
            classification: recovery
                ? .rebaselineRequired : (partial ? .cachedDataIncomplete : .cachedDataStale),
            evidence: source,
            message: recovery
                ? "Saved metadata needs synchronization recovery. Its journal evidence is retained."
                : (partial
                    ? "This saved listing cannot prove all items or their absence. Other server items may exist."
                    : "The saved folder structure is complete at its snapshot cut. Current server state has not been verified."),
            actions: recovery ? [.confirmedRebaseline] : [.retryMetadataRead])
    }

    static func mutation(_ state: MutationQueueState) -> Self? {
        switch state {
        case .submitting, .outcomeUnknown:
            Self(
                classification: .reconciliationRequired, evidence: .durableOperation,
                message:
                    "The original operation may have reached the server. Its identity and request are retained.",
                actions: state == .outcomeUnknown ? [.reconcileOriginalMutation] : [])
        case .conflict:
            Self(
                classification: .conflictRequiresReview, evidence: .durableOperation,
                message: "Review the server conflict before creating another change.",
                actions: [.reviewConflict])
        case .blockedRebaseline:
            Self(
                classification: .rebaselineRequired, evidence: .durableOperation,
                message:
                    "Original queued changes remain saved. Synchronization recovery must respect their existing gates.",
                actions: [.inspectLocalStatus])
        case .pending, .applied, .failedPermanent: nil
        }
    }

    static func sync(_ reason: InboundSyncStopReason?, recovery: SyncRecoveryAction?) -> Self? {
        if let recovery {
            return Self(
                classification: .reconciliationRequired, evidence: .durableOperation,
                message:
                    "The original acknowledgment is unconfirmed. Applied metadata and signed evidence remain saved.",
                actions: recovery.canRecover
                    ? [.recoverOriginalAcknowledgement] : [.inspectLocalStatus])
        }
        let kind: ClientRecoveryClassification
        switch reason {
        case .offline: kind = .temporarilyOffline
        case .serverUnavailable: kind = .serverUnavailable
        case .authenticationRequired: kind = .authenticationRequired
        case .deviceRevoked: kind = .deviceRevoked
        case .rebaselineRequired: kind = .rebaselineRequired
        case .awaitingAckRecovery, .reconciliationRequired, .recoveryLimitReached,
            .metadataChanged, .missingMaterialization:
            kind = .reconciliationRequired
        case .storageFailure: kind = .localStorageFailure
        case .protocolFailure, .invalidConfiguration: kind = .protocolInvalid
        case .scopeMismatch, .committedButSessionChanged: kind = .sessionInvalidated
        case .cancelled: kind = .cancelled
        default: return nil
        }
        return Self(
            classification: kind, evidence: .durableOperation,
            message:
                "Downloaded, locally applied and server-confirmed progress remain distinct. Inspect saved status before continuing.",
            actions: [.inspectLocalStatus])
    }

    static func upload(_ record: UploadRecord, background: BackgroundUploadJob?) -> Self? {
        if background?.blocksForeground == true {
            return Self(
                classification: .reconciliationRequired, evidence: .durableOperation,
                message:
                    "Background transfer still owns this chunk. Foreground Resume waits for task resolution and authoritative server status.",
                actions: [.waitForBackgroundTask, .checkUploadStatus])
        }
        if record.uncertain || [.outcomeUnknown, .completing, .abortPending].contains(record.state)
        {
            return Self(
                classification: .reconciliationRequired, evidence: .durableOperation,
                message:
                    "Server acceptance or publication is unconfirmed. Check the original upload session; accepted bytes alone do not prove publication.",
                actions: record.serverSessionId == nil ? [.resumeUpload] : [.checkUploadStatus])
        }
        guard let failure = record.failure else { return nil }
        let kind: ClientRecoveryClassification
        switch failure {
        case .offline, .dnsFailure, .timeout: kind = .temporarilyOffline
        case .serverUnavailable, .capacityUnavailable: kind = .serverUnavailable
        case .unauthenticated: kind = .authenticationRequired
        case .deviceRevoked: kind = .deviceRevoked
        case .scopeMismatch, .staleSession: kind = .sessionInvalidated
        case .revisionConflict, .nameConflict, .conflict: kind = .conflictRequiresReview
        case .permissionDenied: kind = .permissionDenied
        case .integrityMismatch, .sourceChanged: kind = .integrityFailure
        case .storageFailure, .filesystemFailure, .insufficientStorage, .stagingQuotaExceeded,
            .stagingMissing:
            kind = .localStorageFailure
        case .cancelled: kind = .cancelled
        default: kind = .protocolInvalid
        }
        return Self(
            classification: kind, evidence: .durableOperation, message: failure.message,
            actions: record.state.canResume ? [.resumeUpload] : [])
    }

    static func download(_ failure: DownloadFailure) -> Self {
        let kind: ClientRecoveryClassification
        switch failure {
        case .offline, .dnsFailure, .timeout: kind = .temporarilyOffline
        case .serverUnavailable: kind = .serverUnavailable
        case .unauthenticated: kind = .authenticationRequired
        case .deviceRevoked: kind = .deviceRevoked
        case .scopeMismatch, .staleSession: kind = .sessionInvalidated
        case .sha256Mismatch, .integrityFailure, .etagMismatch: kind = .integrityFailure
        case .permissionDenied: kind = .permissionDenied
        case .insufficientStorage, .stagingQuotaExceeded, .stagingMissing, .filesystemFailure,
            .storageFailure:
            kind = .localStorageFailure
        case .cancelled: kind = .cancelled
        default: kind = .protocolInvalid
        }
        return Self(
            classification: kind, evidence: .durableOperation, message: failure.message,
            actions: failure.state.canResume ? [.resumeDownload] : [])
    }

    /// Presentation for durable foreground download states recovered after interruption. A
    /// partial checkpoint is never described as an offline file or as verified content.
    static func download(_ progress: DownloadProgress, isActive: Bool) -> Self? {
        guard !isActive else { return nil }
        let classification: ClientRecoveryClassification
        let message: String
        switch progress.state {
        case .prepared, .downloading, .partial, .cancelled, .failedRecoverable:
            classification = .transferInterrupted
            if progress.durableBytes == 0 {
                message =
                    "No download bytes were committed. Choose Resume to revalidate this pinned version."
            } else {
                message =
                    "Saved progress covers \(progress.durableBytes) of \(progress.totalBytes) bytes. The partial file is not available offline; Resume revalidates this pinned version."
            }
        case .verifying:
            classification = .verificationRequired
            message =
                "Verification did not finish. This file is unavailable until integrity checks complete."
        default:
            return nil
        }
        return Self(
            classification: classification, evidence: .durableOperation,
            message: message,
            actions: progress.state.canResume ? [.resumeDownload] : [.inspectLocalStatus])
    }

    static func rebaseline(_ progress: RebaselineProgress?, failure: RebaselineFailure?) -> Self? {
        if let progress,
            [.startUnknown, .completionInFlight, .outcomeUnknown].contains(progress.state)
        {
            return Self(
                classification: .reconciliationRequired, evidence: .durableOperation,
                message:
                    "The original snapshot handoff is unresolved. Its manifest identity and completion evidence remain saved. Recovery requires the existing explicit confirmation.",
                actions: [.confirmedRebaseline])
        }
        if progress?.state == .completionConfirmed {
            return Self(
                classification: .rebaselineRequired, evidence: .durableOperation,
                message:
                    "The server handoff is confirmed. The prepared projection still requires atomic local installation.",
                actions: [.confirmedRebaseline])
        }
        guard let failure else { return nil }
        let kind: ClientRecoveryClassification
        switch failure {
        case .storage, .capacity: kind = .localStorageFailure
        case .staleSession, .scopeMismatch: kind = .sessionInvalidated
        case .cancelled: kind = .cancelled
        case .incompatibleSync, .incompatibleMutations, .reconciliationRequired:
            kind = .reconciliationRequired
        case .transport(let transport):
            switch transport {
            case .offline, .dnsFailure, .timeout: kind = .temporarilyOffline
            case .serverUnavailable: kind = .serverUnavailable
            case .unauthenticated, .authenticationRejected, .credentialUnavailable,
                .invalidCredential:
                kind = .authenticationRequired
            case .deviceRevoked: kind = .deviceRevoked
            case .httpFailure(let code): kind = code == 403 ? .permissionDenied : .protocolInvalid
            default: kind = .protocolInvalid
            }
        default: kind = .protocolInvalid
        }
        return Self(
            classification: kind, evidence: .durableOperation,
            message:
                "The previous projection and unresolved recovery evidence are retained. No synchronization success has been assumed.",
            actions: [.inspectLocalStatus])
    }
}

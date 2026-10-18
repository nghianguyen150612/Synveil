import Foundation

public enum PhotoBackupQueueState: String, Codable, Sendable, Equatable, Hashable, CaseIterable {
    case discovered = "DISCOVERED"
    case eligible = "ELIGIBLE"
    case queued = "QUEUED"
    case claimed = "CLAIMED"
    case extracting = "EXTRACTING"
    case verifiedLocally = "VERIFIED_LOCALLY"
    case uploading = "UPLOADING"
    case reconciliationPending = "RECONCILIATION_PENDING"
    case fileUploadedAwaitingPhotoBinding = "FILE_UPLOADED_AWAITING_PHOTO_BINDING"
    case photoBindingConfirmed = "PHOTO_BINDING_CONFIRMED"
    case awaitingBackupRetentionProof = "AWAITING_BACKUP_RETENTION_PROOF"
    case authoritativeBackupConfirmed = "AUTHORITATIVE_BACKUP_CONFIRMED"
    case partial = "PARTIAL"
    case sourceUnavailable = "SOURCE_UNAVAILABLE"
    case failedRecoverable = "FAILED_RECOVERABLE"
    case failedTerminal = "FAILED_TERMINAL"
    case cancelled = "CANCELLED"
}

public enum PhotoBackupSafeFailure: String, Codable, Sendable, Equatable {
    case permissionRevoked
    case limitedSelectionChanged
    case iCloudOriginalUnavailable
    case sourceChanged
    case deviceStorageFull
    case stagingCorrupt
    case networkUnavailable
    case uploadSessionConflict
    case uploadOutcomeUnknown
    case photoBindingUnavailable
    case backupRetentionUnavailable
    case policyChanged
    case destinationUnavailable
    case queueCapacityReached
}

public enum PhotoBackupLivePhotoGroupState: String, Codable, Sendable, Equatable, CaseIterable {
    case pending = "PENDING"
    case partial = "PARTIAL"
    case resourcesUploadedUnbound = "RESOURCES_UPLOADED_UNBOUND"
    case bindingPending = "BINDING_PENDING"
    case bindingConfirmed = "BINDING_CONFIRMED"
    case backupConfirmed = "BACKUP_CONFIRMED"
    case failedRecoverable = "FAILED_RECOVERABLE"
}

/// One durable, owner and device scoped PhotoKit resource operation. Destination and policy
/// generation are immutable once inserted so a later settings edit cannot retarget its P045 row.
public struct PhotoBackupCandidate: Codable, Sendable, Equatable, Identifiable {
    public var id: UUID { candidateId }

    public let candidateId: UUID
    public let profileId: UUID
    public let ownerId: String
    public let deviceId: String
    public let clientLibraryScope: String
    public let localIdentifier: String
    public let sourceRevision: String
    public let selection: PhotoResourceSelection
    public let observationGeneration: Int64
    public let policyRevision: Int64
    public let authorizationGeneration: UUID
    public let destinationLibraryIdentifier: String
    public let destinationParentIdentifier: String
    public let groupIdentity: String?
    public let state: PhotoBackupQueueState
    public let attemptCount: Int
    public let retryAfter: Date?
    public let claimId: UUID?
    public let claimExpiresAt: Date?
    public let claimPreviousState: PhotoBackupQueueState?
    public let extractionOperationId: UUID?
    public let uploadOperationId: String?
    public let fileVersionIdentifier: String?
    public let verifiedByteCount: UInt64?
    public let verifiedSHA256: String?
    public let photoAssetIdentifier: String?
    public let backupSnapshotIdentifier: String?
    public let backupRetentionConfirmedAt: Date?
    public let safeFailure: PhotoBackupSafeFailure?
    public let createdAt: Date
    public let updatedAt: Date

    public init(
        candidateId: UUID = UUID(),
        profileId: UUID,
        ownerId: String,
        deviceId: String,
        clientLibraryScope: String,
        localIdentifier: String,
        sourceRevision: String,
        selection: PhotoResourceSelection,
        observationGeneration: Int64,
        policyRevision: Int64,
        authorizationGeneration: UUID,
        destinationLibraryIdentifier: String,
        destinationParentIdentifier: String,
        groupIdentity: String? = nil,
        state: PhotoBackupQueueState = .queued,
        attemptCount: Int = 0,
        retryAfter: Date? = nil,
        claimId: UUID? = nil,
        claimExpiresAt: Date? = nil,
        claimPreviousState: PhotoBackupQueueState? = nil,
        extractionOperationId: UUID? = nil,
        uploadOperationId: String? = nil,
        fileVersionIdentifier: String? = nil,
        verifiedByteCount: UInt64? = nil,
        verifiedSHA256: String? = nil,
        photoAssetIdentifier: String? = nil,
        backupSnapshotIdentifier: String? = nil,
        backupRetentionConfirmedAt: Date? = nil,
        safeFailure: PhotoBackupSafeFailure? = nil,
        createdAt: Date = Date(),
        updatedAt: Date = Date()
    ) {
        self.candidateId = candidateId
        self.profileId = profileId
        self.ownerId = ownerId
        self.deviceId = deviceId
        self.clientLibraryScope = clientLibraryScope
        self.localIdentifier = localIdentifier
        self.sourceRevision = sourceRevision
        self.selection = selection
        self.observationGeneration = observationGeneration
        self.policyRevision = max(policyRevision, 1)
        self.authorizationGeneration = authorizationGeneration
        self.destinationLibraryIdentifier = destinationLibraryIdentifier
        self.destinationParentIdentifier = destinationParentIdentifier
        self.groupIdentity = groupIdentity
        self.state = state
        self.attemptCount = max(attemptCount, 0)
        self.retryAfter = retryAfter
        self.claimId = claimId
        self.claimExpiresAt = claimExpiresAt
        self.claimPreviousState = claimPreviousState
        self.extractionOperationId = extractionOperationId
        self.uploadOperationId = uploadOperationId
        self.fileVersionIdentifier = fileVersionIdentifier
        self.verifiedByteCount = verifiedByteCount
        self.verifiedSHA256 = verifiedSHA256
        self.photoAssetIdentifier = photoAssetIdentifier
        self.backupSnapshotIdentifier = backupSnapshotIdentifier
        self.backupRetentionConfirmedAt = backupRetentionConfirmedAt
        self.safeFailure = safeFailure
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    public func replacing(
        state: PhotoBackupQueueState,
        claimId: UUID? = nil,
        claimExpiresAt: Date? = nil,
        claimPreviousState: PhotoBackupQueueState? = nil,
        attemptCount: Int? = nil,
        retryAfter: Date? = nil,
        preserveRetryAfter: Bool = true,
        extractionOperationId: UUID? = nil,
        uploadOperationId: String? = nil,
        fileVersionIdentifier: String? = nil,
        verifiedByteCount: UInt64? = nil,
        verifiedSHA256: String? = nil,
        photoAssetIdentifier: String? = nil,
        backupSnapshotIdentifier: String? = nil,
        backupRetentionConfirmedAt: Date? = nil,
        safeFailure: PhotoBackupSafeFailure? = nil,
        updatedAt: Date = Date()
    ) -> PhotoBackupCandidate {
        PhotoBackupCandidate(
            candidateId: candidateId, profileId: profileId, ownerId: ownerId,
            deviceId: deviceId, clientLibraryScope: clientLibraryScope,
            localIdentifier: localIdentifier, sourceRevision: sourceRevision,
            selection: selection, observationGeneration: observationGeneration,
            policyRevision: policyRevision, authorizationGeneration: authorizationGeneration,
            destinationLibraryIdentifier: destinationLibraryIdentifier,
            destinationParentIdentifier: destinationParentIdentifier,
            groupIdentity: groupIdentity, state: state,
            attemptCount: attemptCount ?? self.attemptCount,
            retryAfter: preserveRetryAfter ? (retryAfter ?? self.retryAfter) : retryAfter,
            claimId: claimId,
            claimExpiresAt: claimExpiresAt, claimPreviousState: claimPreviousState,
            extractionOperationId: extractionOperationId
                ?? self.extractionOperationId,
            uploadOperationId: uploadOperationId ?? self.uploadOperationId,
            fileVersionIdentifier: fileVersionIdentifier ?? self.fileVersionIdentifier,
            verifiedByteCount: verifiedByteCount ?? self.verifiedByteCount,
            verifiedSHA256: verifiedSHA256 ?? self.verifiedSHA256,
            photoAssetIdentifier: photoAssetIdentifier ?? self.photoAssetIdentifier,
            backupSnapshotIdentifier: backupSnapshotIdentifier ?? self.backupSnapshotIdentifier,
            backupRetentionConfirmedAt: backupRetentionConfirmedAt
                ?? self.backupRetentionConfirmedAt,
            safeFailure: safeFailure, createdAt: createdAt, updatedAt: updatedAt)
    }
}

public struct PhotoBackupQueueSummary: Sendable, Equatable {
    public let counts: [PhotoBackupQueueState: Int]

    public init(counts: [PhotoBackupQueueState: Int] = [:]) {
        self.counts = counts
    }

    public func count(for state: PhotoBackupQueueState) -> Int { counts[state, default: 0] }

    public var queuedCount: Int {
        count(for: .queued) + count(for: .eligible) + count(for: .claimed)
    }
    public var extractingCount: Int { count(for: .extracting) + count(for: .verifiedLocally) }
    public var uploadingCount: Int { count(for: .uploading) + count(for: .reconciliationPending) }
    public var uploadedAwaitingBindingCount: Int {
        count(for: .fileUploadedAwaitingPhotoBinding)
    }
    public var canonicalFileUploadedCount: Int {
        count(for: .fileUploadedAwaitingPhotoBinding) + count(for: .photoBindingConfirmed)
            + count(for: .awaitingBackupRetentionProof) + count(for: .authoritativeBackupConfirmed)
            + count(for: .partial)
    }
    public var backupConfirmedCount: Int { count(for: .authoritativeBackupConfirmed) }
    public var partialCount: Int { count(for: .partial) }
    public var failedCount: Int { count(for: .failedRecoverable) + count(for: .failedTerminal) }
    public var unavailableCount: Int { count(for: .sourceUnavailable) }
}

public struct PhotoBackupLivePhotoGroupEvidence: Sendable, Equatable, Identifiable {
    public let id: String
    public let state: PhotoBackupLivePhotoGroupState
    public let stillState: PhotoBackupQueueState?
    public let motionState: PhotoBackupQueueState?

    public init(groupIdentity: String, candidates: [PhotoBackupCandidate]) {
        id = groupIdentity
        let stillStates = candidates.filter { $0.selection.role == .primaryImage }.map(\.state)
        let motionStates = candidates.filter { $0.selection.role == .motionVideo }.map(\.state)
        stillState = Self.summarize(stillStates)
        motionState = Self.summarize(motionStates)
        let componentStates = stillStates + motionStates
        if stillStates.isEmpty || motionStates.isEmpty {
            state = componentStates.contains(where: Self.isUploaded) ? .partial : .pending
        } else if componentStates.allSatisfy({ $0 == .authoritativeBackupConfirmed }) {
            state = .backupConfirmed
        } else if componentStates.allSatisfy({ $0 == .photoBindingConfirmed }) {
            state = .bindingConfirmed
        } else if componentStates.allSatisfy({
            [.photoBindingConfirmed, .awaitingBackupRetentionProof].contains($0)
        }) {
            state = .bindingConfirmed
        } else if componentStates.contains(.partial)
            || componentStates.contains(where: {
                [.sourceUnavailable, .failedRecoverable, .failedTerminal].contains($0)
            }), componentStates.contains(where: Self.isUploaded)
        {
            state = .partial
        } else if componentStates.allSatisfy({ Self.isUploaded($0) }) {
            state = .resourcesUploadedUnbound
        } else if componentStates.contains(where: {
            [.failedRecoverable, .failedTerminal].contains($0)
        }) {
            state = .failedRecoverable
        } else {
            state = .pending
        }
    }

    private static func summarize(_ states: [PhotoBackupQueueState]) -> PhotoBackupQueueState? {
        guard !states.isEmpty else { return nil }
        if states.allSatisfy({ $0 == .authoritativeBackupConfirmed }) {
            return .authoritativeBackupConfirmed
        }
        if states.contains(where: {
            [.sourceUnavailable, .failedRecoverable, .failedTerminal].contains($0)
        }) {
            return states.first(where: {
                [.sourceUnavailable, .failedRecoverable, .failedTerminal].contains($0)
            })
        }
        if states.contains(.partial) { return .partial }
        if states.allSatisfy({ Self.isUploaded($0) }) {
            return states.contains(.fileUploadedAwaitingPhotoBinding)
                ? .fileUploadedAwaitingPhotoBinding : .photoBindingConfirmed
        }
        return states.first
    }

    private static func isUploaded(_ state: PhotoBackupQueueState) -> Bool {
        [
            .fileUploadedAwaitingPhotoBinding, .photoBindingConfirmed,
            .awaitingBackupRetentionProof, .authoritativeBackupConfirmed, .partial,
        ].contains(state)
    }
}

public enum PhotoBackupReconciliationResult: Sendable, Equatable {
    case fileVersionConfirmed(identifier: String)
    case stillUploading
    case outcomeUnknown
    case bindingPending
    case photoBindingConfirmed(identifier: String)
    case partial
    case photoBindingUnavailable
    case retentionProofUnavailable
    case authoritativeBackupConfirmed(snapshotIdentifier: String)
}

public enum PhotoImportBindingAvailability: String, Sendable, Equatable {
    case implementedVerified
    case unavailableServerContract
}

public struct PhotoBackupAuthenticatedIdentity: Sendable, Equatable {
    public let profileId: UUID
    public let ownerId: String
    public let deviceId: String
    public let sessionRevision: UInt64

    public init(profileId: UUID, ownerId: String, deviceId: String, sessionRevision: UInt64) {
        self.profileId = profileId
        self.ownerId = ownerId
        self.deviceId = deviceId
        self.sessionRevision = sessionRevision
    }
}

@MainActor
public protocol PhotoBackupAuthenticatedIdentityProviding: Sendable {
    func currentIdentity() async -> PhotoBackupAuthenticatedIdentity?
}

public protocol PhotoImportBindingRepositoryProtocol: Sendable {
    var availability: PhotoImportBindingAvailability { get }
    func bind(_ candidate: PhotoBackupCandidate) async throws -> PhotoBackupReconciliationResult
}

public enum PhotoBackupRetentionAvailability: String, Sendable, Equatable {
    case implementedVerified
    case unavailableServerContract
}

public protocol PhotoBackupRetentionRepositoryProtocol: Sendable {
    var availability: PhotoBackupRetentionAvailability { get }
    func confirmRetention(for candidate: PhotoBackupCandidate) async
        -> PhotoBackupReconciliationResult
}

/// P058 deliberately has no guessed server request. Uploaded files remain unbound until the
/// server publishes and reviews an authoritative PhotoAsset import contract.
public struct UnavailablePhotoImportBindingRepository: PhotoImportBindingRepositoryProtocol {
    public let availability: PhotoImportBindingAvailability = .unavailableServerContract

    public init() {}

    public func bind(_ candidate: PhotoBackupCandidate) async throws
        -> PhotoBackupReconciliationResult
    {
        .photoBindingUnavailable
    }
}

public struct UnavailablePhotoBackupRetentionRepository: PhotoBackupRetentionRepositoryProtocol {
    public let availability: PhotoBackupRetentionAvailability = .unavailableServerContract

    public init() {}

    public func confirmRetention(for candidate: PhotoBackupCandidate) async
        -> PhotoBackupReconciliationResult
    {
        .retentionProofUnavailable
    }
}

public protocol PhotoBackupQueueStoreProtocol: Sendable {
    func backupDiscoveryCursor(
        authorization: PhotoAuthorizationSession, deviceId: String, policyRevision: Int64,
        scanGeneration: Int64
    ) async throws -> String?
    func saveBackupDiscoveryCursor(
        _ localIdentifier: String?, authorization: PhotoAuthorizationSession,
        deviceId: String, policyRevision: Int64, scanGeneration: Int64
    ) async throws
    func fetchObservationPage(
        profileId: UUID, ownerId: String, scanGeneration: Int64,
        after localIdentifier: String?, limit: Int
    ) async throws -> [PhotoAssetObservation]
    func enqueueBackupCandidates(
        _ candidates: [PhotoBackupCandidate], authorization: PhotoAuthorizationSession,
        maximumPending: Int
    ) async throws -> Int
    func claimNextBackupCandidate(
        authorization: PhotoAuthorizationSession, deviceId: String, claimId: UUID,
        leaseUntil: Date, policyRevision: Int64
    ) async throws -> PhotoBackupCandidate?
    func transitionBackupCandidate(
        _ candidate: PhotoBackupCandidate, expectedState: PhotoBackupQueueState,
        authorization: PhotoAuthorizationSession
    ) async throws
    func backupCandidates(
        authorization: PhotoAuthorizationSession, limit: Int
    ) async throws -> [PhotoBackupCandidate]
    func adoptRecoverableBackupCandidates(
        authorization: PhotoAuthorizationSession, deviceId: String
    ) async throws
    func backupQueueSummary(authorization: PhotoAuthorizationSession) async throws
        -> PhotoBackupQueueSummary
    func recoverExpiredBackupClaims(
        authorization: PhotoAuthorizationSession, before: Date
    ) async throws
}

public extension PhotoBackupCandidate {
    static func sourceRevision(for observation: PhotoAssetObservation) -> String {
        if let modificationDate = observation.modificationDate {
            let milliseconds = Int64((modificationDate.timeIntervalSince1970 * 1_000).rounded())
            return "modified-ms=\(milliseconds)"
        }
        let creationMilliseconds =
            observation.creationDate.map {
                Int64(($0.timeIntervalSince1970 * 1_000).rounded())
            }.map { String($0) } ?? "unknown"
        return "fallback=\(observation.assetKind.rawValue):\(creationMilliseconds):"
            + "\(observation.pixelWidth)x\(observation.pixelHeight)"
    }
}

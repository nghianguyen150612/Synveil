import Foundation

public struct PhotoDiscoveryContinuation: Sendable, Equatable, Codable {
    public let generation: Int64
    public let snapshotId: UUID
    public let nextOffset: Int
    public let totalInScope: Int

    public init(generation: Int64, snapshotId: UUID, nextOffset: Int, totalInScope: Int) {
        self.generation = generation
        self.snapshotId = snapshotId
        self.nextOffset = nextOffset
        self.totalInScope = totalInScope
    }
}

public struct PhotoAuthorizationSession: Sendable, Equatable, Codable {
    public let profileId: UUID
    public let ownerId: String
    public let generation: UUID
    public let sessionRevision: UInt64

    public init(
        profileId: UUID, ownerId: String, generation: UUID = UUID(), sessionRevision: UInt64 = 0
    ) {
        self.profileId = profileId
        self.ownerId = ownerId
        self.generation = generation
        self.sessionRevision = sessionRevision
    }
}

/// Stable only within one authorized device library. The ordinal disambiguates resources that
/// share a role, type, and filename; filenames are never treated as identifiers.
public struct PhotoResourceSelection: Sendable, Equatable, Codable, Identifiable {
    public var id: String {
        "\(localIdentifier):\(resourceIndex):\(resourceType):\(rendition.rawValue)"
    }

    public let localIdentifier: String
    public let resourceIndex: Int
    public let resourceType: String
    public let role: PhotoResourceRole
    public let rendition: PhotoResourceRendition
    public let uniformTypeIdentifier: String?
    public let sourceModificationDate: Date?

    public init(
        localIdentifier: String,
        resourceIndex: Int,
        resourceType: String,
        role: PhotoResourceRole,
        rendition: PhotoResourceRendition,
        uniformTypeIdentifier: String?,
        sourceModificationDate: Date?
    ) {
        self.localIdentifier = localIdentifier
        self.resourceIndex = resourceIndex
        self.resourceType = resourceType
        self.role = role
        self.rendition = rendition
        self.uniformTypeIdentifier = uniformTypeIdentifier
        self.sourceModificationDate = sourceModificationDate
    }
}

public struct PhotoResourceExtractionIntent: Sendable, Equatable, Codable, Identifiable {
    public var id: UUID { operationId }

    public let operationId: UUID
    public let profileId: UUID
    public let ownerId: String
    public let deviceId: String
    public let selection: PhotoResourceSelection
    public let observationGeneration: Int64
    public let authorization: PhotoAuthorizationSession
    public let policyVersion: Int
    public let maximumResourceBytes: UInt64
    public let userInitiated: Bool
    public let createdAt: Date

    public init(
        operationId: UUID = UUID(),
        profileId: UUID,
        ownerId: String,
        deviceId: String,
        selection: PhotoResourceSelection,
        observationGeneration: Int64,
        authorization: PhotoAuthorizationSession,
        policyVersion: Int = 1,
        maximumResourceBytes: UInt64,
        userInitiated: Bool,
        createdAt: Date = Date()
    ) {
        self.operationId = operationId
        self.profileId = profileId
        self.ownerId = ownerId
        self.deviceId = deviceId
        self.selection = selection
        self.observationGeneration = observationGeneration
        self.authorization = authorization
        self.policyVersion = policyVersion
        self.maximumResourceBytes = maximumResourceBytes
        self.userInitiated = userInitiated
        self.createdAt = createdAt
    }

    public func replacing(maximumResourceBytes: UInt64) -> PhotoResourceExtractionIntent {
        PhotoResourceExtractionIntent(
            operationId: operationId,
            profileId: profileId,
            ownerId: ownerId,
            deviceId: deviceId,
            selection: selection,
            observationGeneration: observationGeneration,
            authorization: authorization,
            policyVersion: policyVersion,
            maximumResourceBytes: maximumResourceBytes,
            userInitiated: userInitiated,
            createdAt: createdAt)
    }
}

public enum PhotoResourceExtractionState: String, Sendable, Equatable, Codable, CaseIterable {
    case discovered
    case eligible
    case extracting
    case verifying
    case verifiedReady
    case handoffPending
    case handedOff
    case uploadedConfirmed
    case sourceUnavailable
    case permissionRevoked
    case cancelled
    case failedRecoverable
    case failedTerminal
}

public struct PhotoResourceExtractionRecord: Sendable, Equatable, Codable, Identifiable {
    public var id: UUID { operationId }

    public let operationId: UUID
    public let profileId: UUID
    public let ownerId: String
    public let deviceId: String
    public let selection: PhotoResourceSelection
    public let observationGeneration: Int64
    public let authorizationGeneration: UUID
    public let policyVersion: Int
    public let maximumResourceBytes: UInt64
    public let userInitiated: Bool
    public let stagingId: String
    public let byteCount: UInt64?
    public let sha256: String?
    public let state: PhotoResourceExtractionState
    public let relatedUploadOperationId: String?
    public let failureCode: String?
    public let createdAt: Date
    public let updatedAt: Date

    public init(
        operationId: UUID,
        profileId: UUID,
        ownerId: String,
        deviceId: String,
        selection: PhotoResourceSelection,
        observationGeneration: Int64,
        authorizationGeneration: UUID,
        policyVersion: Int = 1,
        maximumResourceBytes: UInt64,
        userInitiated: Bool,
        stagingId: String,
        byteCount: UInt64? = nil,
        sha256: String? = nil,
        state: PhotoResourceExtractionState,
        relatedUploadOperationId: String? = nil,
        failureCode: String? = nil,
        createdAt: Date,
        updatedAt: Date
    ) {
        self.operationId = operationId
        self.profileId = profileId
        self.ownerId = ownerId
        self.deviceId = deviceId
        self.selection = selection
        self.observationGeneration = observationGeneration
        self.authorizationGeneration = authorizationGeneration
        self.policyVersion = policyVersion
        self.maximumResourceBytes = maximumResourceBytes
        self.userInitiated = userInitiated
        self.stagingId = stagingId
        self.byteCount = byteCount
        self.sha256 = sha256
        self.state = state
        self.relatedUploadOperationId = relatedUploadOperationId
        self.failureCode = failureCode
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    public func replacing(
        state: PhotoResourceExtractionState,
        authorizationGeneration: UUID? = nil,
        byteCount: UInt64? = nil,
        sha256: String? = nil,
        relatedUploadOperationId: String? = nil,
        failureCode: String? = nil,
        updatedAt: Date = Date()
    ) -> PhotoResourceExtractionRecord {
        PhotoResourceExtractionRecord(
            operationId: operationId,
            profileId: profileId,
            ownerId: ownerId,
            deviceId: deviceId,
            selection: selection,
            observationGeneration: observationGeneration,
            authorizationGeneration: authorizationGeneration ?? self.authorizationGeneration,
            policyVersion: policyVersion,
            maximumResourceBytes: maximumResourceBytes,
            userInitiated: userInitiated,
            stagingId: stagingId,
            byteCount: byteCount ?? self.byteCount,
            sha256: sha256 ?? self.sha256,
            state: state,
            relatedUploadOperationId: relatedUploadOperationId ?? self.relatedUploadOperationId,
            failureCode: failureCode,
            createdAt: createdAt,
            updatedAt: updatedAt)
    }
}

public struct PhotoResourceVerifiedContent: Sendable, Equatable {
    public let stagingId: String
    public let url: URL
    public let byteCount: UInt64
    public let sha256: String

    public init(stagingId: String, url: URL, byteCount: UInt64, sha256: String) {
        self.stagingId = stagingId
        self.url = url
        self.byteCount = byteCount
        self.sha256 = sha256
    }
}

public struct PhotoTransferSessionContext: Sendable, Equatable {
    public let authorization: PhotoAuthorizationSession
    public let deviceId: String
    public let configuration: PhotoBackupConfiguration

    public init(
        authorization: PhotoAuthorizationSession,
        deviceId: String,
        configuration: PhotoBackupConfiguration
    ) {
        self.authorization = authorization
        self.deviceId = deviceId
        self.configuration = configuration
    }
}

public enum PhotoResourceTransferFailure: String, Error, Sendable, Equatable {
    case permissionDenied
    case sourceUnavailable
    case sourceChanged
    case unsupportedResource
    case resourceTooLarge
    case emptyResource
    case insufficientStorage
    case stagingQuotaExceeded
    case integrityMismatch
    case staleAuthorization
    case ineligibleObservation
    case configurationDisabled
    case extractionAlreadyInProgress
    case existingUnresolvedOperation
    case cancelled
    case storageFailure
}

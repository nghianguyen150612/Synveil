import Foundation

public struct PhotoDiscoveryBatchResult: Sendable, Equatable {
    public let observations: [PhotoAssetObservation]
    public let isComplete: Bool
    public let totalInScope: Int
    public let continuation: PhotoDiscoveryContinuation?

    public init(
        observations: [PhotoAssetObservation], isComplete: Bool, totalInScope: Int,
        continuation: PhotoDiscoveryContinuation? = nil
    ) {
        self.observations = observations
        self.isComplete = isComplete
        self.totalInScope = totalInScope
        self.continuation = continuation
    }
}

public protocol PhotoLibraryPermissionChecking: Sendable {
    func currentPermissionState() async -> PhotoPermissionState
    func requestPermission() async -> PhotoPermissionState
}

public protocol PhotoLibraryDiscoveryProtocol: Sendable {
    func discoverAssets(
        configuration: PhotoBackupConfiguration,
        generation: Int64,
        batchSize: Int
    ) async throws -> PhotoDiscoveryBatchResult

    func discoverAssetPage(
        configuration: PhotoBackupConfiguration,
        generation: Int64,
        batchSize: Int,
        continuation: PhotoDiscoveryContinuation?
    ) async throws -> PhotoDiscoveryBatchResult

    func discoverResourceDescriptors(
        for localIdentifier: String
    ) async throws -> [PhotoAssetResourceDescriptor]

    func invalidateDiscoverySnapshots()
}

public extension PhotoLibraryDiscoveryProtocol {
    func invalidateDiscoverySnapshots() {}

    func discoverAssetPage(
        configuration: PhotoBackupConfiguration,
        generation: Int64,
        batchSize: Int,
        continuation: PhotoDiscoveryContinuation?
    ) async throws -> PhotoDiscoveryBatchResult {
        guard continuation == nil else {
            throw PhotoDiscoveryFailure.continuationExpired
        }
        return try await discoverAssets(
            configuration: configuration, generation: generation, batchSize: batchSize)
    }
}

public enum PhotoDiscoveryFailure: Error, Sendable, Equatable {
    case continuationExpired
    case libraryChanged
    case permissionChanged
}

public protocol PhotoObservationStoreProtocol: Sendable {
    func fetchObservations(
        profileId: UUID,
        ownerId: String
    ) async throws -> [PhotoAssetObservation]

    func markSourceUnavailable(
        localIdentifier: String,
        reason: PhotoSourceAvailability,
        profileId: UUID,
        ownerId: String
    ) async throws

    func saveConfiguration(
        _ config: PhotoBackupConfiguration,
        profileId: UUID
    ) async throws

    func fetchConfiguration(
        profileId: UUID
    ) async throws -> PhotoBackupConfiguration

    func clearObservations(
        profileId: UUID
    ) async throws

    func activateAuthorization(_ authorization: PhotoAuthorizationSession) async throws
    func revokeAuthorization(_ authorization: PhotoAuthorizationSession) async throws
    func commitObservations(
        _ observations: [PhotoAssetObservation], authorization: PhotoAuthorizationSession
    ) async throws
    func createExtraction(
        _ intent: PhotoResourceExtractionIntent, stagingId: String
    ) async throws -> PhotoResourceExtractionRecord
    func transitionExtraction(
        _ record: PhotoResourceExtractionRecord,
        expectedState: PhotoResourceExtractionState,
        authorization: PhotoAuthorizationSession
    ) async throws
    func extraction(
        operationId: UUID, authorization: PhotoAuthorizationSession
    ) async throws -> PhotoResourceExtractionRecord?
    func extractions(
        authorization: PhotoAuthorizationSession
    ) async throws -> [PhotoResourceExtractionRecord]
    func extractionStagingIds() async throws -> Set<String>
}

public protocol PhotoResourceExtractionSource: Sendable {
    func selections(
        for localIdentifier: String,
        includingEditedRenditions: Bool
    ) async throws -> [PhotoResourceSelection]
    func currentModificationDate(for localIdentifier: String) async throws -> Date?
    func extract(
        _ selection: PhotoResourceSelection,
        maximumBytes: UInt64,
        receive: @escaping @Sendable (Data) throws -> Void
    ) async throws -> UInt64
}

@MainActor
public protocol PhotoTransferContextProviding: Sendable {
    func currentContext() async -> PhotoTransferSessionContext?
}

public protocol PhotoLibraryObservationCoordinatorProtocol: Sendable {
    func startObservation(
        profileId: UUID, ownerId: String, sessionRevision: UInt64
    ) async throws -> PhotoDiscoveryScanResult
    func stopObservation() async throws
    func currentScanResult() async -> PhotoDiscoveryScanResult
    func currentAuthorizationSession() async -> PhotoAuthorizationSession?
}

public extension PhotoLibraryObservationCoordinatorProtocol {
    func startObservation(
        profileId: UUID, ownerId: String
    ) async throws -> PhotoDiscoveryScanResult {
        try await startObservation(profileId: profileId, ownerId: ownerId, sessionRevision: 0)
    }
}

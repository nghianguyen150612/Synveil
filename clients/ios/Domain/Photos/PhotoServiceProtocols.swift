import Foundation

public struct PhotoDiscoveryBatchResult: Sendable, Equatable {
    public let observations: [PhotoAssetObservation]
    public let isComplete: Bool
    public let totalInScope: Int

    public init(observations: [PhotoAssetObservation], isComplete: Bool, totalInScope: Int) {
        self.observations = observations
        self.isComplete = isComplete
        self.totalInScope = totalInScope
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

    func discoverResourceDescriptors(
        for localIdentifier: String
    ) async throws -> [PhotoAssetResourceDescriptor]
}

public protocol PhotoObservationStoreProtocol: Sendable {
    func saveObservations(
        _ observations: [PhotoAssetObservation],
        profileId: UUID,
        ownerId: String
    ) async throws

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
}

public protocol PhotoLibraryObservationCoordinatorProtocol: Sendable {
    func startObservation(profileId: UUID, ownerId: String) async throws -> PhotoDiscoveryScanResult
    func stopObservation() async
    func currentScanResult() async -> PhotoDiscoveryScanResult
}

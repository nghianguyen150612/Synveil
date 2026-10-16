import Foundation

public final class PhotoLibraryObservationCoordinator: PhotoLibraryObservationCoordinatorProtocol, @unchecked Sendable {
    private let discoveryService: PhotoLibraryDiscoveryProtocol
    private let store: PhotoObservationStoreProtocol
    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let changeObserver: PhotoLibraryChangeObserverAdapter

    private let lock = NSLock()
    private var activeProfileId: UUID?
    private var activeOwnerId: String?
    private var isObserving = false
    private var currentGeneration: Int64 = 1
    private var lastScanResult: PhotoDiscoveryScanResult = .disabled

    public init(
        discoveryService: PhotoLibraryDiscoveryProtocol,
        store: PhotoObservationStoreProtocol,
        permissionAdapter: PhotoLibraryPermissionChecking,
        changeObserver: PhotoLibraryChangeObserverAdapter = PhotoLibraryChangeObserverAdapter()
    ) {
        self.discoveryService = discoveryService
        self.store = store
        self.permissionAdapter = permissionAdapter
        self.changeObserver = changeObserver
    }

    public func startObservation(profileId: UUID, ownerId: String) async throws -> PhotoDiscoveryScanResult {
        let config = try await store.fetchConfiguration(profileId: profileId)
        guard config.isOptedIn && !config.pauseDiscovery else {
            lock.lock()
            self.lastScanResult = .disabled
            lock.unlock()
            return .disabled
        }

        let permission = await permissionAdapter.currentPermissionState()
        guard permission.isAuthorized else {
            lock.lock()
            self.lastScanResult = .permissionDenied(permission)
            lock.unlock()
            return .permissionDenied(permission)
        }

        lock.lock()
        self.activeProfileId = profileId
        self.activeOwnerId = ownerId
        self.isObserving = true
        let gen = self.currentGeneration
        lock.unlock()

        let observations = try await discoveryService.discoverAssets(
            configuration: config,
            generation: gen,
            batchSize: 1000
        )

        try await store.saveObservations(observations, profileId: profileId, ownerId: ownerId)

        let result = PhotoDiscoveryScanResult.completed(totalObserved: observations.count, generation: gen)

        lock.lock()
        self.lastScanResult = result
        lock.unlock()

        changeObserver.register { [weak self] in
            guard let self = self else { return }
            Task {
                await self.handleLibraryChange()
            }
        }

        return result
    }

    public func stopObservation() async {
        lock.lock()
        self.isObserving = false
        self.activeProfileId = nil
        self.activeOwnerId = nil
        self.lastScanResult = .disabled
        lock.unlock()

        changeObserver.unregister()
    }

    public func currentScanResult() async -> PhotoDiscoveryScanResult {
        lock.lock()
        defer { lock.unlock() }
        return lastScanResult
    }

    private func handleLibraryChange() async {
        lock.lock()
        guard isObserving, let profileId = activeProfileId, let ownerId = activeOwnerId else {
            lock.unlock()
            return
        }
        currentGeneration += 1
        let gen = currentGeneration
        lock.unlock()

        do {
            let config = try await store.fetchConfiguration(profileId: profileId)
            let observations = try await discoveryService.discoverAssets(
                configuration: config,
                generation: gen,
                batchSize: 1000
            )
            try await store.saveObservations(observations, profileId: profileId, ownerId: ownerId)

            lock.lock()
            self.lastScanResult = .completed(totalObserved: observations.count, generation: gen)
            lock.unlock()
        } catch {
            lock.lock()
            self.lastScanResult = .interrupted(reason: error.localizedDescription)
            lock.unlock()
        }
    }
}

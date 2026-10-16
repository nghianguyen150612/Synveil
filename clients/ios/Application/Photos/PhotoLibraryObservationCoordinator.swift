import Foundation

public final class PhotoLibraryObservationCoordinator: PhotoLibraryObservationCoordinatorProtocol,
    @unchecked Sendable
{
    private let discoveryService: PhotoLibraryDiscoveryProtocol
    private let store: PhotoObservationStoreProtocol
    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let changeObserver: PhotoLibraryChangeObserverAdapter

    private let lock = NSLock()
    private var activeProfileId: UUID?
    private var activeOwnerId: String?
    private var isObserving = false
    private var currentGeneration: Int64 = 1
    private var sessionEpoch: UInt64 = 0
    private var activeScanTask: Task<Void, Never>?
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

    public func startObservation(
        profileId: UUID,
        ownerId: String
    ) async throws -> PhotoDiscoveryScanResult {
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
        self.sessionEpoch &+= 1
        let capturedEpoch = self.sessionEpoch
        self.activeProfileId = profileId
        self.activeOwnerId = ownerId
        self.isObserving = true
        let gen = self.currentGeneration
        lock.unlock()

        let batchResult = try await discoveryService.discoverAssets(
            configuration: config,
            generation: gen,
            batchSize: 1000
        )

        lock.lock()
        guard isObserving,
            sessionEpoch == capturedEpoch,
            activeProfileId == profileId,
            activeOwnerId == ownerId
        else {
            lock.unlock()
            return .disabled
        }
        lock.unlock()

        try await store.saveObservations(
            batchResult.observations,
            profileId: profileId,
            ownerId: ownerId
        )

        let result: PhotoDiscoveryScanResult
        if batchResult.isComplete {
            result = .completed(totalObserved: batchResult.observations.count, generation: gen)
        } else {
            result = .partial(
                observedSoFar: batchResult.observations.count,
                totalInScope: batchResult.totalInScope,
                generation: gen
            )
        }

        lock.lock()
        guard isObserving,
            sessionEpoch == capturedEpoch,
            activeProfileId == profileId,
            activeOwnerId == ownerId
        else {
            lock.unlock()
            return .disabled
        }
        self.lastScanResult = result
        lock.unlock()

        changeObserver.register { [weak self] in
            guard let self = self else { return }
            self.enqueueCoalescedChangeScan()
        }

        return result
    }

    public func stopObservation() async {
        lock.lock()
        self.isObserving = false
        self.sessionEpoch &+= 1
        self.activeProfileId = nil
        self.activeOwnerId = nil
        self.lastScanResult = .disabled
        let currentTask = activeScanTask
        self.activeScanTask = nil
        lock.unlock()

        currentTask?.cancel()
        changeObserver.unregister()
    }

    public func currentScanResult() async -> PhotoDiscoveryScanResult {
        lock.lock()
        defer { lock.unlock() }
        return lastScanResult
    }

    private func enqueueCoalescedChangeScan() {
        lock.lock()
        guard isObserving, activeScanTask == nil else {
            lock.unlock()
            return
        }
        let task = Task { [weak self] in
            await self?.performCoalescedChangeScan()
        }
        self.activeScanTask = task
        lock.unlock()
    }

    private func performCoalescedChangeScan() async {
        defer {
            lock.lock()
            self.activeScanTask = nil
            lock.unlock()
        }

        lock.lock()
        guard isObserving,
            let profileId = activeProfileId,
            let ownerId = activeOwnerId
        else {
            lock.unlock()
            return
        }
        currentGeneration += 1
        let gen = currentGeneration
        let capturedEpoch = sessionEpoch
        lock.unlock()

        do {
            let config = try await store.fetchConfiguration(profileId: profileId)
            let batchResult = try await discoveryService.discoverAssets(
                configuration: config,
                generation: gen,
                batchSize: 1000
            )

            lock.lock()
            guard isObserving,
                sessionEpoch == capturedEpoch,
                activeProfileId == profileId,
                activeOwnerId == ownerId
            else {
                lock.unlock()
                return
            }
            lock.unlock()

            try await store.saveObservations(
                batchResult.observations,
                profileId: profileId,
                ownerId: ownerId
            )

            let result: PhotoDiscoveryScanResult
            if batchResult.isComplete {
                result = .completed(
                    totalObserved: batchResult.observations.count,
                    generation: gen
                )
            } else {
                result = .partial(
                    observedSoFar: batchResult.observations.count,
                    totalInScope: batchResult.totalInScope,
                    generation: gen
                )
            }

            lock.lock()
            guard isObserving,
                sessionEpoch == capturedEpoch,
                activeProfileId == profileId,
                activeOwnerId == ownerId
            else {
                lock.unlock()
                return
            }
            self.lastScanResult = result
            lock.unlock()
        } catch {
            lock.lock()
            guard isObserving, sessionEpoch == capturedEpoch else {
                lock.unlock()
                return
            }
            self.lastScanResult = .interrupted(reason: error.localizedDescription)
            lock.unlock()
        }
    }
}

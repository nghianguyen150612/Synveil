import Foundation

public actor PhotoLibraryObservationCoordinator: PhotoLibraryObservationCoordinatorProtocol {
    private let discoveryService: PhotoLibraryDiscoveryProtocol
    private let store: PhotoObservationStoreProtocol
    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let changeObserver: PhotoLibraryChangeObserverAdapter

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
            self.lastScanResult = .disabled
            return .disabled
        }

        let permission = await permissionAdapter.currentPermissionState()
        guard permission.isAuthorized else {
            self.lastScanResult = .permissionDenied(permission)
            return .permissionDenied(permission)
        }

        self.sessionEpoch &+= 1
        let capturedEpoch = self.sessionEpoch
        self.activeProfileId = profileId
        self.activeOwnerId = ownerId
        self.isObserving = true
        let gen = self.currentGeneration

        let batchResult = try await discoveryService.discoverAssets(
            configuration: config,
            generation: gen,
            batchSize: 1000
        )

        guard isObserving,
            sessionEpoch == capturedEpoch,
            activeProfileId == profileId,
            activeOwnerId == ownerId
        else {
            return .disabled
        }

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

        guard isObserving,
            sessionEpoch == capturedEpoch,
            activeProfileId == profileId,
            activeOwnerId == ownerId
        else {
            return .disabled
        }
        self.lastScanResult = result

        changeObserver.register { [weak self] in
            guard let self = self else { return }
            Task {
                await self.enqueueCoalescedChangeScan()
            }
        }

        return result
    }

    public func stopObservation() async {
        self.isObserving = false
        self.sessionEpoch &+= 1
        self.activeProfileId = nil
        self.activeOwnerId = nil
        self.lastScanResult = .disabled
        let currentTask = activeScanTask
        self.activeScanTask = nil

        currentTask?.cancel()
        changeObserver.unregister()
    }

    public func currentScanResult() async -> PhotoDiscoveryScanResult {
        lastScanResult
    }

    private func enqueueCoalescedChangeScan() async {
        guard isObserving, activeScanTask == nil else {
            return
        }
        let task = Task { [weak self] in
            guard let self = self else { return }
            await self.performCoalescedChangeScan()
        }
        self.activeScanTask = task
    }

    private func performCoalescedChangeScan() async {
        defer {
            self.activeScanTask = nil
        }

        guard isObserving,
            let profileId = activeProfileId,
            let ownerId = activeOwnerId
        else {
            return
        }
        currentGeneration += 1
        let gen = currentGeneration
        let capturedEpoch = sessionEpoch

        do {
            let config = try await store.fetchConfiguration(profileId: profileId)
            let batchResult = try await discoveryService.discoverAssets(
                configuration: config,
                generation: gen,
                batchSize: 1000
            )

            guard isObserving,
                sessionEpoch == capturedEpoch,
                activeProfileId == profileId,
                activeOwnerId == ownerId
            else {
                return
            }

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

            guard isObserving,
                sessionEpoch == capturedEpoch,
                activeProfileId == profileId,
                activeOwnerId == ownerId
            else {
                return
            }
            self.lastScanResult = result
        } catch {
            guard isObserving, sessionEpoch == capturedEpoch else {
                return
            }
            self.lastScanResult = .interrupted(reason: error.localizedDescription)
        }
    }
}

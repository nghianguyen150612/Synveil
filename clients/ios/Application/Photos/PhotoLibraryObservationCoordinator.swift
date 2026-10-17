import Foundation

public actor PhotoLibraryObservationCoordinator: PhotoLibraryObservationCoordinatorProtocol {
    private let discoveryService: PhotoLibraryDiscoveryProtocol
    private let store: PhotoObservationStoreProtocol
    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let changeObserver: PhotoLibraryChangeObserverAdapter

    private var activeProfileId: UUID?
    private var activeOwnerId: String?
    private var activeAuthorization: PhotoAuthorizationSession?
    private var isObserving = false
    private var initialScanInProgress = false
    private var currentGeneration: Int64 = 1
    private var sessionEpoch: UInt64 = 0
    private var activeScanTask: Task<Void, Never>?
    private var activeScanEpoch: UInt64?
    private var lastScanResult: PhotoDiscoveryScanResult = .disabled
    private var lastObservedConfiguration: PhotoBackupConfiguration?
    private var lastObservedPermission: PhotoPermissionState?
    private var lastObservedChangeRevision: UInt64?

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
        ownerId: String,
        sessionRevision: UInt64
    ) async throws -> PhotoDiscoveryScanResult {
        let config = try await store.fetchConfiguration(profileId: profileId)
        guard config.isOptedIn && !config.pauseDiscovery else {
            try await stopObservation()
            return .disabled
        }

        let permission = await permissionAdapter.currentPermissionState()
        guard permission.isAuthorized else {
            try await stopObservation()
            if activeAuthorization == nil { lastScanResult = .permissionDenied(permission) }
            return .permissionDenied(permission)
        }

        if let authorization = activeAuthorization,
            authorization.profileId == profileId, authorization.ownerId == ownerId,
            authorization.sessionRevision == sessionRevision, isObserving
        {
            let unchanged =
                lastObservedConfiguration == config
                && lastObservedPermission == permission
                && lastObservedChangeRevision == changeObserver.revision
            if unchanged { return lastScanResult }
            guard !initialScanInProgress, activeScanTask == nil else { return lastScanResult }
            initialScanInProgress = true
            advanceGeneration()
            discoveryService.invalidateDiscoverySnapshots()
            let capturedEpoch = sessionEpoch
            let result = await scanWithRestarts(
                config: config, authorization: authorization, capturedEpoch: capturedEpoch)
            initialScanInProgress = false
            guard isCurrent(capturedEpoch, authorization: authorization) else { return .disabled }
            lastScanResult = result
            lastObservedConfiguration = config
            lastObservedPermission = permission
            lastObservedChangeRevision = changeObserver.revision
            if case .permissionDenied = result {
                try await stopObservation()
                if activeAuthorization == nil { lastScanResult = result }
            }
            return result
        }

        let oldAuthorization = activeAuthorization
        let oldTask = activeScanTask
        sessionEpoch &+= 1
        let capturedEpoch = sessionEpoch
        isObserving = false
        initialScanInProgress = true
        activeScanTask = nil
        activeScanEpoch = nil
        oldTask?.cancel()
        changeObserver.unregister()
        discoveryService.invalidateDiscoverySnapshots()
        if let oldAuthorization {
            try await store.revokeAuthorization(oldAuthorization)
        }

        let authorization = PhotoAuthorizationSession(
            profileId: profileId, ownerId: ownerId, sessionRevision: sessionRevision)
        activeProfileId = profileId
        activeOwnerId = ownerId
        activeAuthorization = authorization
        isObserving = true
        lastObservedConfiguration = nil
        lastObservedPermission = nil
        lastObservedChangeRevision = nil
        do {
            try await store.activateAuthorization(authorization)
        } catch {
            isObserving = false
            initialScanInProgress = false
            activeProfileId = nil
            activeOwnerId = nil
            activeAuthorization = nil
            throw error
        }

        guard isCurrent(capturedEpoch, authorization: authorization) else {
            try? await store.revokeAuthorization(authorization)
            initialScanInProgress = false
            return .disabled
        }

        changeObserver.register { [weak self] in
            guard let self else { return }
            Task { await self.enqueueCoalescedChangeScan() }
        }
        let result = await scanWithRestarts(
            config: config, authorization: authorization, capturedEpoch: capturedEpoch)
        initialScanInProgress = false
        guard isCurrent(capturedEpoch, authorization: authorization) else { return .disabled }
        lastScanResult = result
        lastObservedConfiguration = config
        lastObservedPermission = permission
        lastObservedChangeRevision = changeObserver.revision
        if case .permissionDenied = result {
            try await stopObservation()
            if activeAuthorization == nil { lastScanResult = result }
        }
        return result
    }

    public func stopObservation() async throws {
        isObserving = false
        initialScanInProgress = false
        sessionEpoch &+= 1
        activeProfileId = nil
        activeOwnerId = nil
        let authorization = activeAuthorization
        activeAuthorization = nil
        lastScanResult = .disabled
        lastObservedConfiguration = nil
        lastObservedPermission = nil
        lastObservedChangeRevision = nil
        let currentTask = activeScanTask
        activeScanTask = nil
        activeScanEpoch = nil
        currentTask?.cancel()
        changeObserver.unregister()
        discoveryService.invalidateDiscoverySnapshots()
        if let authorization {
            // This serialized SQLite revocation is the effective write barrier. When this method
            // returns, prior queued commits are finished and later commits with the old token fail.
            do {
                try await store.revokeAuthorization(authorization)
            } catch {
                if activeAuthorization == nil { activeAuthorization = authorization }
                throw error
            }
        }
    }

    public func currentScanResult() async -> PhotoDiscoveryScanResult {
        lastScanResult
    }

    public func currentAuthorizationSession() async -> PhotoAuthorizationSession? {
        activeAuthorization
    }

    private func scanWithRestarts(
        config: PhotoBackupConfiguration,
        authorization: PhotoAuthorizationSession,
        capturedEpoch: UInt64
    ) async -> PhotoDiscoveryScanResult {
        for attempt in 0..<3 {
            guard isCurrent(capturedEpoch, authorization: authorization) else { return .disabled }
            let permission = await permissionAdapter.currentPermissionState()
            guard permission.isAuthorized else { return .permissionDenied(permission) }
            let generation = currentGeneration
            let revision = changeObserver.revision
            do {
                let result = try await scanGeneration(
                    config: config, permission: permission, authorization: authorization,
                    generation: generation, capturedEpoch: capturedEpoch,
                    startingRevision: revision)
                guard isCurrent(capturedEpoch, authorization: authorization) else {
                    return .disabled
                }
                return result
            } catch PhotoDiscoveryFailure.libraryChanged {
                discoveryService.invalidateDiscoverySnapshots()
                advanceGeneration()
                if attempt == 2 {
                    return .interrupted(reason: "Photo library changed during discovery")
                }
            } catch PhotoDiscoveryFailure.permissionChanged {
                let current = await permissionAdapter.currentPermissionState()
                return .permissionDenied(current)
            } catch is CancellationError {
                return .disabled
            } catch {
                return .interrupted(reason: error.localizedDescription)
            }
        }
        return .interrupted(reason: "Photo library scan could not stabilize")
    }

    private func scanGeneration(
        config: PhotoBackupConfiguration,
        permission: PhotoPermissionState,
        authorization: PhotoAuthorizationSession,
        generation: Int64,
        capturedEpoch: UInt64,
        startingRevision: UInt64
    ) async throws -> PhotoDiscoveryScanResult {
        var continuation: PhotoDiscoveryContinuation?
        var observed = 0
        var expectedTotal: Int?
        var priorOffset = 0
        var expectedSnapshotId: UUID?

        while true {
            try Task.checkCancellation()
            guard isCurrent(capturedEpoch, authorization: authorization) else {
                throw CancellationError()
            }
            try await validateScanFence(
                permission: permission, authorization: authorization,
                capturedEpoch: capturedEpoch, startingRevision: startingRevision)

            let page = try await discoveryService.discoverAssetPage(
                configuration: config, generation: generation, batchSize: 1_000,
                continuation: continuation)
            let nextOffset = priorOffset.addingReportingOverflow(page.observations.count)
            let nextObserved = observed.addingReportingOverflow(page.observations.count)
            guard page.totalInScope >= 0, page.observations.count <= 1_000,
                expectedTotal.map({ $0 == page.totalInScope }) ?? true,
                !nextOffset.overflow, !nextObserved.overflow
            else { throw PhotoDiscoveryFailure.continuationExpired }
            expectedTotal = page.totalInScope

            if let cursor = page.continuation {
                guard cursor.generation == generation,
                    cursor.totalInScope == page.totalInScope,
                    cursor.nextOffset == nextOffset.partialValue,
                    cursor.nextOffset > priorOffset,
                    cursor.nextOffset <= page.totalInScope,
                    expectedSnapshotId.map({ $0 == cursor.snapshotId }) ?? true,
                    !page.isComplete
                else { throw PhotoDiscoveryFailure.continuationExpired }
                expectedSnapshotId = cursor.snapshotId
            } else if !page.isComplete {
                // Older test adapters may only report that a result is partial. Never treat that
                // page as complete or repeat it in an unbounded first-page loop.
                guard page.totalInScope >= page.observations.count else {
                    throw PhotoDiscoveryFailure.continuationExpired
                }
            } else if nextOffset.partialValue != page.totalInScope {
                throw PhotoDiscoveryFailure.continuationExpired
            }

            try await validateScanFence(
                permission: permission, authorization: authorization,
                capturedEpoch: capturedEpoch, startingRevision: startingRevision)
            try await store.commitObservations(page.observations, authorization: authorization)
            observed = nextObserved.partialValue
            priorOffset = nextOffset.partialValue

            try await validateScanFence(
                permission: permission, authorization: authorization,
                capturedEpoch: capturedEpoch, startingRevision: startingRevision)

            if page.isComplete {
                return .completed(totalObserved: observed, generation: generation)
            }
            guard let next = page.continuation else {
                return .partial(
                    observedSoFar: observed, totalInScope: page.totalInScope,
                    generation: generation)
            }
            continuation = next
        }
    }

    private func validateScanFence(
        permission: PhotoPermissionState,
        authorization: PhotoAuthorizationSession,
        capturedEpoch: UInt64,
        startingRevision: UInt64
    ) async throws {
        guard isCurrent(capturedEpoch, authorization: authorization) else {
            throw CancellationError()
        }
        guard changeObserver.revision == startingRevision else {
            throw PhotoDiscoveryFailure.libraryChanged
        }
        let currentPermission = await permissionAdapter.currentPermissionState()
        guard currentPermission.isAuthorized else { throw PhotoDiscoveryFailure.permissionChanged }
        guard currentPermission == permission else { throw PhotoDiscoveryFailure.permissionChanged }
    }

    private func isCurrent(
        _ capturedEpoch: UInt64, authorization: PhotoAuthorizationSession
    ) -> Bool {
        isObserving && sessionEpoch == capturedEpoch && activeAuthorization == authorization
            && activeProfileId == authorization.profileId && activeOwnerId == authorization.ownerId
    }

    private func advanceGeneration() {
        if currentGeneration < Int64.max { currentGeneration += 1 } else { currentGeneration = 1 }
    }

    private func enqueueCoalescedChangeScan() async {
        guard isObserving, !initialScanInProgress, activeScanTask == nil else { return }
        let capturedEpoch = sessionEpoch
        let task = Task { [weak self] in
            guard let self else { return }
            await self.performCoalescedChangeScan(capturedEpoch: capturedEpoch)
        }
        activeScanTask = task
        activeScanEpoch = capturedEpoch
    }

    private func performCoalescedChangeScan(capturedEpoch: UInt64) async {
        defer {
            if activeScanEpoch == capturedEpoch {
                activeScanTask = nil
                activeScanEpoch = nil
            }
        }
        guard isObserving, let authorization = activeAuthorization,
            let profileId = activeProfileId, let ownerId = activeOwnerId,
            sessionEpoch == capturedEpoch
        else { return }

        advanceGeneration()
        do {
            let config = try await store.fetchConfiguration(profileId: profileId)
            guard config.isOptedIn && !config.pauseDiscovery else {
                try await stopObservation()
                return
            }
            let permission = await permissionAdapter.currentPermissionState()
            guard isCurrent(capturedEpoch, authorization: authorization) else { return }
            let result = await scanWithRestarts(
                config: config, authorization: authorization, capturedEpoch: capturedEpoch)
            guard isCurrent(capturedEpoch, authorization: authorization),
                activeOwnerId == ownerId
            else { return }
            lastScanResult = result
            lastObservedConfiguration = config
            lastObservedPermission = permission
            lastObservedChangeRevision = changeObserver.revision
            if case .permissionDenied = result { try await stopObservation() }
        } catch {
            guard isCurrent(capturedEpoch, authorization: authorization) else { return }
            lastScanResult = .interrupted(reason: error.localizedDescription)
        }
    }
}

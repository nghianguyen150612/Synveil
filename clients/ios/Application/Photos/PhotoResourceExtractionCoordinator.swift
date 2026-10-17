import Foundation

public actor PhotoResourceExtractionCoordinator {
    private let source: PhotoResourceExtractionSource
    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let contextProvider: PhotoTransferContextProviding
    private let store: PhotoObservationStoreProtocol
    private let staging: PhotoResourceStagingCoordinator
    private let mutationStore: MutationQueueSQLiteStore
    private let maximumResourceBytes: UInt64
    private var operations: [UUID: Task<PhotoResourceExtractionRecord, any Error>] = [:]

    init(
        source: PhotoResourceExtractionSource,
        permissionAdapter: PhotoLibraryPermissionChecking,
        contextProvider: PhotoTransferContextProviding,
        store: PhotoObservationStoreProtocol,
        staging: PhotoResourceStagingCoordinator,
        mutationStore: MutationQueueSQLiteStore,
        maximumResourceBytes: UInt64 = DownloadPolicy.quota / 2
    ) {
        self.source = source
        self.permissionAdapter = permissionAdapter
        self.contextProvider = contextProvider
        self.store = store
        self.staging = staging
        self.mutationStore = mutationStore
        self.maximumResourceBytes = maximumResourceBytes
    }

    public func prepare(
        _ intent: PhotoResourceExtractionIntent
    ) async throws -> PhotoResourceExtractionRecord {
        guard operations[intent.operationId] == nil, operations.isEmpty else {
            throw PhotoResourceTransferFailure.extractionAlreadyInProgress
        }
        let task = Task { try await performPreparation(intent) }
        operations[intent.operationId] = task
        defer { operations[intent.operationId] = nil }
        return try await withTaskCancellationHandler {
            try await task.value
        } onCancel: {
            task.cancel()
        }
    }

    public func availableSelections(
        for observation: PhotoAssetObservation
    ) async throws -> [PhotoResourceSelection] {
        guard await permissionAdapter.currentPermissionState().isAuthorized else {
            throw PhotoResourceTransferFailure.permissionDenied
        }
        guard let context = await contextProvider.currentContext() else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        guard context.configuration.isOptedIn, !context.configuration.pauseDiscovery else {
            throw PhotoResourceTransferFailure.configurationDisabled
        }
        let observations = try await store.fetchObservations(
            profileId: context.authorization.profileId, ownerId: context.authorization.ownerId)
        guard
            let current = observations.first(where: {
                $0.localIdentifier == observation.localIdentifier
            }), current.discoveryGeneration == observation.discoveryGeneration,
            !current.isExcluded, current.sourceAvailability == .available
        else { throw PhotoResourceTransferFailure.ineligibleObservation }
        return try await source.selections(
            for: observation.localIdentifier, includingEditedRenditions: true)
    }

    public func prepareSelection(
        operationId: UUID = UUID(),
        selection: PhotoResourceSelection,
        observationGeneration: Int64
    ) async throws -> PhotoResourceExtractionRecord {
        guard let context = await contextProvider.currentContext() else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        let intent = PhotoResourceExtractionIntent(
            operationId: operationId,
            profileId: context.authorization.profileId,
            ownerId: context.authorization.ownerId,
            deviceId: context.deviceId,
            selection: selection,
            observationGeneration: observationGeneration,
            authorization: context.authorization,
            maximumResourceBytes: maximumResourceBytes,
            userInitiated: true)
        return try await prepare(intent)
    }

    public func cancelPreparation(operationId: UUID) {
        operations[operationId]?.cancel()
    }

    public func cancelAllPreparations() async {
        let active = Array(operations.values)
        active.forEach { $0.cancel() }
        for task in active { _ = try? await task.value }
    }

    public func recoverStagingAfterLaunch() async throws {
        guard operations.isEmpty else { return }
        if let context = await contextProvider.currentContext() {
            let records = try await store.extractions(authorization: context.authorization)
            for record in records {
                switch record.state {
                case .discovered, .eligible, .extracting, .verifying:
                    let interrupted = record.replacing(
                        state: .failedRecoverable, failureCode: "interrupted_after_restart")
                    try? await store.transitionExtraction(
                        interrupted, expectedState: record.state,
                        authorization: context.authorization)
                case .verifiedReady:
                    do {
                        guard let byteCount = record.byteCount, let sha256 = record.sha256 else {
                            throw PhotoResourceTransferFailure.integrityMismatch
                        }
                        _ = try await staging.verify(
                            stagingId: record.stagingId, byteCount: byteCount, sha256: sha256)
                    } catch {
                        let invalid = record.replacing(
                            state: .failedTerminal, failureCode: "staging_integrity_failure")
                        try? await store.transitionExtraction(
                            invalid, expectedState: .verifiedReady,
                            authorization: context.authorization)
                    }
                default:
                    break
                }
            }
        }
        let retained = try await store.extractionStagingIds()
        try await staging.recoverAfterLaunch(keeping: retained)
    }

    public func recentExtractions() async throws -> [PhotoResourceExtractionRecord] {
        guard let context = await contextProvider.currentContext() else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        let records = try await store.extractions(authorization: context.authorization)
        return Array(records.prefix(100))
    }

    private func performPreparation(
        _ intent: PhotoResourceExtractionIntent
    ) async throws -> PhotoResourceExtractionRecord {
        var record: PhotoResourceExtractionRecord?
        var writer: PhotoResourceStageWriter?
        let stagingId = UploadOperationId()
        do {
            try Task.checkCancellation()
            guard intent.userInitiated else {
                throw PhotoResourceTransferFailure.configurationDisabled
            }
            let context = try await validate(intent)
            let observations = try await store.fetchObservations(
                profileId: intent.profileId, ownerId: intent.ownerId)
            guard
                let observation = observations.first(where: {
                    $0.localIdentifier == intent.selection.localIdentifier
                }), observation.discoveryGeneration == intent.observationGeneration,
                !observation.isExcluded, observation.sourceAvailability == .available
            else { throw PhotoResourceTransferFailure.ineligibleObservation }

            let choices = try await source.selections(
                for: intent.selection.localIdentifier, includingEditedRenditions: true)
            guard choices.contains(intent.selection) else {
                throw PhotoResourceTransferFailure.sourceUnavailable
            }
            try validateVideoPolicy(intent.selection, configuration: context.configuration)

            let maximumBytes = min(intent.maximumResourceBytes, maximumResourceBytes)
            guard maximumBytes > 0 else { throw PhotoResourceTransferFailure.resourceTooLarge }
            let durableIntent = intent.replacing(maximumResourceBytes: maximumBytes)
            record = try await store.createExtraction(durableIntent, stagingId: stagingId.rawValue)
            guard let discovered = record else {
                throw PhotoResourceTransferFailure.storageFailure
            }
            let eligible = discovered.replacing(state: .eligible)
            try await store.transitionExtraction(
                eligible, expectedState: .discovered, authorization: intent.authorization)
            record = eligible

            let durableReservations = try await mutationStore.downloadReservations()
            writer = try await staging.begin(
                stagingId: stagingId, maximumBytes: maximumBytes,
                durableReservations: durableReservations)
            guard let eligibleRecord = record else {
                throw PhotoResourceTransferFailure.storageFailure
            }
            let extracting = eligibleRecord.replacing(state: .extracting)
            try await store.transitionExtraction(
                extracting, expectedState: .eligible, authorization: intent.authorization)
            record = extracting

            guard let activeWriter = writer else {
                throw PhotoResourceTransferFailure.storageFailure
            }
            let streamedByteCount = try await source.extract(
                intent.selection, maximumBytes: maximumBytes
            ) { data in
                try activeWriter.append(data)
            }
            try Task.checkCancellation()
            guard let extractionRecord = record else {
                throw PhotoResourceTransferFailure.storageFailure
            }
            let verifying = extractionRecord.replacing(state: .verifying)
            try await store.transitionExtraction(
                verifying, expectedState: .extracting, authorization: intent.authorization)
            record = verifying

            guard
                photoTimestampValuesMatch(
                    try await source.currentModificationDate(
                        for: intent.selection.localIdentifier),
                    intent.selection.sourceModificationDate)
            else { throw PhotoResourceTransferFailure.sourceChanged }
            _ = try await validate(intent)
            try await validateObservation(intent)

            let staged = try await staging.publish(stagingId: stagingId, writer: activeWriter)
            writer = nil
            guard staged.byteCount == streamedByteCount, staged.byteCount > 0 else {
                throw PhotoResourceTransferFailure.integrityMismatch
            }
            let verified = try await staging.verify(
                stagingId: staged.stagingId, byteCount: staged.byteCount,
                sha256: staged.sha256)
            guard verified.byteCount == staged.byteCount, verified.sha256 == staged.sha256 else {
                throw PhotoResourceTransferFailure.integrityMismatch
            }

            // The permission, account, configuration and observation are re-read immediately
            // before the SQLite transaction. READY is committed only if its durable session
            // generation is still active inside that transaction.
            _ = try await validate(intent)
            try await validateObservation(intent)
            guard let verifyingRecord = record else {
                throw PhotoResourceTransferFailure.storageFailure
            }
            let ready = verifyingRecord.replacing(
                state: .verifiedReady, byteCount: verified.byteCount, sha256: verified.sha256)
            try await store.transitionExtraction(
                ready, expectedState: .verifying, authorization: intent.authorization)
            return ready
        } catch {
            writer?.abort()
            await staging.discard(stagingId: stagingId.rawValue)
            if let record {
                let (state, code) = failureState(error)
                let failed = record.replacing(state: state, failureCode: code)
                try? await store.transitionExtraction(
                    failed, expectedState: record.state, authorization: intent.authorization)
            }
            throw normalize(error)
        }
    }

    private func validate(
        _ intent: PhotoResourceExtractionIntent
    ) async throws -> PhotoTransferSessionContext {
        try Task.checkCancellation()
        guard await permissionAdapter.currentPermissionState().isAuthorized else {
            throw PhotoResourceTransferFailure.permissionDenied
        }
        guard let context = await contextProvider.currentContext(),
            context.authorization == intent.authorization,
            context.authorization.profileId == intent.profileId,
            context.authorization.ownerId == intent.ownerId,
            context.deviceId == intent.deviceId
        else { throw PhotoResourceTransferFailure.staleAuthorization }
        guard context.configuration.isOptedIn, !context.configuration.pauseDiscovery else {
            throw PhotoResourceTransferFailure.configurationDisabled
        }
        try validateVideoPolicy(intent.selection, configuration: context.configuration)
        return context
    }

    private func validateObservation(_ intent: PhotoResourceExtractionIntent) async throws {
        let observations = try await store.fetchObservations(
            profileId: intent.profileId, ownerId: intent.ownerId)
        guard
            let observation = observations.first(where: {
                $0.localIdentifier == intent.selection.localIdentifier
            }), observation.discoveryGeneration == intent.observationGeneration,
            !observation.isExcluded, observation.sourceAvailability == .available
        else { throw PhotoResourceTransferFailure.ineligibleObservation }
    }

    private func validateVideoPolicy(
        _ selection: PhotoResourceSelection, configuration: PhotoBackupConfiguration
    ) throws {
        if !configuration.includeVideos,
            selection.role == .primaryVideo || selection.role == .motionVideo
        {
            throw PhotoResourceTransferFailure.configurationDisabled
        }
    }

    private func failureState(
        _ error: Error
    ) -> (PhotoResourceExtractionState, String) {
        if error is CancellationError {
            return (.cancelled, PhotoResourceTransferFailure.cancelled.rawValue)
        }
        guard let failure = error as? PhotoResourceTransferFailure else {
            return (.failedRecoverable, PhotoResourceTransferFailure.storageFailure.rawValue)
        }
        switch failure {
        case .permissionDenied, .staleAuthorization:
            return (.permissionRevoked, failure.rawValue)
        case .sourceUnavailable:
            return (.sourceUnavailable, failure.rawValue)
        case .integrityMismatch, .unsupportedResource, .ineligibleObservation:
            return (.failedTerminal, failure.rawValue)
        case .cancelled:
            return (.cancelled, failure.rawValue)
        default:
            return (.failedRecoverable, failure.rawValue)
        }
    }

    private func normalize(_ error: Error) -> Error {
        if error is CancellationError { return PhotoResourceTransferFailure.cancelled }
        if let error = error as? PhotoResourceTransferFailure { return error }
        return PhotoResourceTransferFailure.storageFailure
    }
}

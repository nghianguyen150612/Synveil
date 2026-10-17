import Foundation

/// Explicitly hands one already verified PhotoKit resource to the P045 foreground coordinator.
/// This type has no background transfer entry point and never creates a second HTTP client.
@MainActor
final class PhotoResourceUploadHandoffCoordinator {
    private let source: PhotoResourceExtractionSource
    private let permissionAdapter: PhotoLibraryPermissionChecking
    private let contextProvider: PhotoTransferContextProviding
    private let store: PhotoObservationStoreProtocol
    private let staging: PhotoResourceStagingCoordinator
    private let uploads: any PhotoForegroundUploadCoordinatorProtocol
    private let beforeUploadStart: @MainActor () async -> Void

    init(
        source: PhotoResourceExtractionSource,
        permissionAdapter: PhotoLibraryPermissionChecking,
        contextProvider: PhotoTransferContextProviding,
        store: PhotoObservationStoreProtocol,
        staging: PhotoResourceStagingCoordinator,
        uploads: any PhotoForegroundUploadCoordinatorProtocol,
        beforeUploadStart: @escaping @MainActor () async -> Void = {}
    ) {
        self.source = source
        self.permissionAdapter = permissionAdapter
        self.contextProvider = contextProvider
        self.store = store
        self.staging = staging
        self.uploads = uploads
        self.beforeUploadStart = beforeUploadStart
    }

    func uploadSelected(
        extractionId: UUID,
        target: UploadTarget,
        libraryStatus: LibraryStatus,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> Result<FileUploadResult, PhotoResourceTransferFailure> {
        do {
            guard await permissionAdapter.currentPermissionState().isAuthorized else {
                throw PhotoResourceTransferFailure.permissionDenied
            }
            guard let context = await contextProvider.currentContext(),
                context.configuration.isOptedIn, !context.configuration.pauseDiscovery,
                context.deviceId.isEmpty == false
            else { throw PhotoResourceTransferFailure.staleAuthorization }

            guard
                var record = try await store.extraction(
                    operationId: extractionId, authorization: context.authorization)
            else { throw PhotoResourceTransferFailure.ineligibleObservation }
            guard record.profileId == context.authorization.profileId,
                record.ownerId == context.authorization.ownerId,
                record.deviceId == context.deviceId,
                [.verifiedReady, .handoffPending].contains(record.state),
                let byteCount = record.byteCount, let sha256 = record.sha256
            else { throw PhotoResourceTransferFailure.ineligibleObservation }

            let observations = try await store.fetchObservations(
                profileId: record.profileId, ownerId: record.ownerId)
            guard
                let observation = observations.first(where: {
                    $0.localIdentifier == record.selection.localIdentifier
                }), observation.discoveryGeneration == record.observationGeneration,
                observation.sourceAvailability == .available, !observation.isExcluded
            else { throw PhotoResourceTransferFailure.ineligibleObservation }

            let choices = try await source.selections(
                for: record.selection.localIdentifier, includingEditedRenditions: true)
            guard choices.contains(record.selection) else {
                throw PhotoResourceTransferFailure.sourceUnavailable
            }
            guard
                photoTimestampValuesMatch(
                    try await source.currentModificationDate(
                        for: record.selection.localIdentifier),
                    record.selection.sourceModificationDate)
            else { throw PhotoResourceTransferFailure.sourceChanged }

            let verified = try await staging.verify(
                stagingId: record.stagingId, byteCount: byteCount, sha256: sha256)
            let expected = try UploadExpectedContent(byteLength: byteCount, sha256: sha256)

            // Recheck the source and live policy after the staged bytes have been rehashed. A
            // permission, owner, device, or rendition change in the earlier awaits must not
            // turn a previously selected row into a handoff.
            let currentChoices = try await source.selections(
                for: record.selection.localIdentifier, includingEditedRenditions: true)
            let currentModificationDate = try await source.currentModificationDate(
                for: record.selection.localIdentifier)
            guard currentChoices.contains(record.selection),
                photoTimestampValuesMatch(
                    currentModificationDate, record.selection.sourceModificationDate)
            else { throw PhotoResourceTransferFailure.sourceChanged }
            try await validateCurrentContext(matching: context)

            let uploadOperationId: UploadOperationId
            if let existing = record.relatedUploadOperationId {
                uploadOperationId = try UploadOperationId(validating: existing)
            } else {
                uploadOperationId = UploadOperationId()
                record = record.replacing(
                    state: .handoffPending,
                    relatedUploadOperationId: uploadOperationId.rawValue)
                try await store.transitionExtraction(
                    record, expectedState: .verifiedReady,
                    authorization: context.authorization)
            }

            guard record.state == .handoffPending else {
                throw PhotoResourceTransferFailure.ineligibleObservation
            }
            await beforeUploadStart()
            try await validateHandoffEligibility(record, matching: context)
            let result = await uploads.start(
                selectedURL: verified.url,
                target: target,
                library: target.library,
                parent: target.parent,
                libraryStatus: libraryStatus,
                backgroundEnabled: false,
                stableOperationId: uploadOperationId,
                expectedSourceContent: expected,
                progress: progress)

            let durableUpload: UploadRecord?
            switch result {
            case .completed(let upload): durableUpload = upload
            case .scheduled(let upload): durableUpload = upload
            case .stopped(let upload, _): durableUpload = upload
            }
            if let durableUpload {
                let outcome: PhotoResourceExtractionState =
                    durableUpload.state == .committed && durableUpload.completion != nil
                    ? .uploadedConfirmed : .handedOff
                let handedOff = record.replacing(state: outcome)
                try await store.transitionExtraction(
                    handedOff, expectedState: record.state,
                    authorization: context.authorization)
                await staging.discard(stagingId: record.stagingId)
            }
            return .success(result)
        } catch let failure as PhotoResourceTransferFailure {
            return .failure(failure)
        } catch is CancellationError {
            return .failure(.cancelled)
        } catch {
            return .failure(.storageFailure)
        }
    }

    func resumeExisting(
        extractionId: UUID,
        uploadOperationId: String,
        library: LibraryId,
        libraryStatus: LibraryStatus,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> Result<FileUploadResult, PhotoResourceTransferFailure> {
        do {
            guard await permissionAdapter.currentPermissionState().isAuthorized else {
                throw PhotoResourceTransferFailure.permissionDenied
            }
            guard let context = await contextProvider.currentContext(),
                context.configuration.isOptedIn, !context.configuration.pauseDiscovery
            else { throw PhotoResourceTransferFailure.staleAuthorization }
            guard
                let record = try await store.extraction(
                    operationId: extractionId, authorization: context.authorization),
                record.deviceId == context.deviceId,
                record.state == .handedOff,
                record.relatedUploadOperationId == uploadOperationId
            else { throw PhotoResourceTransferFailure.ineligibleObservation }
            let uploadId = try UploadOperationId(validating: uploadOperationId)
            let result = await uploads.resume(
                id: uploadId, library: library, libraryStatus: libraryStatus,
                backgroundEnabled: false, progress: progress)
            if committedRecord(from: result) != nil {
                let confirmed = record.replacing(state: .uploadedConfirmed)
                try await store.transitionExtraction(
                    confirmed, expectedState: .handedOff,
                    authorization: context.authorization)
            }
            return .success(result)
        } catch {
            if let failure = error as? PhotoResourceTransferFailure { return .failure(failure) }
            return .failure(.storageFailure)
        }
    }

    func pauseForegroundUpload(extractionId: UUID) async throws {
        guard let context = await contextProvider.currentContext(),
            let record = try await store.extraction(
                operationId: extractionId, authorization: context.authorization),
            record.deviceId == context.deviceId,
            record.state == .handedOff,
            let uploadId = record.relatedUploadOperationId
        else { throw PhotoResourceTransferFailure.ineligibleObservation }
        let operationId = try UploadOperationId(validating: uploadId)
        try await uploads.pauseUserTransfer(id: operationId)
    }

    func currentExtraction(extractionId: UUID) async throws -> PhotoResourceExtractionRecord? {
        guard let context = await contextProvider.currentContext() else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        return try await store.extraction(
            operationId: extractionId, authorization: context.authorization)
    }

    private func validateCurrentContext(
        matching expected: PhotoTransferSessionContext
    ) async throws {
        guard await permissionAdapter.currentPermissionState().isAuthorized else {
            throw PhotoResourceTransferFailure.permissionDenied
        }
        guard let current = await contextProvider.currentContext(),
            current.authorization == expected.authorization,
            current.deviceId == expected.deviceId,
            current.configuration == expected.configuration,
            current.configuration.isOptedIn, !current.configuration.pauseDiscovery
        else { throw PhotoResourceTransferFailure.staleAuthorization }
    }

    private func validateHandoffEligibility(
        _ record: PhotoResourceExtractionRecord,
        matching expected: PhotoTransferSessionContext
    ) async throws {
        try await validateCurrentContext(matching: expected)
        guard
            let current = try await store.extraction(
                operationId: record.operationId, authorization: expected.authorization),
            current.state == .handoffPending,
            current.relatedUploadOperationId == record.relatedUploadOperationId,
            current.deviceId == expected.deviceId
        else { throw PhotoResourceTransferFailure.staleAuthorization }
        let observations = try await store.fetchObservations(
            profileId: current.profileId, ownerId: current.ownerId)
        guard
            let observation = observations.first(where: {
                $0.localIdentifier == current.selection.localIdentifier
            }), observation.discoveryGeneration == current.observationGeneration,
            observation.sourceAvailability == .available, !observation.isExcluded
        else { throw PhotoResourceTransferFailure.ineligibleObservation }
        try await validateCurrentContext(matching: expected)
    }

    private func committedRecord(from result: FileUploadResult) -> UploadRecord? {
        let record: UploadRecord
        switch result {
        case .completed(let value): record = value
        case .scheduled(let value): record = value
        case .stopped(let value?, _): record = value
        case .stopped(nil, _): return nil
        }
        return record.state == .committed && record.completion != nil ? record : nil
    }
}

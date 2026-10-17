import Foundation
import UniformTypeIdentifiers

enum PhotoBackupForegroundRunState: Sendable, Equatable {
    case idle
    case suspended
    case discoveryDisabled
    case permissionUnavailable
    case discoveryIncomplete
    case networkRestricted
    case destinationUnavailable
    case queueAtCapacity
    case waitingForUserUpload
    case processed(Int)
    case noEligibleWork
    case failed
}

/// Runs one finite, owner scoped batch while the authenticated app is active. The worker only
/// queues local PhotoKit resources and delegates verified content to P057/P045.
@MainActor
final class PhotoBackupForegroundWorker {
    private let observations: any PhotoLibraryObservationCoordinatorProtocol
    private let store: any PhotoObservationStoreProtocol & PhotoBackupQueueStoreProtocol
    private let extraction: PhotoResourceExtractionCoordinator
    private let handoff: PhotoResourceUploadHandoffCoordinator
    private let contextProvider: any PhotoTransferContextProviding
    private let identityProvider: any PhotoBackupAuthenticatedIdentityProviding
    private let permission: any PhotoLibraryPermissionChecking
    private let network: any PhotoBackupNetworkChecking
    private let libraryCatalog: any LibraryCatalogRepositoryProtocol
    private let nodeRepository: any NodeRepositoryProtocol
    private let bridge: any RustBridgeProtocol
    private let uploads: any PhotoForegroundUploadCoordinatorProtocol
    private let bindingRepository: any PhotoImportBindingRepositoryProtocol
    private let retentionRepository: any PhotoBackupRetentionRepositoryProtocol

    private var isForegroundActive = false
    private var lifecycleGeneration: UInt64 = 0
    private var runningTask: Task<Void, Never>?
    private var rerunRequested = false
    private(set) var lastRunState: PhotoBackupForegroundRunState = .idle
    var bindingAvailability: PhotoImportBindingAvailability {
        bindingRepository.availability
    }
    var retentionAvailability: PhotoBackupRetentionAvailability {
        retentionRepository.availability
    }

    private let maximumAssetsPerQueuePass = 500
    private let maximumCandidatesPerBatch = 2
    private let maximumPendingCandidates = 1_000
    private let claimLeaseDuration: TimeInterval = 3_600

    init(
        observations: any PhotoLibraryObservationCoordinatorProtocol,
        store: any PhotoObservationStoreProtocol & PhotoBackupQueueStoreProtocol,
        extraction: PhotoResourceExtractionCoordinator,
        handoff: PhotoResourceUploadHandoffCoordinator,
        contextProvider: any PhotoTransferContextProviding,
        identityProvider: any PhotoBackupAuthenticatedIdentityProviding,
        permission: any PhotoLibraryPermissionChecking,
        network: any PhotoBackupNetworkChecking,
        libraryCatalog: any LibraryCatalogRepositoryProtocol,
        nodeRepository: any NodeRepositoryProtocol,
        bridge: any RustBridgeProtocol,
        uploads: any PhotoForegroundUploadCoordinatorProtocol,
        bindingRepository: any PhotoImportBindingRepositoryProtocol =
            UnavailablePhotoImportBindingRepository(),
        retentionRepository: any PhotoBackupRetentionRepositoryProtocol =
            UnavailablePhotoBackupRetentionRepository()
    ) {
        self.observations = observations
        self.store = store
        self.extraction = extraction
        self.handoff = handoff
        self.contextProvider = contextProvider
        self.identityProvider = identityProvider
        self.permission = permission
        self.network = network
        self.libraryCatalog = libraryCatalog
        self.nodeRepository = nodeRepository
        self.bridge = bridge
        self.uploads = uploads
        self.bindingRepository = bindingRepository
        self.retentionRepository = retentionRepository
    }

    func updateLifecycle(isActive: Bool) {
        guard isForegroundActive != isActive else { return }
        isForegroundActive = isActive
        lifecycleGeneration &+= 1
        if isActive {
            requestBoundedRun()
        } else {
            runningTask?.cancel()
            rerunRequested = false
            Task { await extraction.cancelAllPreparations() }
        }
    }

    func policyDidChange(automaticEnabled: Bool) {
        if automaticEnabled {
            requestBoundedRun()
        } else {
            lifecycleGeneration &+= 1
            runningTask?.cancel()
            rerunRequested = false
            Task { await extraction.cancelAllPreparations() }
        }
    }

    func invalidateSession() {
        isForegroundActive = false
        lifecycleGeneration &+= 1
        rerunRequested = false
        runningTask?.cancel()
        Task { await extraction.cancelAllPreparations() }
    }

    func requestBoundedRun() {
        guard isForegroundActive else {
            lastRunState = .suspended
            return
        }
        guard runningTask == nil else {
            rerunRequested = true
            return
        }
        let generation = lifecycleGeneration
        runningTask = Task { @MainActor in
            await performBoundedRun(generation: generation)
            self.finishRun(generation: generation)
        }
    }

    func runBoundedPassNow() async {
        requestBoundedRun()
        let activeRun = runningTask
        await activeRun?.value
    }

    func queueSummary() async -> PhotoBackupQueueSummary {
        guard let context = await contextProvider.currentContext() else { return .init() }
        return (try? await store.backupQueueSummary(authorization: context.authorization))
            ?? .init()
    }

    func livePhotoGroupEvidence() async -> [PhotoBackupLivePhotoGroupEvidence] {
        guard let context = await contextProvider.currentContext(),
            let candidates = try? await store.backupCandidates(
                authorization: context.authorization, limit: 1_000)
        else { return [] }
        let grouped = Dictionary(
            grouping: candidates.compactMap { candidate in
                candidate.groupIdentity.map { ($0, candidate) }
            }, by: { $0.0 })
        return grouped.map { entry in
            PhotoBackupLivePhotoGroupEvidence(
                groupIdentity: entry.key, candidates: entry.value.map { $0.1 })
        }.sorted { $0.id < $1.id }
    }

    private func finishRun(generation: UInt64) {
        runningTask = nil
        guard generation == lifecycleGeneration, isForegroundActive, rerunRequested else { return }
        rerunRequested = false
        requestBoundedRun()
    }

    private func performBoundedRun(generation: UInt64) async {
        do {
            guard isCurrentRun(generation) else {
                lastRunState = .suspended
                return
            }
            guard let identity = await identityProvider.currentIdentity() else {
                lastRunState = .suspended
                return
            }
            let initialPolicy = try await store.fetchConfiguration(profileId: identity.profileId)
            guard initialPolicy.discoveryEnabled, !initialPolicy.pauseDiscovery else {
                lastRunState = .discoveryDisabled
                return
            }
            guard await permission.currentPermissionState().isAuthorized else {
                lastRunState = .permissionUnavailable
                return
            }
            guard isCurrentRun(generation) else {
                lastRunState = .suspended
                return
            }

            let scan = try await observations.startObservation(
                profileId: identity.profileId, ownerId: identity.ownerId,
                sessionRevision: identity.sessionRevision)
            guard isCurrentRun(generation) else {
                lastRunState = .suspended
                return
            }
            guard case .completed(_, let scanGeneration) = scan else {
                lastRunState = .discoveryIncomplete
                return
            }
            guard let context = await contextProvider.currentContext(),
                context.authorization.profileId == identity.profileId,
                context.authorization.ownerId == identity.ownerId,
                context.deviceId == identity.deviceId
            else {
                lastRunState = .discoveryDisabled
                return
            }

            try await extraction.recoverStagingAfterLaunch()
            try await store.adoptRecoverableBackupCandidates(
                authorization: context.authorization, deviceId: context.deviceId)
            try await store.recoverExpiredBackupClaims(
                authorization: context.authorization, before: Date())
            guard context.configuration.canAutomaticallyBackUpInForeground else {
                lastRunState = .discoveryDisabled
                return
            }
            var queueCapacityReached = false
            do {
                try await enqueueOneBoundedObservationPage(
                    context: context, scanGeneration: scanGeneration, runGeneration: generation)
            } catch PhotoResourceTransferFailure.queueCapacityReached {
                queueCapacityReached = true
            }

            guard isCurrentRun(generation) else {
                lastRunState = .suspended
                return
            }
            guard networkAllows(context.configuration) else {
                lastRunState = .networkRestricted
                return
            }
            guard let target = try await validatedDestination(context.configuration) else {
                lastRunState = .destinationUnavailable
                return
            }
            var processed = 0
            for _ in 0..<maximumCandidatesPerBatch {
                guard isCurrentRun(generation) else {
                    lastRunState = .suspended
                    return
                }
                guard networkAllows(context.configuration) else {
                    lastRunState = .networkRestricted
                    return
                }
                if uploads.hasActiveForegroundOperation {
                    lastRunState = .waitingForUserUpload
                    return
                }
                guard let currentContext = await contextProvider.currentContext(),
                    currentContext.authorization == context.authorization,
                    currentContext.deviceId == context.deviceId,
                    currentContext.configuration == context.configuration,
                    currentContext.configuration.canAutomaticallyBackUpInForeground
                else {
                    lastRunState = .discoveryDisabled
                    return
                }
                guard
                    let candidate = try await store.claimNextBackupCandidate(
                        authorization: context.authorization, deviceId: context.deviceId,
                        claimId: UUID(), leaseUntil: Date().addingTimeInterval(claimLeaseDuration),
                        policyRevision: context.configuration.policyRevision)
                else { break }
                try Task.checkCancellation()
                await process(
                    candidate, context: context, target: target, generation: generation)
                processed += 1
            }
            if processed > 0 {
                lastRunState = .processed(processed)
            } else if queueCapacityReached {
                lastRunState = .queueAtCapacity
            } else {
                lastRunState = .noEligibleWork
            }
        } catch is CancellationError {
            lastRunState = .suspended
        } catch {
            lastRunState = .failed
        }
    }

    private func enqueueOneBoundedObservationPage(
        context: PhotoTransferSessionContext, scanGeneration: Int64, runGeneration: UInt64
    ) async throws {
        let cursor = try await store.backupDiscoveryCursor(
            authorization: context.authorization, deviceId: context.deviceId,
            policyRevision: context.configuration.policyRevision,
            scanGeneration: scanGeneration)
        let page = try await store.fetchObservationPage(
            profileId: context.authorization.profileId,
            ownerId: context.authorization.ownerId, scanGeneration: scanGeneration,
            after: cursor, limit: maximumAssetsPerQueuePass + 1)
        let hasMore = page.count > maximumAssetsPerQueuePass
        let boundedPage = Array(page.prefix(maximumAssetsPerQueuePass))
        guard !boundedPage.isEmpty else {
            return
        }

        var candidates: [PhotoBackupCandidate] = []
        candidates.reserveCapacity(100)
        var visited = 0
        for observation in boundedPage
        where observation.sourceAvailability == .available
            && !observation.isExcluded && eligibleAsset(observation, policy: context.configuration)
        {
            try Task.checkCancellation()
            guard isCurrentRun(runGeneration) else { throw CancellationError() }
            let selections: [PhotoResourceSelection]
            do {
                selections = try await extraction.availableSelections(for: observation)
            } catch let failure as PhotoResourceTransferFailure {
                if [.permissionDenied, .staleAuthorization].contains(failure) { throw failure }
                if failure == .sourceUnavailable {
                    try await store.markSourceUnavailable(
                        localIdentifier: observation.localIdentifier,
                        reason: .sourceUnavailable, profileId: context.authorization.profileId,
                        ownerId: context.authorization.ownerId)
                }
                visited += 1
                continue
            }
            for selection in selections
            where eligibleSelection(
                selection, observation: observation, policy: context.configuration)
            {
                let sourceRevision = PhotoBackupCandidate.sourceRevision(for: observation)
                let groupIdentity =
                    observation.assetKind == .livePhoto
                    ? "\(context.deviceId)|\(observation.localIdentifier)|\(sourceRevision)" : nil
                candidates.append(
                    PhotoBackupCandidate(
                        profileId: context.authorization.profileId,
                        ownerId: context.authorization.ownerId,
                        deviceId: context.deviceId,
                        clientLibraryScope: observation.permissionScope.rawValue,
                        localIdentifier: observation.localIdentifier,
                        sourceRevision: sourceRevision,
                        selection: selection,
                        observationGeneration: observation.discoveryGeneration,
                        policyRevision: context.configuration.policyRevision,
                        authorizationGeneration: context.authorization.generation,
                        destinationLibraryIdentifier: context.configuration.targetLibraryIdentifier
                            ?? "",
                        destinationParentIdentifier: context.configuration.targetParentIdentifier
                            ?? "",
                        groupIdentity: groupIdentity))
                if candidates.count >= 100 {
                    _ = try await store.enqueueBackupCandidates(
                        candidates, authorization: context.authorization,
                        maximumPending: maximumPendingCandidates)
                    candidates.removeAll(keepingCapacity: true)
                }
            }
            visited += 1
            if visited.isMultiple(of: 100) {
                if !candidates.isEmpty {
                    _ = try await store.enqueueBackupCandidates(
                        candidates, authorization: context.authorization,
                        maximumPending: maximumPendingCandidates)
                    candidates.removeAll(keepingCapacity: true)
                }
                try await store.saveBackupDiscoveryCursor(
                    observation.localIdentifier, authorization: context.authorization,
                    deviceId: context.deviceId,
                    policyRevision: context.configuration.policyRevision,
                    scanGeneration: scanGeneration)
            }
        }
        if !candidates.isEmpty {
            _ = try await store.enqueueBackupCandidates(
                candidates, authorization: context.authorization,
                maximumPending: maximumPendingCandidates)
        }
        if hasMore {
            guard let last = boundedPage.last else { return }
            try await store.saveBackupDiscoveryCursor(
                last.localIdentifier, authorization: context.authorization,
                deviceId: context.deviceId, policyRevision: context.configuration.policyRevision,
                scanGeneration: scanGeneration)
        } else {
            guard let last = boundedPage.last else { return }
            try await store.saveBackupDiscoveryCursor(
                last.localIdentifier, authorization: context.authorization,
                deviceId: context.deviceId,
                policyRevision: context.configuration.policyRevision,
                scanGeneration: scanGeneration)
        }
    }

    private func process(
        _ claimed: PhotoBackupCandidate,
        context: PhotoTransferSessionContext,
        target: (library: Library, parent: NodeId),
        generation: UInt64
    ) async {
        var candidate = claimed
        let claimState = candidate.claimPreviousState ?? .queued
        do {
            guard isCurrentRun(generation), networkAllows(context.configuration) else {
                try await releaseClaim(
                    candidate, to: claimState, failure: .networkUnavailable,
                    authorization: context.authorization)
                return
            }
            if uploads.hasActiveForegroundOperation {
                try await releaseClaim(
                    candidate, to: claimState, failure: nil,
                    authorization: context.authorization)
                lastRunState = .waitingForUserUpload
                return
            }

            if candidate.uploadOperationId != nil
                && (candidate.extractionOperationId == nil
                    || candidate.policyRevision != context.configuration.policyRevision
                    || candidate.destinationLibraryIdentifier != target.library.id.rawValue
                    || candidate.destinationParentIdentifier != target.parent.rawValue)
            {
                await reconcileExistingCandidate(
                    candidate, context: context, target: target,
                    generation: generation)
                return
            }
            if let uploadOperationId = candidate.uploadOperationId,
                let extractionOperationId = candidate.extractionOperationId
            {
                let uploading = candidate.replacing(
                    state: .uploading, claimId: candidate.claimId,
                    claimExpiresAt: candidate.claimExpiresAt,
                    claimPreviousState: claimState,
                    uploadOperationId: uploadOperationId)
                try await store.transitionBackupCandidate(
                    uploading, expectedState: .claimed, authorization: context.authorization)
                candidate = uploading
                let result = await handoff.resumeAutomaticCandidate(
                    candidate, extractionId: extractionOperationId,
                    target: .createFile(
                        library: target.library.id, parent: target.parent,
                        name: filename(for: candidate)),
                    libraryStatus: target.library.status,
                    progress: { _ in })
                await recordUploadResult(result, candidate: candidate, context: context)
                return
            }

            var priorExtraction: PhotoResourceExtractionRecord?
            if let extractionId = candidate.extractionOperationId {
                priorExtraction = try await handoff.currentExtraction(extractionId: extractionId)
                guard priorExtraction != nil else {
                    try await releaseClaim(
                        candidate, to: .failedTerminal,
                        failure: .stagingCorrupt, authorization: context.authorization)
                    return
                }
            }
            if priorExtraction == nil {
                let recent = try await extraction.recentExtractions()
                priorExtraction = recent.first(where: {
                    !$0.userInitiated && $0.deviceId == candidate.deviceId
                        && $0.selection == candidate.selection
                        && $0.observationGeneration == candidate.observationGeneration
                        && $0.policyVersion == Int(candidate.policyRevision)
                })
            }

            let extractionRecord: PhotoResourceExtractionRecord
            if let existing = priorExtraction,
                [.verifiedReady, .handoffPending, .handedOff, .uploadedConfirmed].contains(
                    existing.state)
            {
                extractionRecord = existing
                let queuedState: PhotoBackupQueueState =
                    existing.relatedUploadOperationId == nil
                    ? .verifiedLocally : .uploading
                let refreshed = candidate.replacing(
                    state: queuedState, claimId: candidate.claimId,
                    claimExpiresAt: candidate.claimExpiresAt,
                    claimPreviousState: claimState,
                    extractionOperationId: existing.operationId,
                    uploadOperationId: existing.relatedUploadOperationId,
                    verifiedByteCount: existing.byteCount,
                    verifiedSHA256: existing.sha256)
                try await store.transitionBackupCandidate(
                    refreshed, expectedState: .claimed, authorization: context.authorization)
                candidate = refreshed
                if existing.relatedUploadOperationId != nil {
                    let result = await handoff.resumeAutomaticCandidate(
                        candidate, extractionId: existing.operationId,
                        target: .createFile(
                            library: target.library.id, parent: target.parent,
                            name: filename(for: candidate)),
                        libraryStatus: target.library.status, progress: { _ in })
                    await recordUploadResult(result, candidate: candidate, context: context)
                    return
                }
            } else {
                let extracting = candidate.replacing(
                    state: .extracting, claimId: candidate.claimId,
                    claimExpiresAt: candidate.claimExpiresAt,
                    claimPreviousState: claimState)
                try await store.transitionBackupCandidate(
                    extracting, expectedState: .claimed, authorization: context.authorization)
                candidate = extracting
                let prepared = try await extraction.prepareAutomaticSelection(
                    selection: candidate.selection,
                    observationGeneration: candidate.observationGeneration,
                    policyRevision: candidate.policyRevision)
                extractionRecord = prepared
                let verified = candidate.replacing(
                    state: .verifiedLocally, claimId: candidate.claimId,
                    claimExpiresAt: candidate.claimExpiresAt,
                    claimPreviousState: claimState,
                    extractionOperationId: prepared.operationId,
                    verifiedByteCount: prepared.byteCount,
                    verifiedSHA256: prepared.sha256)
                try await store.transitionBackupCandidate(
                    verified, expectedState: .extracting, authorization: context.authorization)
                candidate = verified
            }

            guard isCurrentRun(generation), networkAllows(context.configuration),
                !uploads.hasActiveForegroundOperation,
                let currentContext = await contextProvider.currentContext(),
                currentContext.authorization == context.authorization,
                currentContext.deviceId == context.deviceId,
                currentContext.configuration == context.configuration,
                currentContext.configuration.canAutomaticallyBackUpInForeground
            else {
                try await releaseClaim(
                    candidate, to: .verifiedLocally,
                    failure: .policyChanged, authorization: context.authorization)
                return
            }

            var pendingHandoff = extractionRecord
            let uploadOperationId: UploadOperationId
            if let existing = extractionRecord.relatedUploadOperationId {
                uploadOperationId = try UploadOperationId(validating: existing)
                if extractionRecord.state == .verifiedReady {
                    pendingHandoff = extractionRecord.replacing(
                        state: .handoffPending,
                        relatedUploadOperationId: uploadOperationId.rawValue)
                    try await store.transitionExtraction(
                        pendingHandoff, expectedState: .verifiedReady,
                        authorization: context.authorization)
                }
            } else {
                uploadOperationId = UploadOperationId()
                pendingHandoff = extractionRecord.replacing(
                    state: .handoffPending,
                    relatedUploadOperationId: uploadOperationId.rawValue)
                try await store.transitionExtraction(
                    pendingHandoff, expectedState: .verifiedReady,
                    authorization: context.authorization)
            }
            let extractionId = candidate.extractionOperationId ?? pendingHandoff.operationId
            let uploading = candidate.replacing(
                state: .uploading, claimId: candidate.claimId,
                claimExpiresAt: candidate.claimExpiresAt,
                claimPreviousState: claimState,
                extractionOperationId: extractionId,
                uploadOperationId: uploadOperationId.rawValue)
            try await store.transitionBackupCandidate(
                uploading, expectedState: candidate.state,
                authorization: context.authorization)
            candidate = uploading
            let handoffResult = await handoff.uploadSelected(
                extractionId: extractionId,
                target: .createFile(
                    library: target.library.id, parent: target.parent,
                    name: filename(for: candidate)),
                libraryStatus: target.library.status,
                progress: { _ in })
            await recordUploadResult(handoffResult, candidate: candidate, context: context)
        } catch is CancellationError {
            try? await releaseClaim(
                candidate, to: claimState, failure: nil,
                authorization: context.authorization)
        } catch let failure as PhotoResourceTransferFailure {
            let state: PhotoBackupQueueState
            if failure == .sourceUnavailable {
                state = candidate.attemptCount >= 5 ? .failedTerminal : .sourceUnavailable
            } else {
                state = .failedRecoverable
            }
            try? await releaseClaim(
                candidate, to: state, failure: safeFailure(for: failure),
                authorization: context.authorization)
        } catch {
            try? await releaseClaim(
                candidate, to: .failedRecoverable, failure: .stagingCorrupt,
                authorization: context.authorization)
        }
    }

    private func recordUploadResult(
        _ result: Result<FileUploadResult, PhotoResourceTransferFailure>,
        candidate: PhotoBackupCandidate,
        context: PhotoTransferSessionContext
    ) async {
        guard let upload = uploadRecord(from: result) else {
            try? await releaseClaim(
                candidate, to: .reconciliationPending, failure: .uploadOutcomeUnknown,
                authorization: context.authorization)
            return
        }
        if upload.state == .committed, let completion = upload.completion,
            completion.expected.byteLength == candidate.verifiedByteCount,
            completion.expected.sha256 == candidate.verifiedSHA256,
            !completion.fileVersionId.rawValue.isEmpty
        {
            let confirmed = candidate.replacing(
                state: .fileUploadedAwaitingPhotoBinding,
                claimId: candidate.claimId, claimExpiresAt: nil, claimPreviousState: nil,
                fileVersionIdentifier: completion.fileVersionId.rawValue,
                safeFailure: .photoBindingUnavailable, retryAfter: nil,
                preserveRetryAfter: false)
            do {
                try await store.transitionBackupCandidate(
                    confirmed, expectedState: .uploading,
                    authorization: context.authorization)
                await reconcilePhotoAuthority(confirmed, authorization: context.authorization)
            } catch {
                try? await releaseClaim(
                    candidate, to: .reconciliationPending, failure: .uploadOutcomeUnknown,
                    authorization: context.authorization)
            }
            return
        }
        try? await releaseClaim(
            candidate, to: .reconciliationPending, failure: .uploadOutcomeUnknown,
            authorization: context.authorization)
    }

    private func reconcileExistingCandidate(
        _ candidate: PhotoBackupCandidate,
        context: PhotoTransferSessionContext,
        target: (library: Library, parent: NodeId),
        generation: UInt64
    ) async {
        guard let uploadRaw = candidate.uploadOperationId,
            let byteCount = candidate.verifiedByteCount,
            let sha256 = candidate.verifiedSHA256,
            (try? UploadExpectedContent(byteLength: byteCount, sha256: sha256)) != nil
        else {
            try? await releaseClaim(
                candidate, to: .reconciliationPending,
                failure: .uploadOutcomeUnknown, authorization: context.authorization)
            return
        }
        var currentCandidate = candidate
        do {
            let uploadId = try UploadOperationId(validating: uploadRaw)
            let originalLibrary = try await LibraryId.validated(
                candidate.destinationLibraryIdentifier, using: bridge)
            let reconciling = candidate.replacing(
                state: .reconciliationPending, claimId: candidate.claimId,
                claimExpiresAt: nil, claimPreviousState: nil,
                safeFailure: .uploadOutcomeUnknown)
            try await store.transitionBackupCandidate(
                reconciling, expectedState: .claimed, authorization: context.authorization)
            currentCandidate = reconciling

            guard let currentContext = await contextProvider.currentContext(),
                currentContext.authorization == context.authorization,
                currentContext.deviceId == context.deviceId
            else {
                try await releaseClaim(
                    reconciling, to: .reconciliationPending,
                    failure: .uploadOutcomeUnknown, authorization: context.authorization)
                return
            }

            let status = await uploads.reconcileExisting(id: uploadId, library: originalLibrary)
            if case .success(let record) = status, record.state == .committed {
                await recordReconciledUpload(record, candidate: reconciling, context: context)
                return
            }

            let canResumeExactOperation =
                candidate.policyRevision == context.configuration.policyRevision
                && candidate.destinationLibraryIdentifier
                    == context.configuration.targetLibraryIdentifier
                && candidate.destinationParentIdentifier
                    == context.configuration.targetParentIdentifier
                && context.configuration.canAutomaticallyBackUpInForeground
                && networkAllows(context.configuration) && isCurrentRun(generation)
                && !uploads.hasActiveForegroundOperation
                && originalLibrary == target.library.id
            guard canResumeExactOperation else {
                try await releaseClaim(
                    reconciling, to: .reconciliationPending,
                    failure: .uploadOutcomeUnknown, authorization: context.authorization)
                return
            }
            guard let currentContext = await contextProvider.currentContext(),
                currentContext.authorization == context.authorization,
                currentContext.deviceId == context.deviceId,
                currentContext.configuration == context.configuration,
                currentContext.configuration.canAutomaticallyBackUpInForeground
            else {
                try await releaseClaim(
                    reconciling, to: .reconciliationPending,
                    failure: .policyChanged, authorization: context.authorization)
                return
            }
            let uploading = reconciling.replacing(
                state: .uploading, claimId: reconciling.claimId,
                claimExpiresAt: nil, claimPreviousState: nil, safeFailure: nil)
            try await store.transitionBackupCandidate(
                uploading, expectedState: .reconciliationPending,
                authorization: context.authorization)
            currentCandidate = uploading
            let result = await uploads.resume(
                id: uploadId, library: target.library.id,
                libraryStatus: target.library.status, backgroundEnabled: false,
                progress: { _ in })
            await recordUploadResult(result, candidate: uploading, context: context)
        } catch is CancellationError {
            try? await releaseClaim(
                currentCandidate, to: .reconciliationPending,
                failure: .uploadOutcomeUnknown, authorization: context.authorization)
        } catch {
            try? await releaseClaim(
                currentCandidate, to: .reconciliationPending,
                failure: .uploadOutcomeUnknown, authorization: context.authorization)
        }
    }

    private func recordReconciledUpload(
        _ upload: UploadRecord,
        candidate: PhotoBackupCandidate,
        context: PhotoTransferSessionContext
    ) async {
        guard upload.state == .committed, let completion = upload.completion,
            completion.expected.byteLength == candidate.verifiedByteCount,
            completion.expected.sha256 == candidate.verifiedSHA256,
            !completion.fileVersionId.rawValue.isEmpty
        else {
            try? await releaseClaim(
                candidate, to: .failedTerminal,
                failure: .uploadSessionConflict, authorization: context.authorization)
            return
        }
        let confirmed = candidate.replacing(
            state: .fileUploadedAwaitingPhotoBinding, claimId: candidate.claimId,
            claimExpiresAt: nil, claimPreviousState: nil,
            fileVersionIdentifier: completion.fileVersionId.rawValue,
            retryAfter: nil, preserveRetryAfter: false, safeFailure: .photoBindingUnavailable)
        do {
            try await store.transitionBackupCandidate(
                confirmed, expectedState: .reconciliationPending,
                authorization: context.authorization)
            await reconcilePhotoAuthority(confirmed, authorization: context.authorization)
        } catch {
            try? await releaseClaim(
                candidate, to: .reconciliationPending,
                failure: .uploadOutcomeUnknown, authorization: context.authorization)
        }
    }

    private func reconcilePhotoAuthority(
        _ candidate: PhotoBackupCandidate, authorization: PhotoAuthorizationSession
    ) async {
        guard let result = try? await bindingRepository.bind(candidate) else { return }
        switch result {
        case .photoBindingConfirmed(let identifier) where !identifier.isEmpty:
            let bound = candidate.replacing(
                state: .photoBindingConfirmed, claimId: candidate.claimId,
                claimExpiresAt: nil, claimPreviousState: nil,
                photoAssetIdentifier: identifier, safeFailure: nil)
            do {
                try await store.transitionBackupCandidate(
                    bound, expectedState: .fileUploadedAwaitingPhotoBinding,
                    authorization: authorization)
            } catch { return }
            await reconcileRetention(bound, authorization: authorization)
        case .partial:
            let partial = candidate.replacing(
                state: .partial, claimId: candidate.claimId,
                claimExpiresAt: nil, claimPreviousState: nil,
                safeFailure: .photoBindingUnavailable)
            try? await store.transitionBackupCandidate(
                partial, expectedState: .fileUploadedAwaitingPhotoBinding,
                authorization: authorization)
        default:
            break
        }
    }

    private func reconcileRetention(
        _ candidate: PhotoBackupCandidate, authorization: PhotoAuthorizationSession
    ) async {
        let awaitingProof = candidate.replacing(
            state: .awaitingBackupRetentionProof, claimId: candidate.claimId,
            claimExpiresAt: nil, claimPreviousState: nil,
            safeFailure: .backupRetentionUnavailable)
        do {
            try await store.transitionBackupCandidate(
                awaitingProof, expectedState: .photoBindingConfirmed,
                authorization: authorization)
        } catch { return }
        let result = await retentionRepository.confirmRetention(for: awaitingProof)
        guard case .authoritativeBackupConfirmed(let snapshotIdentifier) = result,
            !snapshotIdentifier.isEmpty
        else { return }
        let confirmed = awaitingProof.replacing(
            state: .authoritativeBackupConfirmed, claimId: awaitingProof.claimId,
            claimExpiresAt: nil, claimPreviousState: nil,
            backupSnapshotIdentifier: snapshotIdentifier,
            backupRetentionConfirmedAt: Date(), safeFailure: nil)
        try? await store.transitionBackupCandidate(
            confirmed, expectedState: .awaitingBackupRetentionProof,
            authorization: authorization)
    }

    private func uploadRecord(
        from result: Result<FileUploadResult, PhotoResourceTransferFailure>
    ) -> UploadRecord? {
        guard case .success(let outcome) = result else { return nil }
        switch outcome {
        case .completed(let record), .scheduled(let record): record
        case .stopped(let record, _): record
        }
    }

    private func releaseClaim(
        _ candidate: PhotoBackupCandidate,
        to state: PhotoBackupQueueState,
        failure: PhotoBackupSafeFailure?,
        authorization: PhotoAuthorizationSession
    ) async throws {
        let terminal =
            state == .failedRecoverable && candidate.uploadOperationId == nil
            && candidate.attemptCount >= 5
        let nextState: PhotoBackupQueueState = terminal ? .failedTerminal : state
        let retryAfter: Date?
        if nextState == .failedRecoverable || nextState == .reconciliationPending
            || nextState == .sourceUnavailable
        {
            let exponent = min(max(candidate.attemptCount - 1, 0), 10)
            let delay = min(30 * pow(2, Double(exponent)), 6 * 60 * 60)
            retryAfter = Date().addingTimeInterval(delay)
        } else {
            retryAfter = nil
        }
        let released = candidate.replacing(
            state: nextState, claimId: candidate.claimId,
            claimExpiresAt: nil, claimPreviousState: nil,
            retryAfter: retryAfter, preserveRetryAfter: false,
            safeFailure: failure)
        try await store.transitionBackupCandidate(
            released, expectedState: candidate.state, authorization: authorization)
    }

    private func validatedDestination(_ policy: PhotoBackupConfiguration) async throws
        -> (library: Library, parent: NodeId)?
    {
        guard let libraryRaw = policy.targetLibraryIdentifier,
            let parentRaw = policy.targetParentIdentifier
        else { return nil }
        let libraryId = try await LibraryId.validated(libraryRaw, using: bridge)
        let parentId = try await NodeId.validated(parentRaw, using: bridge)
        guard case .loaded(let libraries) = await libraryCatalog.listLibraries(),
            let library = libraries.first(where: { $0.id == libraryId }),
            library.status == .active
        else { return nil }
        if parentId == library.rootNodeId { return (library, parentId) }
        guard
            case .loaded(let parent) = await nodeRepository.getNodeMetadata(
                libraryId: libraryId, nodeId: parentId),
            parent.libraryId == libraryId, parent.id == parentId,
            parent.kind == .directory, parent.state == .active
        else { return nil }
        return (library, parentId)
    }

    private func eligibleAsset(
        _ observation: PhotoAssetObservation, policy: PhotoBackupConfiguration
    ) -> Bool {
        switch observation.assetKind {
        case .stillPhoto, .screenshot: policy.includeStillImages
        case .video: policy.includeVideos
        case .livePhoto: policy.includeLivePhotos
        case .unknown: false
        }
    }

    private func eligibleSelection(
        _ selection: PhotoResourceSelection,
        observation: PhotoAssetObservation,
        policy: PhotoBackupConfiguration
    ) -> Bool {
        guard selection.role != .auxiliary else { return false }
        if observation.assetKind == .livePhoto && !policy.includeLivePhotos { return false }
        if selection.role == .primaryImage && !policy.includeStillImages
            && observation.assetKind != .livePhoto
        {
            return false
        }
        if selection.role == .primaryVideo && !policy.includeVideos { return false }
        if selection.role == .motionVideo && !policy.includeLivePhotos { return false }
        if policy.renditionPolicy == .originalOnly,
            ![.original, .pairedMotion].contains(selection.rendition)
        {
            return false
        }
        return true
    }

    private func networkAllows(_ policy: PhotoBackupConfiguration) -> Bool {
        let path = network.currentState()
        guard path.isSatisfied else { return false }
        if policy.respectLowDataMode && path.isConstrained { return false }
        if path.isCellular {
            return policy.networkRestriction == .wifiAndCellular
                && policy.allowCellularTransfer
        }
        return path.isWiFi
    }

    private func filename(for candidate: PhotoBackupCandidate) -> String {
        let suffix =
            candidate.selection.uniformTypeIdentifier.flatMap {
                UTType($0)?.preferredFilenameExtension
            } ?? "media"
        let role = candidate.selection.role.rawValue.replacingOccurrences(
            of: "[^A-Za-z0-9]", with: "-", options: .regularExpression)
        return "Synveil-Photo-\(role)-\(candidate.candidateId.uuidString.prefix(8)).\(suffix)"
    }

    private func safeFailure(for failure: PhotoResourceTransferFailure) -> PhotoBackupSafeFailure {
        switch failure {
        case .permissionDenied, .staleAuthorization: .permissionRevoked
        case .sourceUnavailable: .iCloudOriginalUnavailable
        case .sourceChanged: .sourceChanged
        case .insufficientStorage, .stagingQuotaExceeded: .deviceStorageFull
        case .integrityMismatch, .storageFailure: .stagingCorrupt
        case .configurationDisabled: .policyChanged
        case .ineligibleObservation: .limitedSelectionChanged
        case .queueCapacityReached: .queueCapacityReached
        default: .networkUnavailable
        }
    }

    private func isCurrentRun(_ generation: UInt64) -> Bool {
        isForegroundActive && generation == lifecycleGeneration && !Task.isCancelled
    }
}

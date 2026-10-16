import Foundation

enum FileUploadResult: Sendable {
    case completed(UploadRecord)
    case scheduled(UploadRecord)
    case stopped(UploadRecord?, UploadFailure)
}

private enum UploadActionResult: Sendable {
    case transfer(FileUploadResult)
    case record(Result<UploadRecord, UploadFailure>)
}

/// Shared foreground/upload business-logic owner. All progress is a SQLite-confirmed server offset, never bytes
/// handed to URLSession. Each user action performs at most one recovery/status pass.
@MainActor
protocol FileUploadActivityReaderProtocol {
    func saved(library: LibraryId, limit: Int) async throws -> [UploadRecord]
    func visibleSaved(library: LibraryId) async throws -> [UploadRecord]
    func backgroundJob(_ id: UploadOperationId) async throws -> BackgroundUploadJob?
}

@MainActor
final class FileUploadCoordinator: FileUploadActivityReaderProtocol {
    private let repository: AuthenticatedUploadRepository
    private let database: MutationQueueSQLiteStore
    private let staging: UploadStagingStore
    let background: BackgroundUploadCoordinator?

    func backgroundJob(_ id: UploadOperationId) async throws -> BackgroundUploadJob? {
        try await background?.job(id)
    }
    private let identityGenerator: any ClientMutationIdentityGeneratorProtocol
    private var active: Task<UploadActionResult, Never>?
    private var activeUploadId: UploadOperationId?
    private var generation: UInt64 = 0
    private(set) var lifecycleGeneration: UInt64 = 0
    var notificationObservation: (@MainActor (UploadRecord) -> Void)?

    init(
        repository: AuthenticatedUploadRepository, database: MutationQueueSQLiteStore,
        staging: UploadStagingStore,
        identityGenerator: any ClientMutationIdentityGeneratorProtocol,
        background: BackgroundUploadCoordinator? = nil
    ) {
        self.repository = repository
        self.database = database
        self.staging = staging
        self.identityGenerator = identityGenerator
        self.background = background
    }

    func start(
        selectedURL: URL, target: UploadTarget? = nil, library: LibraryId,
        parent: NodeId? = nil, libraryStatus: LibraryStatus, backgroundEnabled: Bool = false,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> FileUploadResult {
        guard active == nil else { return .stopped(nil, .duplicateOperation) }
        let resolvedTarget: UploadTarget
        if let target {
            resolvedTarget = target
        } else {
            guard let parent else { return .stopped(nil, .invalidTarget) }
            resolvedTarget = .createFile(
                library: library, parent: parent, name: selectedURL.lastPathComponent)
        }
        guard
            case .transfer(let result) = await launch(
                { [self] captured in
                    .transfer(
                        await executeStart(
                            selectedURL: selectedURL, target: resolvedTarget,
                            libraryStatus: libraryStatus, backgroundEnabled: backgroundEnabled,
                            generation: captured, progress: progress))
                }, failureResult: { .transfer(.stopped(nil, $0)) })
        else {
            return .stopped(nil, .duplicateOperation)
        }
        return result
    }

    func resume(
        id: UploadOperationId, library: LibraryId, libraryStatus: LibraryStatus,
        backgroundEnabled: Bool = false,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> FileUploadResult {
        guard
            case .transfer(let result) = await launch(
                { [self] captured in
                    .transfer(
                        await executeResume(
                            id: id, library: library, libraryStatus: libraryStatus,
                            backgroundEnabled: backgroundEnabled, generation: captured,
                            progress: progress))
                }, failureResult: { .transfer(.stopped(nil, $0)) })
        else {
            return .stopped(nil, .duplicateOperation)
        }
        return result
    }

    func checkServerStatus(
        id: UploadOperationId, library: LibraryId
    ) async -> Result<UploadRecord, UploadFailure> {
        guard
            case .record(let result) = await launch(
                { [self] captured in
                    .record(
                        await executeStatus(
                            id: id, library: library, generation: captured))
                }, failureResult: { .record(.failure($0)) })
        else {
            return .failure(.duplicateOperation)
        }
        return result
    }

    func abort(
        id: UploadOperationId, library: LibraryId
    ) async -> Result<UploadRecord, UploadFailure> {
        guard
            case .record(let result) = await launch(
                { [self] captured in
                    .record(
                        await executeAbort(
                            id: id, library: library, generation: captured))
                }, failureResult: { .record(.failure($0)) })
        else {
            return .failure(.duplicateOperation)
        }
        return result
    }

    func saved(library: LibraryId, limit: Int = 20) async throws -> [UploadRecord] {
        let session = try await repository.provider.begin()
        let scope = try await session.mutationScope(libraryId: library, bridge: repository.bridge)
        try await database.bindSession(scope: scope, credentialId: session.credentialIdentifier)
        let owner = UUID()
        try await database.claimUploadRun(owner: owner)
        do {
            let saved = try await database.uploads(
                scope: scope, credentialId: session.credentialIdentifier,
                limit: limit, bridge: repository.bridge)
            for record in saved where record.state == .committed && !record.stagingReleased {
                _ = await cleanupCommitted(record)
            }
            try await staging.reconcileOrphans(
                keeping: database.unreleasedUploadStagingIds())
            let records = try await database.uploads(
                scope: scope, credentialId: session.credentialIdentifier,
                limit: limit, bridge: repository.bridge)
            try await repository.provider.validate(session)
            await database.finishUploadRun(owner: owner)
            return records
        } catch {
            await database.finishUploadRun(owner: owner)
            throw error
        }
    }

    /// Read-only presentation refresh; no cleanup, network transfer or ownership changes.
    func visibleSaved(library: LibraryId) async throws -> [UploadRecord] {
        let session = try await repository.provider.begin()
        let scope = try await session.mutationScope(libraryId: library, bridge: repository.bridge)
        let records = try await database.uploads(
            scope: scope, credentialId: session.credentialIdentifier,
            limit: 20, bridge: repository.bridge)
        try await repository.provider.validate(session)
        return records
    }

    func cancel() { active?.cancel() }

    /// Scene/app suspension cancels foreground work without withdrawing or adopting any legacy
    /// background job. The generation fences old presentation callbacks after reactivation.
    func suspendForeground(lifecycleGeneration: UInt64) {
        updateLifecycle(lifecycleGeneration: lifecycleGeneration, isActive: false)
    }

    /// Reactivation advances the callback fence without resuming an interrupted transfer.
    func updateLifecycle(lifecycleGeneration: UInt64, isActive: Bool) {
        guard lifecycleGeneration >= self.lifecycleGeneration else { return }
        self.lifecycleGeneration = lifecycleGeneration
        if !isActive { active?.cancel() }
    }

    /// User Pause withdraws the exact job's durable background policy. Scene suspension uses
    /// cancel() instead, so an already approved system transfer can continue.
    func pauseUserTransfer() async throws {
        let id = activeUploadId
        active?.cancel()
        if let id, let background { try await background.pause(id) }
    }

    func invalidateSession() {
        background?.invalidateSession()
        generation &+= 1
        active?.cancel()
    }

    private func launch(
        _ operation: @escaping @MainActor (UInt64) async -> UploadActionResult,
        failureResult: (UploadFailure) -> UploadActionResult
    ) async -> UploadActionResult {
        guard active == nil else { return failureResult(.duplicateOperation) }
        let captured = generation
        let owner = UUID()
        do { try await database.claimUploadRun(owner: owner) } catch {
            return failureResult(UploadPolicy.classify(error))
        }
        let task = Task { @MainActor in
            let result = await operation(captured)
            await database.finishUploadRun(owner: owner)
            return result
        }
        active = task
        let result = await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
        active = nil
        activeUploadId = nil
        return result
    }

    private func executeStart(
        selectedURL: URL, target: UploadTarget, libraryStatus: LibraryStatus,
        backgroundEnabled: Bool, generation captured: UInt64,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> FileUploadResult {
        var record: UploadRecord?
        var session: LibraryRequestScope?
        var stagedId: UploadOperationId?
        var durableRecord = false
        do {
            guard libraryStatus == .active else { throw UploadFailure.invalidTarget }
            if backgroundEnabled {
                guard background?.capability.isAvailable == true else {
                    throw UploadFailure.backgroundTransferUnavailable
                }
            }
            let current = try await repository.provider.begin()
            session = current
            let scope = try await repository.preflight(target, session: current)
            try await database.bindSession(scope: scope, credentialId: current.credentialIdentifier)
            let operationId = UploadOperationId()
            activeUploadId = operationId
            let stagingId = UploadOperationId()
            stagedId = stagingId
            let reservations = try await database.downloadReservations()
            let expected = try await staging.importSelectedFile(
                selectedURL, stagingId: stagingId, durableReservations: reservations)
            try await fence(current, captured)
            let intent = try await UploadIntent.prepare(
                target: target, expected: expected,
                generator: identityGenerator, bridge: repository.bridge)
            let targetNode: NodeId? = target.node
            let prepared = UploadRecord(
                id: operationId, stagingId: stagingId, stagingReleased: false,
                intent: intent, scope: scope,
                credentialId: current.credentialIdentifier,
                originatingSessionId: UUID().uuidString.lowercased(),
                serverSessionId: nil, serverTargetNodeId: targetNode,
                authoritativeOffset: 0, serverState: nil, state: .sourceStaged,
                uncertain: false, attemptCount: 0, completion: nil, failure: nil)
            try await staging.verify(prepared)
            do {
                try await database.prepareUpload(prepared)
            } catch MutationQueueFailure.commitAcknowledgementLost {
                // The source must remain if the INSERT may have committed but exact readback
                // itself fails. Reopen reconciliation can later distinguish an orphan safely.
                durableRecord = true
                guard
                    let readBack = try await database.upload(
                        scope: scope, credentialId: current.credentialIdentifier,
                        id: operationId, bridge: repository.bridge),
                    readBack.intent == intent, readBack.stagingId == stagingId,
                    readBack.state == .sourceStaged
                else { throw UploadFailure.storageFailure }
                record = readBack
                durableRecord = true
            }
            if record == nil {
                record = prepared
                durableRecord = true
            }
            guard let currentRecord = record else { throw UploadFailure.storageFailure }
            progress(UploadProgress(currentRecord))
            return await continueUpload(
                currentRecord, session: current, generation: captured, progress: progress,
                createIfNeeded: true, backgroundEnabled: backgroundEnabled)
        } catch {
            let failure = UploadPolicy.classify(error)
            if !durableRecord, let stagedId {
                await staging.discardUncommitted(stagedId)
                await staging.cancelReservation(stagedId)
            }
            return await stopped(
                record, failure: failure, session: session, generation: captured,
                progress: progress)
        }
    }

    private func executeResume(
        id: UploadOperationId, library: LibraryId, libraryStatus: LibraryStatus,
        backgroundEnabled: Bool, generation captured: UInt64,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> FileUploadResult {
        activeUploadId = id
        var record: UploadRecord?
        var session: LibraryRequestScope?
        do {
            guard libraryStatus == .active else { throw UploadFailure.invalidTarget }
            if backgroundEnabled {
                guard background?.capability.isAvailable == true else {
                    throw UploadFailure.backgroundTransferUnavailable
                }
            }
            let current = try await repository.provider.begin()
            session = current
            let scope = try await current.mutationScope(
                libraryId: library, bridge: repository.bridge)
            try await database.bindSession(scope: scope, credentialId: current.credentialIdentifier)
            guard
                let saved = try await database.upload(
                    scope: scope, credentialId: current.credentialIdentifier,
                    id: id, bridge: repository.bridge), saved.state.canResume,
                !saved.stagingReleased
            else { throw UploadFailure.invalidMetadata }
            if let background { try await background.prepareForeground(saved, session: current) }
            guard let refreshed = try await locateUpload(id: id, library: library, session: current)
            else { throw UploadFailure.storageFailure }
            record = refreshed
            _ = try await repository.preflight(
                refreshed.intent.target, session: current, verifyRevision: false)
            try await staging.verify(refreshed)
            try await fence(current, captured)
            progress(UploadProgress(refreshed))
            return await continueUpload(
                refreshed, session: current, generation: captured, progress: progress,
                createIfNeeded: refreshed.serverSessionId == nil,
                backgroundEnabled: backgroundEnabled)
        } catch {
            return await stopped(
                record, failure: UploadPolicy.classify(error), session: session,
                generation: captured, progress: progress)
        }
    }

    private func continueUpload(
        _ initial: UploadRecord, session: LibraryRequestScope, generation captured: UInt64,
        progress: @escaping @MainActor (UploadProgress) -> Void,
        createIfNeeded: Bool, backgroundEnabled: Bool = false
    ) async -> FileUploadResult {
        var record = initial
        do {
            try await fence(session, captured)
            try await staging.verify(record)
            if createIfNeeded {
                let dispatched = try await update(
                    record, state: .sessionCreating, uncertain: true,
                    failure: nil, beginAttempt: true)
                record = dispatched
                progress(UploadProgress(record))
                do {
                    let evidence = try await repository.create(
                        record.intent, session: session, expectedOffset: 0)
                    record = try await accept(
                        evidence, for: record, session: session, generation: captured,
                        progress: progress)
                } catch {
                    throw UploadPolicy.classify(error)
                }
            } else {
                guard record.serverSessionId != nil else { throw UploadFailure.invalidMetadata }
                let evidence = try await repository.status(record, session: session)
                record = try await accept(
                    evidence, for: record, session: session, generation: captured,
                    progress: progress)
            }
            if record.state == .committed { return .completed(record) }
            guard record.serverState == .open, record.serverSessionId != nil else {
                return .stopped(record, record.failure ?? .completionUnknown)
            }
            if backgroundEnabled {
                guard let background else { throw UploadFailure.invalidMetadata }
                if record.authoritativeOffset < record.intent.expected.byteLength {
                    try await background.schedule(
                        record, session: session, explicitAuthorization: true)
                }
                return .scheduled(record)
            }
            var reconciledPatchFailure = false
            while record.authoritativeOffset < record.intent.expected.byteLength {
                try await fence(session, captured)
                let count = Int(
                    min(
                        UInt64(UploadPolicy.chunkBytes),
                        record.intent.expected.byteLength - record.authoritativeOffset))
                guard count > 0 else { throw UploadFailure.invalidMetadata }
                let bytes = try await staging.chunk(
                    record, offset: record.authoritativeOffset, count: count)
                let prior = record.authoritativeOffset
                record = try await update(
                    record, state: .uploading, uncertain: true,
                    failure: nil, beginAttempt: true)
                do {
                    let nextOffset = try await repository.patch(
                        record, offset: prior, bytes: bytes, session: session)
                    let nextState: UploadLocalState =
                        nextOffset == record.intent.expected.byteLength
                        ? .readyToComplete : .uploading
                    record = try await update(
                        record, state: nextState, uncertain: false,
                        authoritativeOffset: nextOffset, serverState: .open, failure: nil)
                    progress(UploadProgress(record))
                } catch {
                    let patchFailure = UploadPolicy.classify(error)
                    if Task.isCancelled || [
                        .unauthenticated, .deviceRevoked, .staleSession, .scopeMismatch,
                        .cancelled,
                    ].contains(patchFailure) {
                        throw Task.isCancelled ? UploadFailure.cancelled : patchFailure
                    }
                    guard !reconciledPatchFailure else { throw patchFailure }
                    reconciledPatchFailure = true
                    let status = try await repository.status(record, session: session)
                    record = try await accept(
                        status, for: record, session: session, generation: captured,
                        progress: progress)
                    if record.state == .committed { return .completed(record) }
                    if patchFailure == .chunkTooLarge || patchFailure == .unsupportedMediaType
                        || patchFailure == .capacityUnavailable || patchFailure == .sessionExpired
                    {
                        throw patchFailure
                    }
                    guard record.serverState == .open else {
                        return .stopped(record, patchFailure)
                    }
                    if record.authoritativeOffset == prior { throw patchFailure }
                }
            }
            guard record.authoritativeOffset == record.intent.expected.byteLength else {
                throw UploadFailure.offsetMismatch
            }
            try await fence(session, captured)
            try await staging.verify(record)
            if record.state != .readyToComplete {
                record = try await update(
                    record, state: .readyToComplete, uncertain: false,
                    serverState: .open, failure: nil)
                progress(UploadProgress(record))
            }
            let targetNode = try targetNodeId(record)
            record = try await update(
                record, state: .completing, uncertain: true,
                failure: nil, beginAttempt: true)
            do {
                let completion = try await repository.complete(
                    record, targetNodeId: targetNode, session: session)
                record = try await commit(completion, record: record, progress: progress)
            } catch {
                let completionFailure = UploadPolicy.classify(error)
                if Task.isCancelled || [
                    .unauthenticated, .deviceRevoked, .staleSession, .scopeMismatch,
                    .revisionConflict, .conflict, .sessionExpired, .cancelled,
                ].contains(completionFailure) {
                    throw Task.isCancelled ? UploadFailure.cancelled : completionFailure
                }
                let status = try await repository.status(record, session: session)
                record = try await accept(
                    status, for: record, session: session, generation: captured,
                    progress: progress)
                if record.state == .committed { return .completed(record) }
                throw UploadFailure.completionUnknown
            }
            record = await cleanupCommitted(record)
            progress(UploadProgress(record))
            return .completed(record)
        } catch {
            if record.state == .committed { return .completed(record) }
            return await stopped(
                record, failure: UploadPolicy.classify(error), session: session,
                generation: captured, progress: progress)
        }
    }

    private func accept(
        _ evidence: UploadSessionEvidence, for previous: UploadRecord,
        session: LibraryRequestScope, generation captured: UInt64,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async throws -> UploadRecord {
        try await fence(session, captured)
        guard evidence.id == previous.intent.idempotencyKey.rawValue,
            evidence.operation == previous.intent.target.operation,
            evidence.expected == previous.intent.expected,
            evidence.libraryId == previous.intent.target.library,
            evidence.receivedBytes >= previous.authoritativeOffset,
            evidence.receivedBytes <= previous.intent.expected.byteLength
        else { throw UploadFailure.invalidMetadata }
        switch previous.intent.target {
        case .createFile(_, let parent, let name):
            guard evidence.parentId == parent, evidence.name == name,
                previous.serverTargetNodeId.map({ $0 == evidence.nodeId }) ?? true
            else { throw UploadFailure.invalidTarget }
        case .replaceContent(_, let node, let revision):
            guard evidence.nodeId == node, evidence.expectedRevision == revision else {
                throw UploadFailure.invalidTarget
            }
        }
        switch evidence.state {
        case .open:
            let state: UploadLocalState
            if previous.state == .failedPermanent {
                state = .failedPermanent
            } else if evidence.receivedBytes == previous.intent.expected.byteLength {
                state = .readyToComplete
            } else {
                state = evidence.receivedBytes == 0 ? .sessionOpen : .uploading
            }
            let updated = try await update(
                previous, state: state, uncertain: false,
                serverSessionId: evidence.id, serverTargetNodeId: evidence.nodeId,
                authoritativeOffset: evidence.receivedBytes, serverState: evidence.state,
                failure: state == .failedPermanent ? previous.failure : nil)
            progress(UploadProgress(updated))
            return updated
        case .committed:
            guard let completion = evidence.completion else { throw UploadFailure.invalidMetadata }
            let committed = try await update(
                previous, state: .committed, uncertain: false,
                serverSessionId: evidence.id, serverTargetNodeId: evidence.nodeId,
                authoritativeOffset: evidence.receivedBytes, serverState: evidence.state,
                completion: completion, failure: nil)
            let cleaned = await cleanupCommitted(committed)
            progress(UploadProgress(cleaned))
            return cleaned
        case .verifying, .committing:
            let updated = try await update(
                previous, state: .outcomeUnknown, uncertain: true,
                serverSessionId: evidence.id, serverTargetNodeId: evidence.nodeId,
                authoritativeOffset: evidence.receivedBytes, serverState: evidence.state,
                failure: .completionUnknown)
            progress(UploadProgress(updated))
            return updated
        case .failed:
            return try await update(
                previous, state: .failedPermanent, uncertain: false,
                serverSessionId: evidence.id, serverTargetNodeId: evidence.nodeId,
                authoritativeOffset: evidence.receivedBytes, serverState: evidence.state,
                failure: .serverFailed)
        case .expired:
            return try await update(
                previous, state: .expired, uncertain: false,
                serverSessionId: evidence.id, serverTargetNodeId: evidence.nodeId,
                authoritativeOffset: evidence.receivedBytes, serverState: evidence.state,
                failure: .sessionExpired)
        case .aborted:
            return try await update(
                previous, state: .aborted, uncertain: false,
                serverSessionId: evidence.id, serverTargetNodeId: evidence.nodeId,
                authoritativeOffset: evidence.receivedBytes, serverState: evidence.state,
                failure: .sessionAborted)
        }
    }

    private func commit(
        _ completion: UploadCompletion, record: UploadRecord,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async throws -> UploadRecord {
        guard completion.sessionId == record.serverSessionId,
            completion.expected == record.intent.expected,
            completion.nodeId == (try targetNodeId(record))
        else { throw UploadFailure.integrityMismatch }
        let committed = try await update(
            record, state: .committed, uncertain: false,
            authoritativeOffset: record.intent.expected.byteLength,
            serverState: .committed, completion: completion, failure: nil)
        progress(UploadProgress(committed))
        return await cleanupCommitted(committed)
    }

    private func cleanupCommitted(_ record: UploadRecord) async -> UploadRecord {
        guard !record.stagingReleased else { return record }
        do { try await staging.removeCommittedSource(record) } catch { return record }
        do { try await database.releaseUploadStaging(record) } catch {}
        guard
            let verified = try? await database.upload(
                scope: record.scope, credentialId: record.credentialId,
                id: record.id, bridge: repository.bridge), verified.state == .committed,
            verified.completion?.sessionId == record.completion?.sessionId,
            verified.completion?.nodeId == record.completion?.nodeId,
            verified.completion?.fileVersionId == record.completion?.fileVersionId,
            verified.stagingReleased
        else { return record }
        return verified
    }

    private func executeStatus(
        id: UploadOperationId, library: LibraryId,
        generation captured: UInt64
    ) async -> Result<UploadRecord, UploadFailure> {
        activeUploadId = id
        do {
            let session = try await repository.provider.begin()
            try await fence(session, captured)
            guard
                let existing = try await locateUpload(
                    id: id, library: library, session: session)
            else {
                throw UploadFailure.invalidMetadata
            }
            guard
                existing.state.canResume || existing.state == .abortPending
                    || existing.state == .committed || existing.state == .failedPermanent
            else {
                throw existing.failure ?? UploadFailure.invalidMetadata
            }
            guard existing.serverSessionId != nil else { throw UploadFailure.invalidMetadata }
            if let background {
                try await background.prepareForeground(
                    existing, session: session, permitTerminalInspection: true)
            }
            guard let refreshed = try await locateUpload(id: id, library: library, session: session)
            else { throw UploadFailure.storageFailure }
            let evidence = try await repository.status(refreshed, session: session)
            let record = try await accept(
                evidence, for: refreshed, session: session, generation: captured,
                progress: { _ in })
            return .success(record)
        } catch { return .failure(UploadPolicy.classify(error)) }
    }

    private func executeAbort(
        id: UploadOperationId, library: LibraryId,
        generation captured: UInt64
    ) async -> Result<UploadRecord, UploadFailure> {
        activeUploadId = id
        do {
            let session = try await repository.provider.begin()
            try await fence(session, captured)
            guard var record = try await locateUpload(id: id, library: library, session: session),
                record.serverSessionId != nil,
                record.state.canResume || record.state == .abortPending
                    || record.state == .failedPermanent,
                record.serverState != .committing,
                record.serverState != .verifying
            else { throw UploadFailure.invalidMetadata }
            if let background {
                try await background.prepareForeground(
                    record, session: session, permitTerminalInspection: true)
            }
            guard let refreshed = try await locateUpload(id: id, library: library, session: session)
            else { throw UploadFailure.storageFailure }
            record = refreshed
            record = try await update(
                record, state: .abortPending, uncertain: true, failure: nil,
                beginAttempt: true)
            try await fence(session, captured)
            do {
                let evidence = try await repository.abort(record, session: session)
                return try await reconcileAbort(
                    evidence, record: record, session: session, generation: captured)
            } catch {
                let failure = UploadPolicy.classify(error)
                if Task.isCancelled
                    || [.unauthenticated, .deviceRevoked, .staleSession, .scopeMismatch,
                        .cancelled].contains(failure)
                {
                    throw Task.isCancelled ? UploadFailure.cancelled : failure
                }
                let evidence = try await repository.status(record, session: session)
                return try await reconcileAbort(
                    evidence, record: record, session: session, generation: captured)
            }
        } catch { return .failure(UploadPolicy.classify(error)) }
    }

    private func reconcileAbort(
        _ evidence: UploadSessionEvidence, record: UploadRecord,
        session: LibraryRequestScope, generation captured: UInt64
    ) async throws -> Result<UploadRecord, UploadFailure> {
        try await fence(session, captured)
        if [.aborted, .committed, .expired, .failed].contains(evidence.state) {
            let updated = try await accept(
                evidence, for: record, session: session, generation: captured,
                progress: { _ in })
            return updated.state == .aborted || updated.state == .committed
                ? .success(updated) : .failure(updated.failure ?? .completionUnknown)
        }
        let nextState: UploadLocalState =
            evidence.state == .open ? .abortPending : .outcomeUnknown
        _ = try await update(
            record, state: nextState, uncertain: true,
            serverSessionId: evidence.id, serverTargetNodeId: evidence.nodeId,
            authoritativeOffset: evidence.receivedBytes, serverState: evidence.state,
            failure: .completionUnknown)
        return .failure(.completionUnknown)
    }

    private func locateUpload(
        id: UploadOperationId, library: LibraryId, session: LibraryRequestScope
    ) async throws -> UploadRecord? {
        let scope = try await session.mutationScope(libraryId: library, bridge: repository.bridge)
        try await database.bindSession(scope: scope, credentialId: session.credentialIdentifier)
        return try await database.upload(
            scope: scope, credentialId: session.credentialIdentifier,
            id: id, bridge: repository.bridge)
    }

    private func stopped(
        _ record: UploadRecord?, failure: UploadFailure, session: LibraryRequestScope?,
        generation captured: UInt64, progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> FileUploadResult {
        guard let record, captured == generation, record.state != .committed else {
            return .stopped(captured == generation ? record : nil, failure)
        }
        let state: UploadLocalState
        let ambiguousOutcomeFailure: Bool
        switch failure {
        case .offline, .dnsFailure, .timeout, .tlsFailure, .redirectRejected,
            .serverUnavailable, .invalidMetadata, .completionUnknown, .offsetMismatch,
            .unexpectedStatus, .conflict:
            ambiguousOutcomeFailure = true
        default:
            ambiguousOutcomeFailure = false
        }
        if failure == .cancelled {
            state = record.uncertain ? .outcomeUnknown : .cancelledLocal
        } else if failure == .staleSession || failure == .scopeMismatch
            || failure == .unauthenticated || failure == .deviceRevoked
        {
            state = .sessionInvalidated
        } else if record.uncertain
            && [.sessionCreating, .uploading, .completing, .abortPending].contains(record.state)
            && ambiguousOutcomeFailure
        {
            state = .outcomeUnknown
        } else if failure.localState == .failedRecoverable {
            state = .failedRecoverable
        } else if failure.localState == .storageUnavailable {
            state = .storageUnavailable
        } else {
            state = .failedPermanent
        }
        do {
            let updated = try await update(
                record, state: state, uncertain: record.uncertain,
                failure: failure)
            if let session {
                try? await repository.provider.validate(session)
            }
            if captured == generation { progress(UploadProgress(updated)) }
            return .stopped(captured == generation ? updated : nil, failure)
        } catch {
            return .stopped(captured == generation ? record : nil, failure)
        }
    }

    private func update(
        _ previous: UploadRecord, state: UploadLocalState, uncertain: Bool,
        serverSessionId: String? = nil, serverTargetNodeId: NodeId? = nil,
        authoritativeOffset: UInt64? = nil, serverState: UploadServerState? = nil,
        completion: UploadCompletion? = nil, failure: UploadFailure? = nil,
        beginAttempt: Bool = false
    ) async throws -> UploadRecord {
        do {
            try await database.updateUpload(
                previous, state: state, uncertain: uncertain,
                serverSessionId: serverSessionId, serverTargetNodeId: serverTargetNodeId,
                authoritativeOffset: authoritativeOffset, serverState: serverState,
                completion: completion, failure: failure, beginAttempt: beginAttempt)
        } catch MutationQueueFailure.commitAcknowledgementLost {
            // Exact readback below decides whether the state change committed.
        }
        guard
            let stored = try await database.upload(
                scope: previous.scope, credentialId: previous.credentialId,
                id: previous.id, bridge: repository.bridge)
        else { throw UploadFailure.storageFailure }
        let expectedOffset = authoritativeOffset ?? previous.authoritativeOffset
        let expectedSession = serverSessionId ?? previous.serverSessionId
        let expectedTarget = serverTargetNodeId ?? previous.serverTargetNodeId
        let expectedServerState = serverState ?? previous.serverState
        let expectedCompletion = completion ?? previous.completion
        let expectedFailure = failure
        guard stored.intent == previous.intent, stored.stagingId == previous.stagingId,
            stored.state == state, stored.uncertain == uncertain,
            stored.authoritativeOffset == expectedOffset,
            stored.serverSessionId == expectedSession,
            stored.serverTargetNodeId == expectedTarget,
            stored.serverState == expectedServerState,
            stored.completion?.sessionId == expectedCompletion?.sessionId,
            stored.completion?.nodeId == expectedCompletion?.nodeId,
            stored.completion?.fileVersionId == expectedCompletion?.fileVersionId,
            stored.failure == expectedFailure,
            stored.attemptCount == previous.attemptCount + (beginAttempt ? 1 : 0)
        else { throw UploadFailure.storageFailure }
        notificationObservation?(stored)
        return stored
    }

    private func targetNodeId(_ record: UploadRecord) throws -> NodeId {
        guard let id = record.serverTargetNodeId else { throw UploadFailure.invalidMetadata }
        return id
    }

    private func fence(_ session: LibraryRequestScope, _ captured: UInt64) async throws {
        guard generation == captured else { throw UploadFailure.staleSession }
        try await repository.provider.validate(session)
        guard generation == captured else { throw UploadFailure.staleSession }
        try Task.checkCancellation()
    }
}

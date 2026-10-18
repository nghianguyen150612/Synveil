import Foundation
import Observation

/// Owns only background PATCH orchestration. P045 owns CREATE, COMPLETE, abort, wire decoding,
/// immutable target identity and publication. Every ambiguous operation requires one status GET.
@Observable
@MainActor
final class BackgroundUploadCoordinator {
    private(set) var stateRevision: UInt64 = 0
    private let repository: AuthenticatedUploadRepository
    private let database: MutationQueueSQLiteStore
    private let staging: UploadStagingStore
    private let transport: any BackgroundUploadTransportProtocol
    let capability: BackgroundUploadCapability
    private var invalidated = false
    private var authorizationRevision: UInt64 = 0
    private var pausedUploads: Set<UploadOperationId> = []
    private var pendingInvalidation: Task<Void, Never>?
    private var recovering = false
    private var readyForRecovery = false
    private var recoveryRequested = false
    private var recoveryAction: Task<Void, Never>?

    init(
        repository: AuthenticatedUploadRepository, database: MutationQueueSQLiteStore,
        staging: UploadStagingStore, transport: any BackgroundUploadTransportProtocol,
        capability: BackgroundUploadCapability = .current
    ) {
        self.repository = repository
        self.database = database
        self.staging = staging
        self.transport = transport
        self.capability = capability
    }

    /// Called with P045's shared transfer lease held, only after an explicit user choice.
    func schedule(
        _ record: UploadRecord, session: LibraryRequestScope,
        explicitAuthorization: Bool
    ) async throws {
        guard capability.isAvailable else { throw UploadFailure.backgroundTransferUnavailable }
        if explicitAuthorization {
            await pendingInvalidation?.value
            invalidated = false
            pausedUploads.remove(record.id)
        }
        let capturedRevision = authorizationRevision
        guard !invalidated, record.state.canResume, !record.uncertain,
            record.serverState == .open, !record.stagingReleased,
            record.serverSessionId == record.intent.idempotencyKey.rawValue,
            record.authoritativeOffset < record.intent.expected.byteLength,
            session.matches(record.scope), session.credentialIdentifier == record.credentialId
        else { throw UploadFailure.invalidMetadata }
        let previous = try await database.backgroundJob(record.id)
        guard previous?.blocksForeground != true else { throw UploadFailure.duplicateOperation }
        var job =
            previous
            ?? BackgroundUploadJob(
                operationId: record.id.rawValue, generation: 0,
                authorized: true, state: .preparing, chunk: nil, resolved: true,
                taskIdentifier: nil, evidence: nil)
        if let previous {
            guard explicitAuthorization || previous.authorized else {
                throw UploadFailure.staleSession
            }
            try job.transition(.preparing, explicitResume: explicitAuthorization)
            job.authorized = true
            if let old = job.chunk { try await staging.removeBackgroundChunk(old) }
            job.chunk = nil
            job.taskIdentifier = nil
            job.evidence = nil
        } else {
            guard explicitAuthorization else { throw UploadFailure.staleSession }
        }
        try await repository.provider.validate(session)
        readyForRecovery = true
        try await database.saveBackgroundJob(
            job, replacing: previous, record: record,
            explicitResume: explicitAuthorization)
        stateRevision &+= 1
        let prepared = job
        do {
            let count = Int(
                min(
                    UInt64(UploadPolicy.chunkBytes),
                    record.intent.expected.byteLength - record.authoritativeOffset))
            let chunk = try await staging.prepareBackgroundChunk(
                record, id: UploadOperationId(),
                offset: record.authoritativeOffset, count: count,
                durable: database.downloadReservations())
            try await repository.provider.validate(session)
            guard !invalidated, authorizationRevision == capturedRevision else {
                throw UploadFailure.staleSession
            }
            job.generation += 1
            job.chunk = chunk
            job.resolved = false
            try job.transition(.queued)
            try await database.saveBackgroundJob(job, replacing: prepared, record: record)
            stateRevision &+= 1
            // A lost intent COMMIT acknowledgement cannot reach task creation. Recovery enumerates
            // before doing anything with a queued intent. Never recreate it based on missing mapping.
            let request = try await repository.provider.backgroundUploadRequest(
                record: record, chunk: chunk, session: session)
            let file = try await staging.backgroundChunkURL(chunk)
            try await repository.provider.validate(session)
            guard !invalidated, authorizationRevision == capturedRevision else {
                throw UploadFailure.staleSession
            }
            let task = try transport.submit(request, file: file, intent: chunk)
            let queued = job
            job.taskIdentifier = task
            try job.transition(.scheduled)
            try await database.saveBackgroundJob(job, replacing: queued, record: record)
            stateRevision &+= 1
            try await repository.provider.validate(session)
            guard !invalidated, authorizationRevision == capturedRevision else {
                throw UploadFailure.staleSession
            }
            await transport.resume(task, intentId: chunk.id) {
                !self.invalidated && self.authorizationRevision == capturedRevision
                    && !self.pausedUploads.contains(record.id)
            }
        } catch {
            // Preserve prepared chunks and durable intents; do not remove or replay uncertain work.
            throw UploadPolicy.classify(error)
        }
    }

    func waitForRecoveryAction() async { await recoveryAction?.value }
    func waitForInvalidation() async { await pendingInvalidation?.value }

    func job(_ id: UploadOperationId) async throws -> BackgroundUploadJob? {
        try await database.backgroundJob(id)
    }

    /// Pause persists before cancellation; late evidence is retained but cannot authorize Resume.
    func pause(_ id: UploadOperationId) async throws {
        pausedUploads.insert(id)
        guard var job = try await database.backgroundJob(id) else { return }
        let previous = job
        try job.transition(.paused)
        job.authorized = false
        do {
            try await database.saveBackgroundJob(job, replacing: previous)
            stateRevision &+= 1
        } catch {
            if let task = job.taskIdentifier {
                await transport.cancel(task, intentId: job.chunk?.id)
            }
            throw error
        }
        if let task = job.taskIdentifier { await transport.cancel(task, intentId: job.chunk?.id) }
    }

    func invalidateSession() {
        invalidated = true
        authorizationRevision &+= 1
        pendingInvalidation = Task { @MainActor in
            for previous in (try? await database.backgroundInventory()) ?? [] {
                var job = previous
                job.state = .quarantined
                job.authorized = false
                try? await database.saveBackgroundJob(job, replacing: previous)
            }
            for task in await transport.inventory() {
                await transport.cancel(task.identifier, intentId: task.intentId)
            }
        }
    }

    /// Cold launch performs inventory only. When background transfers are unavailable it attempts
    /// to withdraw persisted continuation authority, then cancels tasks without a network status
    /// read. Otherwise follow-up is limited to persisted jobs; it never creates a session, completes
    /// a file, or resumes a paused job.
    func restore() async {
        guard !recovering else { return }
        recovering = true
        readyForRecovery = false
        recoveryRequested = false
        defer {
            recovering = false
            readyForRecovery = true
            if recoveryRequested {
                // Coalesce only fresh persisted delegate events received during this pass.
                // No timer, zero-progress replay or failure-driven resubmission is scheduled.
                recoveryRequested = false
                recoveryAction = Task { @MainActor in await restore() }
            }
        }
        let tasks = await transport.inventory()
        guard capability.isAvailable else {
            await withdrawBackgroundAuthorizationAndCancel(tasks: tasks)
            return
        }
        guard let jobs = try? await database.backgroundInventory() else {
            for task in tasks { await transport.cancel(task.identifier, intentId: task.intentId) }
            return
        }
        for task in tasks {
            guard jobs.contains(where: { $0.chunk?.id == task.intentId && !$0.resolved }) else {
                await transport.cancel(task.identifier, intentId: task.intentId)
                continue
            }
        }
        for resolved in jobs where resolved.resolved {
            if let chunk = resolved.chunk, !tasks.contains(where: { $0.intentId == chunk.id }) {
                try? await staging.removeBackgroundChunk(chunk)
            }
        }
        for original in jobs where original.blocksForeground {
            let capturedRevision = authorizationRevision
            let owner = UUID()
            do {
                try await database.claimUploadRun(owner: owner)
                let session = try await repository.provider.begin()
                let record = try await database.backgroundRecord(
                    original, session: session, bridge: repository.bridge)
                guard !invalidated, original.state != .quarantined,
                    record.state.canResume
                else { throw UploadFailure.staleSession }
                let matching = tasks.filter { $0.intentId == original.chunk?.id }
                guard matching.count <= 1 else { throw UploadFailure.invalidMetadata }
                if let task = matching.first {
                    guard valid(task, record: record, job: original),
                        original.taskIdentifier == nil || original.taskIdentifier == task.identifier
                    else {
                        throw UploadFailure.invalidMetadata
                    }
                    if original.state == .paused || !original.authorized {
                        await transport.cancel(task.identifier, intentId: task.intentId)
                        await database.finishUploadRun(owner: owner)
                        continue
                    }
                    if original.evidence != nil {
                        // A completed delegate event can race the OS inventory snapshot. Do not
                        // turn that ordinary race into quarantine or resume the completed task.
                        await transport.cancel(task.identifier, intentId: task.intentId)
                        try await reconcile(
                            original, record: record, session: session, continueAuthorized: true)
                        await database.finishUploadRun(owner: owner)
                        continue
                    }
                    if let chunk = original.chunk {
                        _ = try await staging.backgroundChunkURL(chunk)
                    }
                    var bound = original
                    bound.taskIdentifier = task.identifier
                    let state: BackgroundUploadState =
                        [.transferring, .suspended].contains(original.state)
                        ? .suspended : .scheduled
                    try bound.transition(state)
                    try await database.saveBackgroundJob(bound, replacing: original)
                    try await repository.provider.validate(session)
                    guard !invalidated else { throw UploadFailure.staleSession }
                    await transport.resume(task.identifier, intentId: original.chunk?.id ?? "") {
                        !self.invalidated && self.authorizationRevision == capturedRevision
                            && !self.pausedUploads.contains(record.id)
                    }
                } else if original.authorized, original.state != .paused {
                    try await reconcile(
                        original, record: record, session: session,
                        continueAuthorized: original.evidence != nil)
                }
            } catch {
                let failure = UploadPolicy.classify(error)
                if [
                    .staleSession, .scopeMismatch, .unauthenticated, .deviceRevoked,
                    .invalidMetadata,
                ].contains(failure) {
                    try? await quarantine(original)
                    for task in tasks where task.intentId == original.chunk?.id {
                        await transport.cancel(task.identifier, intentId: task.intentId)
                    }
                }
            }
            await database.finishUploadRun(owner: owner)
        }
    }

    func recordActivity(_ task: BackgroundSystemTask) async -> Bool {
        do {
            let inventory = try await database.backgroundInventory()
            guard let previous = inventory.first(where: { $0.chunk?.id == task.intentId }) else {
                try await database.recordBackgroundOrphan(task.identifier)
                return true
            }
            guard previous.taskIdentifier == task.identifier else {
                try await quarantine(previous)
                try await database.recordBackgroundOrphan(task.identifier)
                return true
            }
            guard [.scheduled, .suspended].contains(previous.state) else { return true }
            var active = previous
            try active.transition(.transferring)
            try await database.saveBackgroundJob(active, replacing: previous)
            stateRevision &+= 1
            return true
        } catch { return false }
    }

    func receive(_ task: BackgroundSystemTask, evidence: BackgroundUploadEvidence) async -> Bool {
        do {
            let jobs = try await database.backgroundInventory()
            guard let original = jobs.first(where: { $0.chunk?.id == task.intentId }) else {
                try await database.recordBackgroundOrphan(task.identifier)
                return true
            }
            if original.evidence == evidence { return true }
            if original.resolved {
                try await database.recordBackgroundOrphan(task.identifier)
                return true  // old chunk callbacks cannot change a newer intent
            }
            guard original.taskIdentifier == nil || original.taskIdentifier == task.identifier,
                original.evidence == nil
            else {
                try await quarantine(original)
                try await database.recordBackgroundOrphan(task.identifier)
                return true
            }
            var recorded = original
            recorded.taskIdentifier = task.identifier
            recorded.evidence = evidence
            if evidence.failure == UploadFailure.cancelled.rawValue { recorded.authorized = false }
            if ![.paused, .quarantined, .failedTerminal].contains(recorded.state) {
                try recorded.transition(.awaitingEvidence)
            }
            try await database.saveBackgroundJob(recorded, replacing: original)
            stateRevision &+= 1
            guard readyForRecovery else {
                recoveryRequested = true
                return true
            }
            guard capability.isAvailable else { return true }
            // Evidence is durable even if credentials/lease/status are unavailable. Lifecycle
            // acknowledgement need not wait for a network operation or subsequent chunk.
            recoveryAction = Task { @MainActor in
                guard recorded.authorized, !invalidated, recorded.state != .paused else { return }
                let owner = UUID()
                do {
                    try await database.claimUploadRun(owner: owner)
                    let session = try await repository.provider.begin()
                    let record = try await database.backgroundRecord(
                        recorded, session: session, bridge: repository.bridge)
                    guard valid(task, record: record, job: recorded) else {
                        throw UploadFailure.invalidMetadata
                    }
                    try await reconcile(
                        recorded, record: record, session: session, continueAuthorized: true)
                } catch {
                    if [
                        .unauthenticated, .deviceRevoked, .staleSession, .scopeMismatch,
                        .invalidMetadata,
                    ].contains(UploadPolicy.classify(error)) {
                        try? await quarantine(recorded)
                    }
                }
                await database.finishUploadRun(owner: owner)
            }
            return true
        } catch { return false }
    }

    /// Stops legacy jobs conservatively. Durable authorization withdrawal is attempted before
    /// cancellation; if its write fails, the in-process capability stays closed and cancellation is
    /// still attempted. Cancellation does not prove that no PATCH bytes reached the server. Existing
    /// intent, chunk, task identity, response evidence and server offset are retained for explicit
    /// reconciliation.
    private func withdrawBackgroundAuthorizationAndCancel(tasks: [BackgroundSystemTask]) async {
        if let jobs = try? await database.backgroundInventory() {
            for task in tasks {
                let hasExactOwner = jobs.contains {
                    !$0.resolved && $0.taskIdentifier == task.identifier
                        && $0.chunk?.id == task.intentId
                }
                if !hasExactOwner {
                    // Unknown, duplicate, and stale system tasks are never adopted. Retain a
                    // bounded task-ID observation before cancellation when storage is available.
                    try? await database.recordBackgroundOrphan(task.identifier)
                }
            }
            for previous in jobs where previous.blocksForeground && previous.authorized {
                var withdrawn = previous
                withdrawn.authorized = false
                if previous.state.permits(.paused) {
                    try? withdrawn.transition(.paused)
                }
                do {
                    try await database.saveBackgroundJob(withdrawn, replacing: previous)
                    stateRevision &+= 1
                } catch {
                    // Keep the in-process submission gate closed and retry cancellation on relaunch.
                }
            }
        }
        for task in tasks {
            await transport.cancel(task.identifier, intentId: task.intentId)
        }
    }

    /// Explicit Check/Resume cancels first, waits for OS task disappearance, then performs one GET.
    /// If cancellation has not finished, the durable exclusion stays held for a later user action.
    func prepareForeground(
        _ record: UploadRecord, session: LibraryRequestScope,
        permitTerminalInspection: Bool = false
    ) async throws {
        guard let previous = try await database.backgroundJob(record.id) else { return }
        if previous.state == .failedTerminal, previous.resolved, permitTerminalInspection { return }
        guard previous.state != .quarantined, previous.state != .failedTerminal else {
            throw UploadFailure.staleSession
        }
        if previous.blocksForeground {
            let matching = await transport.inventory().filter { $0.intentId == previous.chunk?.id }
            guard matching.count <= 1,
                matching.allSatisfy({ valid($0, record: record, job: previous) })
            else { throw UploadFailure.invalidMetadata }
            for task in matching {
                await transport.cancel(task.identifier, intentId: task.intentId)
            }
            let tasks = await transport.inventory()
            guard !tasks.contains(where: { $0.intentId == previous.chunk?.id }) else {
                throw UploadFailure.completionUnknown
            }
            try await reconcile(
                previous, record: record, session: session, continueAuthorized: false)
        } else if previous.authorized {
            var next = previous
            try next.transition(.foregroundOnly, explicitResume: true)
            next.authorized = false
            try await database.saveBackgroundJob(next, replacing: previous, explicitResume: true)
        }
    }

    private func reconcile(
        _ original: BackgroundUploadJob, record: UploadRecord,
        session: LibraryRequestScope, continueAuthorized: Bool
    ) async throws {
        guard !invalidated, record.state.canResume, let chunk = original.chunk else {
            throw UploadFailure.staleSession
        }
        let tasks = await transport.inventory()
        guard !tasks.contains(where: { $0.intentId == chunk.id }) else {
            throw UploadFailure.completionUnknown
        }
        var job = original
        if job.state != .paused {
            try job.transition(.reconciling)
            try await database.saveBackgroundJob(job, replacing: original)
        }
        let evidence = try await repository.status(record, session: session)
        try await repository.provider.validate(session)
        guard !invalidated, evidence.id == record.serverSessionId,
            evidence.expected == record.intent.expected,
            evidence.operation == record.intent.target.operation,
            evidence.libraryId == record.scope.libraryId,
            evidence.nodeId == record.serverTargetNodeId,
            evidence.parentId == record.intent.target.parent,
            evidence.name == record.intent.target.name,
            evidence.expectedRevision == record.intent.target.expectedRevision,
            evidence.receivedBytes >= chunk.offset,
            evidence.receivedBytes <= chunk.offset + UInt64(chunk.length),
            [.open, .failed, .expired, .aborted].contains(evidence.state)
        else { throw UploadFailure.invalidMetadata }
        // Validate 204 independently when present. A malformed/lost/failed response can only
        // recover via the strict GET just obtained; local counters never contribute evidence.
        let accepted = try? job.evidence?.acceptedOffset(
            chunk: chunk, expected: record.intent.expected.byteLength)
        if let accepted, accepted != evidence.receivedBytes { throw UploadFailure.offsetMismatch }
        let terminalHTTP =
            job.evidence?.status.map { [400, 401, 403, 404, 410, 413, 415, 507].contains($0) }
            ?? false
        let isTerminal = evidence.state != .open || terminalHTTP
        let next: BackgroundUploadState =
            job.state == .paused
            ? .paused
            : (isTerminal
                ? (evidence.state == .aborted ? .aborted : .failedTerminal)
                : (evidence.receivedBytes == record.intent.expected.byteLength
                    ? .awaitingFinalization : .completedChunk))
        try await database.resolveBackgroundJob(
            job, record: record, offset: evidence.receivedBytes,
            serverState: evidence.state, nextState: next, continueAuthorized: continueAuthorized)
        stateRevision &+= 1
        try await staging.removeBackgroundChunk(chunk)
        guard continueAuthorized, job.authorized, next == .completedChunk,
            evidence.receivedBytes > chunk.offset
        else { return }
        // One status attempt per callback, no automatic zero-progress retry.
        let updated = try await database.backgroundRecord(
            job, session: session, bridge: repository.bridge)
        try await schedule(updated, session: session, explicitAuthorization: false)
    }

    private func quarantine(_ previous: BackgroundUploadJob) async throws {
        guard
            let current = try await database.backgroundJob(
                UploadOperationId(validating: previous.operationId)),
            current.generation == previous.generation, current.chunk == previous.chunk
        else {
            throw UploadFailure.staleSession
        }
        var job = current
        job.state = .quarantined
        job.authorized = false
        do {
            try await database.saveBackgroundJob(job, replacing: current)
            stateRevision &+= 1
        } catch {
            // A storage failure cannot leave scheduling enabled in this process.
            invalidated = true
            authorizationRevision &+= 1
            throw error
        }
    }

    private func valid(_ task: BackgroundSystemTask, record: UploadRecord, job: BackgroundUploadJob)
        -> Bool
    {
        guard let chunk = job.chunk, task.intentId == chunk.id,
            let url = task.requestURL, task.method == "PATCH",
            task.offset == String(chunk.offset), task.length == String(chunk.length),
            var expected = URLComponents(
                url: record.scope.serverEndpoint.url, resolvingAgainstBaseURL: false)
        else { return false }
        let base = expected.percentEncodedPath.trimmingCharacters(
            in: CharacterSet(charactersIn: "/"))
        expected.percentEncodedPath =
            (base.isEmpty ? "" : "/\(base)")
            + "/api/v1/upload-sessions/\(record.serverSessionId ?? "")"
        expected.query = nil
        expected.fragment = nil
        return expected.url == url
    }
}

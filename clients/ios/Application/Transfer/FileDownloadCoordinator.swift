import Foundation

@MainActor
protocol AuthenticatedFileDownloadRequestProviderProtocol: AuthenticatedNodeRequestProviderProtocol
{
    func requestVersion(version: FileVersionId, session: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    func requestContent(
        identity: DownloadIdentity, session: LibraryRequestScope, range: DownloadRange?,
        revalidating: Bool, receive: @escaping @Sendable (Data) async throws -> Void
    ) async throws
}

@MainActor
final class AuthenticatedFileDownloadRepository {
    let provider: any AuthenticatedFileDownloadRequestProviderProtocol
    let bridge: any RustBridgeProtocol
    init(
        provider: any AuthenticatedFileDownloadRequestProviderProtocol,
        bridge: any RustBridgeProtocol
    ) {
        self.provider = provider
        self.bridge = bridge
    }

    func preflight(
        library: LibraryId, nodeId: NodeId, session: LibraryRequestScope,
        pinned: FileVersionId? = nil
    ) async throws -> (ClientMutationScope, FileVersionMetadata) {
        let scope = try await session.mutationScope(libraryId: library, bridge: bridge)
        _ = try await NodeId.validated(nodeId.rawValue, using: bridge)
        let response = try await provider.requestNode(nodeId: nodeId, scope: session)
        try AuthenticatedLibraryCatalogRepository.validateHTTP(response)
        let node = try await NodeResponseDecoder(bridge: bridge).decodeSingle(response.body)
        try await provider.validate(session)
        guard node.id == nodeId, node.libraryId == library else {
            throw DownloadFailure.scopeMismatch
        }
        guard node.kind == .file, node.state == .active, node.trashedAt == nil,
            node.restoreDeadline == nil, !node.purgeEligible, let current = node.currentVersionId
        else { throw DownloadFailure.sourceUnavailable }
        // A resume revalidates the original immutable version, even after the current version changes.
        let version = pinned ?? current
        let metadata = try await FileVersionResponseDecoder(bridge: bridge).decode(
            provider.requestVersion(version: version, session: session), version: version,
            node: nodeId)
        try await provider.validate(session)
        guard pinned != nil || metadata.isCurrent else { throw DownloadFailure.invalidMetadata }
        return (scope, metadata)
    }
}

enum FileDownloadResult: Sendable {
    case completed(VerifiedDownloadedFile)
    case stopped(DownloadRecord?, DownloadFailure)
}

/// One foreground writer for the shared staging store. Reopening or reading status never starts
/// a request. Download/Resume/Retry are explicit; there is no retry loop or mutable Node URL.
@MainActor
final class FileDownloadCoordinator {
    private let repository: AuthenticatedFileDownloadRepository
    private let database: MutationQueueSQLiteStore
    private let staging: DownloadStagingStore
    private var active: Task<FileDownloadResult, Never>?
    private var generation: UInt64 = 0
    private var sessionId = UUID().uuidString.lowercased()
    private var latest: DownloadRecord?

    init(
        repository: AuthenticatedFileDownloadRepository, database: MutationQueueSQLiteStore,
        staging: DownloadStagingStore
    ) {
        self.repository = repository
        self.database = database
        self.staging = staging
    }

    func run(
        library: LibraryId, node: NodeId, selectedVersion: FileVersionId? = nil,
        resume: DownloadOperationId? = nil,
        restart: Bool = false,
        progress: @escaping @MainActor (DownloadProgress) -> Void
    ) async -> FileDownloadResult {
        guard active == nil else { return .stopped(nil, .duplicateOperation) }
        latest = nil
        let captured = generation
        let task = Task {
            await self.execute(
                library: library, node: node, selectedVersion: selectedVersion, resume: resume,
                restart: restart,
                generation: captured, progress: progress)
        }
        active = task
        let result = await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
        if generation == captured { active = nil }
        return result
    }
    func cancel() { active?.cancel() }

    func invalidateSession() {
        generation &+= 1
        sessionId = UUID().uuidString.lowercased()
        active?.cancel()
        // Keep the writer lock until the old task stops; authorization is already fenced by the
        // session controller. Quarantine before best-effort physical deletion.
        let task = active
        Task {
            _ = await task?.value
            active = nil
            latest = nil
            let reservations = try? await database.downloadReservations()
            try? await database.quarantineSessions()
            if let reservations { try? await staging.discard(reservations.map(\.operationId)) }
        }
    }

    /// Local-only durable lookup for a reopened details view; no content/metadata GET is dispatched.
    func saved(library: LibraryId, version: FileVersionId) async throws -> DownloadRecord? {
        let session = try await repository.provider.begin()
        let scope = try await session.mutationScope(libraryId: library, bridge: repository.bridge)
        // An unseen scope has no saved download. bindSession also rejects quarantine/replacement.
        try await database.bindSession(scope: scope, credentialId: session.credentialIdentifier)
        let record = try await database.download(
            scope: scope, credentialId: session.credentialIdentifier,
            version: version, bridge: repository.bridge)
        if let record, record.state == .verified {
            do { try await staging.verify(record, published: true) } catch {
                try? await database.updateDownload(
                    record, state: .failedPermanent,
                    failure: DownloadPolicy.classify(error))
                throw error
            }
        }
        try await repository.provider.validate(session)
        return record
    }

    /// Local-only acquisition for preview and export. It consults the durable VERIFIED record,
    /// proves immutable Node/Library/version identity, verifies bytes, and never performs HTTP.
    func verifiedFile(
        library: LibraryId, node: NodeId, version: FileVersionId
    ) async throws -> VerifiedDownloadedFile {
        let session = try await repository.provider.begin()
        let scope = try await session.mutationScope(libraryId: library, bridge: repository.bridge)
        try await database.bindSession(scope: scope, credentialId: session.credentialIdentifier)
        guard let record = try await database.download(
            scope: scope, credentialId: session.credentialIdentifier,
            version: version, bridge: repository.bridge),
            record.state == .verified, record.identity.scope == scope,
            record.identity.credentialId == session.credentialIdentifier,
            record.identity.nodeId == node, record.identity.versionId == version
        else { throw DownloadFailure.stagingMissing }
        do { try await staging.verify(record, published: true) }
        catch {
            let failure = DownloadPolicy.classify(error)
            try? await database.updateDownload(record, state: failure.state, failure: failure)
            throw failure
        }
        try await repository.provider.validate(session)
        return VerifiedDownloadedFile(
            operationId: record.id, identity: record.identity, session: session)
    }

    /// Creates a private, verified temporary presentation copy without exposing the source path.
    func makeExportCopy(
        _ capability: VerifiedDownloadedFile, exportId: String, displayName: String
    ) async throws -> URL {
        let session = capability.session
        try await repository.provider.validate(session)
        guard session.matches(capability.identity.scope),
            session.credentialIdentifier == capability.identity.credentialId,
            let record = try await database.download(
                scope: capability.identity.scope, credentialId: capability.identity.credentialId,
                id: capability.operationId, bridge: repository.bridge),
            record.state == .verified, record.identity == capability.identity
        else { throw DownloadFailure.staleSession }
        try await staging.reserveExport(
            id: exportId, bytes: record.identity.expected.byteLength,
            durable: database.downloadReservations())
        let url: URL
        do {
            try Task.checkCancellation()
            url = try await staging.copyVerifiedForExport(
                record, exportId: exportId, displayName: displayName)
        } catch {
            try? await staging.removeExport(exportId)
            throw error
        }
        do {
            try await repository.provider.validate(session)
            guard session.sameSession(as: capability.session) else { throw DownloadFailure.staleSession }
            return url
        } catch {
            try? await staging.removeExport(exportId)
            throw error
        }
    }

    func validateForExport(_ capability: VerifiedDownloadedFile) async throws {
        try await repository.provider.validate(capability.session)
        guard capability.session.matches(capability.identity.scope),
            capability.session.credentialIdentifier == capability.identity.credentialId,
            let record = try await database.download(
                scope: capability.identity.scope, credentialId: capability.identity.credentialId,
                id: capability.operationId, bridge: repository.bridge),
            record.state == .verified, record.identity == capability.identity
        else { throw DownloadFailure.staleSession }
        try await staging.verify(record, published: true)
        try await repository.provider.validate(capability.session)
    }

    func validateExportCopy(
        _ capability: VerifiedDownloadedFile, exportId: String, displayName: String
    ) async throws -> URL {
        let session = capability.session
        try await repository.provider.validate(session)
        guard session.matches(capability.identity.scope),
            session.credentialIdentifier == capability.identity.credentialId,
            let record = try await database.download(
                scope: capability.identity.scope, credentialId: capability.identity.credentialId,
                id: capability.operationId, bridge: repository.bridge),
            record.state == .verified, record.identity == capability.identity
        else { throw DownloadFailure.staleSession }
        let url = try await staging.verifyExport(
            id: exportId, displayName: displayName, expected: capability.identity.expected)
        try await repository.provider.validate(session)
        return url
    }

    func releaseExport(_ id: String) async throws { try await staging.removeExport(id) }
    func cleanupOrphanExports() async throws { try await staging.cleanupOrphanExports() }

    func read(_ capability: VerifiedDownloadedFile, offset: UInt64, count: Int) async throws -> Data
    {
        let provider = repository.provider
        try await provider.validate(capability.session)
        guard capability.session.matches(capability.identity.scope),
            capability.session.credentialIdentifier == capability.identity.credentialId,
            let record = try await database.download(
                scope: capability.identity.scope,
                credentialId: capability.identity.credentialId, id: capability.operationId,
                bridge: repository.bridge),
            record.state == .verified, record.identity == capability.identity
        else { throw DownloadFailure.staleSession }
        do {
            let data = try await staging.readVerified(record, offset: offset, count: count)
            try await provider.validate(capability.session)
            return data
        } catch {
            let failure = DownloadPolicy.classify(error)
            if [.sha256Mismatch, .integrityFailure, .stagingMissing].contains(failure) {
                try? await database.updateDownload(record, state: failure.state, failure: failure)
            }
            throw failure
        }
    }

    /// Dedicated conditional path requires actual validated local content first. Never used for
    /// fresh/resume reads; a 304 alone cannot create a capability or fill any missing bytes.
    func revalidate(_ capability: VerifiedDownloadedFile) async throws {
        _ = try await read(capability, offset: 0, count: 0)
        try await repository.provider.requestContent(
            identity: capability.identity,
            session: capability.session, range: nil, revalidating: true,
            receive: { _ in
                throw DownloadFailure.unexpectedStatus
            })
        _ = try await read(capability, offset: 0, count: 0)
    }

    private func execute(
        library: LibraryId, node: NodeId, selectedVersion: FileVersionId?,
        resume: DownloadOperationId?, restart: Bool, generation captured: UInt64,
        progress: @escaping @MainActor (DownloadProgress) -> Void
    ) async -> FileDownloadResult {
        let owner = UUID()
        do { try await database.claimDownloadRun(owner: owner) } catch {
            return .stopped(nil, DownloadPolicy.classify(error))
        }
        let result = await performExecute(
            library: library, node: node, selectedVersion: selectedVersion,
            resume: resume, restart: restart, generation: captured, progress: progress)
        await database.finishDownloadRun(owner: owner)
        return result
    }

    private func performExecute(
        library: LibraryId, node: NodeId, selectedVersion: FileVersionId?,
        resume: DownloadOperationId?, restart: Bool,
        generation captured: UInt64,
        progress: @escaping @MainActor (DownloadProgress) -> Void
    ) async -> FileDownloadResult {
        var session: LibraryRequestScope?
        do {
            let current = try await repository.provider.begin()
            session = current
            let scope = try await current.mutationScope(
                libraryId: library, bridge: repository.bridge)
            try await database.bindSession(scope: scope, credentialId: current.credentialIdentifier)
            var record: DownloadRecord
            if let resume {
                guard
                    let saved = try await database.download(
                        scope: scope,
                        credentialId: current.credentialIdentifier, id: resume,
                        bridge: repository.bridge),
                    saved.identity.nodeId == node,
                    saved.state.canResume || saved.state == .verified || restart
                else { throw DownloadFailure.invalidMetadata }
                record = saved
                latest = record
                let (_, metadata) = try await repository.preflight(
                    library: library, nodeId: node,
                    session: current, pinned: record.identity.versionId)
                guard metadata.expected == record.identity.expected else {
                    throw DownloadFailure.invalidMetadata
                }
                if restart {
                    guard record.state != .verified, record.state != .sessionInvalidated else {
                        throw DownloadFailure.invalidMetadata
                    }
                    try await fence(current, captured)
                    try await staging.discard([record.id])
                    do { try await database.restartDownload(record) } catch MutationQueueFailure
                        .commitAcknowledgementLost
                    {}
                    guard
                        let restarted = try await database.download(
                            scope: scope,
                            credentialId: current.credentialIdentifier, id: record.id,
                            bridge: repository.bridge),
                        restarted.identity == record.identity, restarted.state == .prepared,
                        restarted.checkpoint.offset == 0,
                        restarted.checkpoint.prefixSHA256 == DownloadPolicy.emptyDigest
                    else { throw DownloadFailure.storageFailure }
                    record = restarted
                    latest = restarted
                    try await staging.create(record.id)
                }
            } else {
                let (_, metadata) = try await repository.preflight(
                    library: library, nodeId: node,
                    session: current, pinned: selectedVersion)
                if let saved = try await database.download(
                    scope: scope, credentialId: current.credentialIdentifier,
                    version: metadata.id, bridge: repository.bridge)
                {
                    guard saved.identity.nodeId == node,
                        saved.identity.expected == metadata.expected
                    else {
                        throw DownloadFailure.invalidMetadata
                    }
                    guard saved.state == .verified else {
                        return .stopped(saved, .duplicateOperation)
                    }
                    record = saved
                } else {
                    record = DownloadRecord(
                        id: DownloadOperationId(),
                        identity: DownloadIdentity(
                            scope: scope, nodeId: node, versionId: metadata.id,
                            expected: metadata.expected, credentialId: current.credentialIdentifier,
                            sessionId: sessionId),
                        checkpoint: DownloadCheckpoint(
                            offset: 0, prefixSHA256: DownloadPolicy.emptyDigest),
                        state: .prepared, attemptCount: 0, failure: nil)
                    try await staging.reserve(
                        DownloadStorageReservation(
                            operationId: record.id,
                            bytes: metadata.expected.byteLength),
                        durable: database.downloadReservations())
                    try await fence(current, captured)
                    do { try await database.prepareDownload(record) } catch MutationQueueFailure
                        .commitAcknowledgementLost
                    {
                        guard
                            let committed = try await database.download(
                                scope: scope, credentialId: current.credentialIdentifier,
                                id: record.id, bridge: repository.bridge), committed == record
                        else { throw DownloadFailure.storageFailure }
                    }
                    latest = record
                    try await staging.create(record.id)
                }
            }
            latest = record
            try await fence(current, captured)
            if record.state == .verified {
                try await staging.verify(record, published: true)
                try await fence(current, captured)
                progress(DownloadProgress(record))
                return .completed(
                    VerifiedDownloadedFile(
                        operationId: record.id, identity: record.identity, session: current))
            }
            try await staging.reserve(
                DownloadStorageReservation(
                    operationId: record.id,
                    bytes: record.identity.expected.byteLength),
                durable: database.downloadReservations())
            let recovery = try await staging.recover(record)
            try await fence(current, captured)
            if recovery != .verifyPublished {
                record = try await update(record, state: .downloading, beginAttempt: true)
                try await fence(current, captured)
                progress(DownloadProgress(record))
                if record.checkpoint.offset < record.identity.expected.byteLength
                    || record.identity.expected.byteLength == 0
                {
                    let range: DownloadRange?
                    if record.checkpoint.offset > 0 {
                        range = try DownloadRange(
                            start: record.checkpoint.offset,
                            end: record.identity.expected.byteLength - 1,
                            total: record.identity.expected.byteLength)
                    } else {
                        range = nil
                    }
                    try await repository.provider.requestContent(
                        identity: record.identity, session: current,
                        range: range, revalidating: false,
                        receive: { [self] data in
                            try await self.receive(
                                data, session: current, generation: captured, progress: progress)
                        })
                }
                guard let durable = latest else { throw DownloadFailure.storageFailure }
                record = durable
                guard record.checkpoint.offset == record.identity.expected.byteLength else {
                    throw DownloadFailure.contentLengthMismatch
                }
            }
            record = try await update(record, state: .verifying)
            try await fence(current, captured)
            progress(DownloadProgress(record))
            if recovery == .verifyPublished {
                try await staging.verify(record, published: true)
            } else {
                try await staging.publish(record)
            }
            try await fence(current, captured)
            record = try await update(record, state: .verified)
            try await fence(current, captured)
            progress(DownloadProgress(record))
            return .completed(
                VerifiedDownloadedFile(
                    operationId: record.id, identity: record.identity, session: current))
        } catch {
            let failure = DownloadPolicy.classify(error)
            if let session {
                if failure == .unauthenticated {
                    repository.provider.handle(.authenticationRejected, scope: session)
                }
                if failure == .deviceRevoked {
                    repository.provider.handle(.deviceRevoked, scope: session)
                }
            }
            if let record = latest, record.state != .verified, captured == generation {
                // No network retry. Persist only the last acknowledged prefix. If the COMMIT result
                // was lost, update() already read it back before changing observable progress.
                if let updated = try? await update(record, state: failure.state, failure: failure) {
                    if captured == generation { progress(DownloadProgress(updated)) }
                }
            }
            return .stopped(captured == generation ? latest : nil, failure)
        }
    }

    private func receive(
        _ data: Data, session: LibraryRequestScope, generation captured: UInt64,
        progress: @escaping @MainActor (DownloadProgress) -> Void
    ) async throws {
        try await fence(session, captured)
        guard let record = latest else { throw DownloadFailure.storageFailure }
        let checkpoint = try await staging.append(data, record: record)
        try await fence(session, captured)
        let updated = try await update(record, checkpoint: checkpoint, state: .downloading)
        try await fence(session, captured)
        progress(DownloadProgress(updated))
    }
    private func fence(_ session: LibraryRequestScope, _ captured: UInt64) async throws {
        guard generation == captured else { throw DownloadFailure.staleSession }
        try await repository.provider.validate(session)
        guard generation == captured else { throw DownloadFailure.staleSession }
        try Task.checkCancellation()
    }
    private func update(
        _ record: DownloadRecord, checkpoint: DownloadCheckpoint? = nil,
        state: DownloadState, failure: DownloadFailure? = nil,
        beginAttempt: Bool = false
    ) async throws -> DownloadRecord {
        do {
            try await database.updateDownload(
                record, checkpoint: checkpoint, state: state,
                failure: failure, beginAttempt: beginAttempt)
        } catch MutationQueueFailure.commitAcknowledgementLost {
            // Exact readback below confirms durability; no second write or invented progress.
        }
        guard
            let committed = try await database.download(
                scope: record.identity.scope,
                credentialId: record.identity.credentialId, id: record.id, bridge: repository.bridge
            ),
            committed.identity == record.identity,
            committed.checkpoint == (checkpoint ?? record.checkpoint),
            committed.state == state, committed.failure == failure,
            committed.attemptCount == record.attemptCount + (beginAttempt ? 1 : 0)
        else { throw DownloadFailure.storageFailure }
        latest = committed
        return committed
    }
}

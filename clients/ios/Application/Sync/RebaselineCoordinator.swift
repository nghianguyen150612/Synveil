import Foundation

@MainActor
protocol AuthenticatedRebaselineRequestProviderProtocol:
    AuthenticatedSyncCheckpointRequestProviderProtocol
{
    func startRebaseline(
        scope: ClientMutationScope, session: LibraryRequestScope, onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse
    func requestRebaselinePage(
        bootstrap: RebaselineBootstrap, cursor: String?, limit: Int, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse
    func completeRebaseline(
        _ handoff: RebaselineHandoff, session: LibraryRequestScope, onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse
}

/// Storage creates this only after acquiring a durable attempt against a verified prepared generation.
struct RebaselineHandoff: Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    let bootstrap: RebaselineBootstrap
    let credentialId: String, preparedId: String, attemptId: String
    let token: String
    var description: String { "[REDACTED_REBASELINE_HANDOFF]" }
    var debugDescription: String { description }
}

struct StoredRebaseline: Sendable {
    let bootstrap: RebaselineBootstrap
    let state: RebaselineStageState
    let cursor: String?, terminalToken: String?, preparedId: String?
    let library: Library
    let completion: Data?
}

@MainActor
final class RebaselineCoordinator {
    private let database: MutationQueueSQLiteStore
    private let queue: DurableMutationQueue
    private let provider: any AuthenticatedRebaselineRequestProviderProtocol
    private let decoder: RebaselineResponseDecoder
    private let bridge: any RustBridgeProtocol
    private var running = false
    private var activeTask: Task<Void, Error>?
    func invalidateSession() { activeTask?.cancel() }
    init(
        database: MutationQueueSQLiteStore, queue: DurableMutationQueue,
        provider: any AuthenticatedRebaselineRequestProviderProtocol, bridge: any RustBridgeProtocol
    ) {
        self.database = database
        self.queue = queue
        self.provider = provider
        self.bridge = bridge
        decoder = RebaselineResponseDecoder(bridge: bridge)
    }
    func scope(libraryId: LibraryId) async throws -> ClientMutationScope {
        let session = try await provider.begin()
        let scope = try await session.mutationScope(libraryId: libraryId, bridge: bridge)
        _ = try await queue.capture(scope: scope)
        try await provider.validate(session)
        return scope
    }
    /// Local-only status; interrupted completion is presented as unknown without issuing a request.
    func status(scope: ClientMutationScope) async throws -> RebaselineProgress? {
        let session = try await queue.capture(scope: scope)
        let value = try await database.rebaselineProgress(
            scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
        try await provider.validate(session)
        return value
    }
    private func checked(_ response: HTTPTransportResponse, session: LibraryRequestScope) throws
        -> HTTPTransportResponse
    {
        guard response.statusCode != 200 else { return response }
        guard response.body.count <= 16384 else { throw RebaselineFailure.protocolFailure }
        let code = try LibraryResponseDecoder.errorCode(response.body)
        switch (response.statusCode, code) {
        case (410, "bootstrap_expired"): throw RebaselineFailure.expired
        case (409, "bootstrap_conflict"), (409, "sync_rebaseline_required"):
            throw RebaselineFailure.reconciliationRequired
        case (401, "authentication_failed"):
            provider.handle(.authenticationRejected, scope: session)
            throw RebaselineFailure.transport(.authenticationRejected)
        case (401, "device_revoked"), (403, "device_revoked"):
            provider.handle(.deviceRevoked, scope: session)
            throw RebaselineFailure.transport(.deviceRevoked)
        case (503, "dependency_unavailable"): throw RebaselineFailure.transport(.serverUnavailable)
        default: throw RebaselineFailure.protocolFailure
        }
    }
    func start(scope: ClientMutationScope, library: Library, confirmedByUser: Bool) async throws {
        guard confirmedByUser else { throw RebaselineFailure.confirmationRequired }
        guard library.id == scope.libraryId, library.status != .quarantined else {
            throw RebaselineFailure.scopeMismatch
        }
        _ = try await LibraryProjectionMetadata(library).validated(bridge: bridge)
        try await withRun(scope) { session, owner in
            let attempt = try await self.database.beginRebaselineStart(
                scope: scope, credentialId: session.credentialIdentifier, owner: owner)
            // This durable guard conservatively covers the gap between authorization and dispatch.
            let response = try await self.provider.startRebaseline(
                scope: scope, session: session, onDispatch: {})
            let bootstrap = try await self.decoder.start(
                self.checked(response, session: session), scope: scope)
            guard bootstrap.state == .open else {
                throw bootstrap.state == .expired
                    ? RebaselineFailure.expired : .reconciliationRequired
            }
            try await self.provider.validate(session)
            try await self.database.saveRebaselineStart(
                bootstrap, response: response.body, library: library,
                credentialId: session.credentialIdentifier, owner: owner, attempt: attempt)
            try await self.provider.validate(session)
        }
    }
    func download(
        scope: ClientMutationScope, maximumPages: Int = RebaselinePolicy.maximumPagesPerRun,
        progress: @escaping @MainActor (RebaselineProgress) -> Void = { _ in }
    ) async throws {
        guard (1...RebaselinePolicy.maximumPagesPerRun).contains(maximumPages) else {
            throw RebaselineFailure.protocolFailure
        }
        try await withRun(scope) { session, owner in
            for _ in 0..<maximumPages {
                try Task.checkCancellation()
                let saved = try await self.database.rebaseline(
                    scope: scope, credentialId: session.credentialIdentifier, bridge: self.bridge)
                guard let saved, [.bootstrapOpen, .downloading].contains(saved.state) else {
                    throw RebaselineFailure.unverifiedManifest
                }
                do {
                    let response = try await self.provider.requestRebaselinePage(
                        bootstrap: saved.bootstrap, cursor: saved.cursor,
                        limit: RebaselinePolicy.pageSize, session: session)
                    let page = try await self.decoder.page(
                        self.checked(response, session: session), expected: saved.bootstrap)
                    try await self.provider.validate(session)
                    try await self.database.stageRebaseline(
                        page, cursor: saved.cursor, credentialId: session.credentialIdentifier,
                        owner: owner, bridge: self.bridge)
                    try await self.provider.validate(session)
                    if let value = try await self.status(scope: scope) { progress(value) }
                    if page.evidence != nil { return }
                } catch RebaselineFailure.expired {
                    try await self.database.expireRebaseline(
                        scope: scope, credentialId: session.credentialIdentifier, owner: owner)
                    throw RebaselineFailure.expired
                }
            }
        }
    }
    func prepare(scope: ClientMutationScope) async throws {
        try await withRun(scope) { session, owner in
            try await self.database.prepareRebaseline(
                scope: scope, credentialId: session.credentialIdentifier, owner: owner,
                bridge: self.bridge)
            try await self.provider.validate(session)
        }
    }
    func complete(scope: ClientMutationScope, recovering: Bool, confirmedByUser: Bool) async throws
    {
        guard confirmedByUser else { throw RebaselineFailure.confirmationRequired }
        try await withRun(scope) { session, owner in
            let saved = try await self.database.rebaseline(
                scope: scope, credentialId: session.credentialIdentifier, bridge: self.bridge)
            guard let saved else { throw RebaselineFailure.unverifiedManifest }
            if saved.state == .completionConfirmed {
                guard recovering, let bytes = saved.completion else {
                    throw RebaselineFailure.unverifiedManifest
                }
                let result = try await self.decoder.completion(
                    Self.response(bytes), expected: saved.bootstrap)
                try await self.provider.validate(session)
                try await self.database.activateRebaseline(
                    result, scope: scope, credentialId: session.credentialIdentifier, owner: owner,
                    bridge: self.bridge)
                try await self.provider.validate(session)
                return
            }
            let handoff = try await self.database.claimRebaselineCompletion(
                scope: scope, credentialId: session.credentialIdentifier, owner: owner,
                recovering: recovering, bridge: self.bridge)
            do {
                try await self.provider.validate(session)
                let response = try await self.provider.completeRebaseline(
                    handoff, session: session, onDispatch: {})
                let result = try await self.decoder.completion(
                    self.checked(response, session: session), expected: handoff.bootstrap)
                try await self.provider.validate(session)
                try await self.database.confirmRebaseline(result, handoff: handoff, owner: owner)
                try await self.provider.validate(session)
                try await self.database.activateRebaseline(
                    result, scope: scope, credentialId: session.credentialIdentifier, owner: owner,
                    bridge: self.bridge)
                try await self.provider.validate(session)
            } catch {
                // Never erase terminal evidence, or claim cancellation undid remote completion.
                let failure = RebaselineFailure.classify(error)
                try? await self.database.unknownRebaseline(
                    handoff: handoff, owner: owner,
                    reconciliationRequired: failure == .expired
                        || failure == .reconciliationRequired)
                throw error
            }
        }
    }
    private func withRun(
        _ scope: ClientMutationScope,
        operation: @escaping @MainActor (LibraryRequestScope, UUID) async throws -> Void
    ) async throws {
        guard !running else { throw RebaselineFailure.alreadyRunning }
        running = true
        defer {
            running = false
            activeTask = nil
        }
        let work = Task { @MainActor in try await self.run(scope, operation: operation) }
        activeTask = work
        try await withTaskCancellationHandler {
            try await work.value
        } onCancel: {
            work.cancel()
        }
    }
    private func run(
        _ scope: ClientMutationScope,
        operation: @MainActor (LibraryRequestScope, UUID) async throws -> Void
    ) async throws {
        let session = try await queue.capture(scope: scope)
        let owner = UUID()
        try await database.claimRebaselineRun(
            scope: scope, credentialId: session.credentialIdentifier, owner: owner)
        do {
            try await provider.validate(session)
            try await operation(session, owner)
            await database.finishRebaselineRun(owner: owner)
        } catch {
            await database.finishRebaselineRun(owner: owner)
            throw RebaselineFailure.classify(error)
        }
    }
    nonisolated static func response(_ bytes: Data) -> HTTPTransportResponse {
        HTTPTransportResponse(
            statusCode: 200, headers: ["Content-Type": "application/json"], body: bytes)
    }
}

import Foundation

/// Production adapter. All feed parsing, materialization, checkpoint and ACK semantics stay in
/// InboundSyncCoordinator. The same SQLite connection and queue enforce foreground exclusion.
@MainActor
final class BackgroundSyncWork: BackgroundSyncWorkProtocol {
    private let provider: AuthenticatedLibraryRequestProvider
    private let database: MutationQueueSQLiteStore
    private let bridge: any RustBridgeProtocol
    private let restoration: any SessionRestorationServiceProtocol
    private let inbound: any InboundSyncCoordinatorProtocol
    private let sessionProvider: any LocalLibrarySessionProviderProtocol
    init(
        provider: AuthenticatedLibraryRequestProvider, database: MutationQueueSQLiteStore,
        bridge: any RustBridgeProtocol, restoration: any SessionRestorationServiceProtocol,
        inbound: any InboundSyncCoordinatorProtocol,
        sessionProvider: (any LocalLibrarySessionProviderProtocol)? = nil
    ) {
        self.provider = provider
        self.database = database
        self.bridge = bridge
        self.restoration = restoration
        self.inbound = inbound
        self.sessionProvider = sessionProvider ?? provider
    }
    func capture() async throws -> LibraryRequestScope { try await sessionProvider.begin() }
    func validate(_ session: LibraryRequestScope) async throws {
        try await provider.validate(session)
        try await database.requireBackgroundAuthorization(session)
        try await provider.validate(session)
    }
    func libraries(session: LibraryRequestScope, after: String?) async throws -> [LibraryId] {
        try await sessionProvider.validate(session)
        let ids = try await database.backgroundLibraryIdentifiers(session: session, after: after)
        var result: [LibraryId] = []
        for raw in ids { result.append(try await LibraryId.validated(raw, using: bridge)) }
        try await sessionProvider.validate(session)
        return result
    }
    func verify(session: LibraryRequestScope, library: LibraryId) async throws {
        let scope = try await session.mutationScope(libraryId: library, bridge: bridge)
        let owner = UUID()
        try await sessionProvider.validate(session)
        try await database.claimInboundRun(
            scope: scope, credentialId: session.credentialIdentifier, owner: owner)
        let result = await restoration.retryVerification(
            of: session.restorationSession, expectedServerEndpoint: session.serverEndpoint)
        await database.finishInboundRun(owner: owner)
        switch result {
        case .remotelyVerifiedAuthorizedSession(let restored):
            guard restored == session.restorationSession else { throw LibraryFailure.staleSession }
            try await validate(session)
        case .locallyValidStoredSession(_, let failure),
            .unresolvedAuthorizationEvidence(_, let failure):
            switch failure {
            case .authenticationRejected:
                provider.handle(.authenticationRejected, scope: session)
                throw LibraryFailure.authenticationRejected
            case .deviceRevoked:
                provider.handle(.deviceRevoked, scope: session)
                throw LibraryFailure.deviceRevoked
            case .serverUnavailable, .dnsFailure, .networkFailure, .timeout:
                throw LibraryFailure.offline
            case .cancelled: throw CancellationError()
            case .localStorageFailure: throw MutationQueueFailure.unavailable
            default: throw LibraryFailure.protocolFailure
            }
        default: throw LibraryFailure.credentialUnavailable
        }
    }
    func synchronize(session: LibraryRequestScope, library: LibraryId) async -> InboundSyncRunResult
    {
        do {
            try await validate(session)
            let scope = try await session.mutationScope(libraryId: library, bridge: bridge)
            return await inbound.synchronize(scope: scope, configuration: .background) { _ in }
        } catch {
            return InboundSyncRunResult(
                reason: InboundSyncCoordinator.classify(error), progress: .initial, recovery: nil)
        }
    }
    func confirmation(session: LibraryRequestScope, library: LibraryId) async throws
        -> BackgroundSyncConfirmation?
    {
        try await validate(session)
        let scope = try await session.mutationScope(libraryId: library, bridge: bridge)
        let page = try await database.latestConfirmedInbound(
            scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
        try await validate(session)
        return page.map {
            BackgroundSyncConfirmation(
                library: library, recordedAt: $0.updatedAt,
                position: SyncJournalPosition(epoch: $0.page.start.epoch, sequence: $0.page.through)
            )
        }
    }
    func lastSavedConfirmation() async throws -> BackgroundSyncConfirmation? {
        let session = try await capture()
        try await validate(session)
        guard let raw = try await database.lastBackgroundConfirmedLibrary(session: session) else {
            return nil
        }
        let library = try await LibraryId.validated(raw, using: bridge)
        return try await confirmation(session: session, library: library)
    }
}

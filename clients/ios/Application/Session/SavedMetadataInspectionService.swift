import Foundation

/// Read authority only. This type has no HTTP, mutation, ACK, upload, or rebaseline methods.
@MainActor
final class SavedMetadataSessionProvider: LocalLibrarySessionProviderProtocol,
    NodeBrowserScopeProviderProtocol
{
    private let controller: SessionController
    private let credentials: any SecureCredentialSinkProtocol
    private let bridge: any RustBridgeProtocol
    private let authorizationStore: MutationQueueSQLiteStore

    init(
        controller: SessionController, credentials: any SecureCredentialSinkProtocol,
        bridge: any RustBridgeProtocol, authorizationStore: MutationQueueSQLiteStore
    ) {
        self.controller = controller
        self.credentials = credentials
        self.bridge = bridge
        self.authorizationStore = authorizationStore
    }

    func begin() async throws -> LibraryRequestScope {
        try Task.checkCancellation()
        let revision = controller.lifecycleRevision
        let session: DeviceCredentialSession
        if controller.state == .authenticated, let endpoint = controller.serverEndpoint {
            session = try await credentials.load(expectedServerEndpoint: endpoint)
        } else {
            session = try controller.savedMetadataSession()
        }
        let scope = LibraryRequestScope(session: session, revision: revision)
        try await validate(scope)
        return scope
    }

    func validate(_ scope: LibraryRequestScope) async throws {
        try check(scope)
        let stored: DeviceCredentialSession
        do {
            stored = try await credentials.load(expectedServerEndpoint: scope.serverEndpoint)
        } catch is CancellationError { throw CancellationError() } catch {
            throw LibraryFailure.credentialUnavailable
        }
        try check(scope)
        guard
            scope.sameSession(
                as: LibraryRequestScope(session: stored, revision: scope.lifecycleRevision))
        else { throw LibraryFailure.staleSession }
        if controller.state == .restorationVerificationPending {
            guard
                try await authorizationStore.permitsPendingSavedInspection(
                    SessionAuthorizationIdentity(stored))
            else { throw LibraryFailure.unauthenticated }
            try check(scope)
        }
    }

    private func check(_ scope: LibraryRequestScope) throws {
        try Task.checkCancellation()
        guard controller.lifecycleRevision == scope.lifecycleRevision,
            controller.serverEndpoint == scope.serverEndpoint,
            controller.permitsSavedMetadataInspection
        else { throw LibraryFailure.staleSession }
        if controller.state == .restorationVerificationPending {
            guard
                scope.sameSession(
                    as: LibraryRequestScope(
                        session: try controller.savedMetadataSession(),
                        revision: controller.lifecycleRevision))
            else { throw LibraryFailure.staleSession }
        }
    }

    func scope(libraryId: LibraryId) async throws -> ClientMutationScope {
        try await begin().mutationScope(libraryId: libraryId, bridge: bridge)
    }
}

@MainActor
protocol SavedMetadataInspectionProtocol {
    func libraries() async throws -> [Library]
    func browse(library: Library, parent: NodeParentScope) async -> OfflineNodeBrowserResult
}

/// Reuses P039 projection validation and P041 saved-only selection. It cannot bind a new scope,
/// unquarantine records, prove current permissions, or authorize network execution.
@MainActor
final class SavedMetadataInspectionService: SavedMetadataInspectionProtocol {
    private let provider: SavedMetadataSessionProvider
    private let database: MutationQueueSQLiteStore
    private let bridge: any RustBridgeProtocol
    private let browser: OfflineNodeBrowserService

    init(
        controller: SessionController, credentials: any SecureCredentialSinkProtocol,
        database: MutationQueueSQLiteStore, bridge: any RustBridgeProtocol,
        liveRepository: any NodeRepositoryProtocol
    ) {
        let provider = SavedMetadataSessionProvider(
            controller: controller, credentials: credentials,
            bridge: bridge, authorizationStore: database)
        self.provider = provider
        self.database = database
        self.bridge = bridge
        browser = OfflineNodeBrowserService(
            liveRepository: liveRepository,
            projection: SQLiteNodeProjectionRepository(
                database: database, provider: provider, bridge: bridge),
            scopeProvider: provider)
    }

    func libraries() async throws -> [Library] {
        let session = try await provider.begin()
        let libraries = try await database.savedLibraries(session: session, bridge: bridge)
        try await provider.validate(session)
        return libraries
    }

    func browse(library: Library, parent: NodeParentScope) async -> OfflineNodeBrowserResult {
        do {
            let session = try await provider.begin()
            let eligible = try await database.savedLibraries(session: session, bridge: bridge)
            guard eligible.contains(library), library.status != .quarantined else {
                return .savedUnavailable(.unavailable)
            }
            let result = await browser.browse(
                libraryId: library.id, parent: parent, request: .savedOnly)
            try await provider.validate(session)
            return result
        } catch { return .savedUnavailable(.unavailable) }
    }
}

import Foundation

/// Offline reads perform local validation and SQLite queries only. Provider.begin reads Keychain,
/// not the network. No result is retained in memory after a session change.
@MainActor
final class SQLiteNodeProjectionRepository: NodeProjectionRepositoryProtocol {
    private let database: MutationQueueSQLiteStore
    private let provider: any LocalLibrarySessionProviderProtocol
    private let bridge: any RustBridgeProtocol
    init(
        database: MutationQueueSQLiteStore,
        provider: any LocalLibrarySessionProviderProtocol, bridge: any RustBridgeProtocol
    ) {
        self.database = database
        self.provider = provider
        self.bridge = bridge
    }
    private func session(_ scope: ClientMutationScope) async throws -> LibraryRequestScope {
        let session = try await provider.begin()
        guard session.matches(scope) else { throw SyncProjectionFailure.scopeMismatch }
        try await provider.validate(session)
        return session
    }

    /// An authenticated metadata read denial invalidates offline eligibility for that Library.
    /// The original session is rechecked; an old response cannot affect a replacement account.
    func rejectReadAuthorization(libraryId: LibraryId, session: LibraryRequestScope) async throws {
        try await provider.validate(session)
        let scope = try await session.mutationScope(libraryId: libraryId, bridge: bridge)
        try await provider.validate(session)
        try await database.quarantineMetadataScope(
            scope: scope, credentialId: session.credentialIdentifier)
    }
    /// A complete live catalog can update Library observations only for already bound scopes.
    /// Missing/forbidden Libraries lose saved-read eligibility; no scope or checkpoint is created.
    func rememberValidatedCatalog(_ libraries: [Library], session: LibraryRequestScope) async throws
    {
        try await provider.validate(session)
        var observed: Set<String> = []
        for library in libraries {
            _ = try await LibraryProjectionMetadata(library).validated(bridge: bridge)
            if library.status != .quarantined { observed.insert(library.id.rawValue) }
        }
        try await provider.validate(session)
        let eligible = try await database.retainObservedLibraryAccess(observed, session: session)
        for library in libraries where eligible.contains(library.id.rawValue) {
            let scope = try await session.mutationScope(libraryId: library.id, bridge: bridge)
            try await provider.validate(session)
            try await database.rememberLibrary(
                library, scope: scope,
                credentialId: session.credentialIdentifier, bridge: bridge)
        }
        try await provider.validate(session)
    }

    func node(scope: ClientMutationScope, nodeId: NodeId) async -> CachedNodeResult {
        do {
            let session = try await session(scope)
            _ = try await NodeId.validated(nodeId.rawValue, using: bridge)
            let node = try await database.cachedNode(
                scope: scope, nodeId: nodeId, credentialId: session.credentialIdentifier,
                bridge: bridge)
            let state = try await database.cachedProjectionState(
                scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
            try await provider.validate(session)
            return node.map { .found($0, state) } ?? .missing(state)
        } catch { return .unavailable(NodeProjectionPolicy.classify(error)) }
    }
    func children(scope: ClientMutationScope, parentId: NodeId, limit: Int = 100) async
        -> CachedChildrenResult
    {
        do {
            let session = try await session(scope)
            _ = try await NodeId.validated(parentId.rawValue, using: bridge)
            let children = try await database.cachedChildren(
                scope: scope, parentId: parentId, limit: limit,
                credentialId: session.credentialIdentifier, bridge: bridge)
            try await provider.validate(session)
            return .loaded(children)
        } catch { return .unavailable(NodeProjectionPolicy.classify(error)) }
    }
    func projectionState(scope: ClientMutationScope) async -> NodeProjectionStateResult {
        do {
            let session = try await session(scope)
            let state = try await database.cachedProjectionState(
                scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
            try await provider.validate(session)
            return .loaded(state)
        } catch { return .unavailable(NodeProjectionPolicy.classify(error)) }
    }
    /// Trusted catalog observations may be explicitly cached without claiming snapshot completeness.
    func rememberLibrary(_ library: Library, scope: ClientMutationScope) async throws {
        let session = try await session(scope)
        try await database.rememberLibrary(
            library, scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
        try await provider.validate(session)
    }
}

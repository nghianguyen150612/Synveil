import Foundation

/// Offline reads perform local validation and SQLite queries only. Provider.begin reads Keychain,
/// not the network. No result is retained in memory after a session change.
@MainActor
final class SQLiteNodeProjectionRepository: NodeProjectionRepositoryProtocol {
    private let database: MutationQueueSQLiteStore
    private let provider: any AuthenticatedSyncFeedRequestProviderProtocol
    private let bridge: any RustBridgeProtocol
    init(
        database: MutationQueueSQLiteStore,
        provider: any AuthenticatedSyncFeedRequestProviderProtocol, bridge: any RustBridgeProtocol
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

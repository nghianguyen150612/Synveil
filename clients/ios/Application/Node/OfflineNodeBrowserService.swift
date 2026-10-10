import Foundation

enum NodeBrowserContentSource: String, Hashable, Sendable {
    case live
    case cached
    case previouslyLoaded
}

enum OfflineNodeBrowserRequest: Equatable, Sendable {
    /// Try the authenticated server first, then use the exact authenticated local scope on a
    /// narrowly classified transient failure.
    case liveFirst
    /// Read the P039 projection only. This request never calls the Node repository.
    case savedOnly
    /// Explicit server refresh. A failure is returned to the caller without another cache read.
    case serverOnly
}

enum NodeCacheUIFailure: Equatable, Sendable {
    case unavailable
    case synchronizationRecoveryRequired
}

struct NodeCachedDirectoryPresentation: Equatable, Sendable {
    let nodes: [Node]
    let knowledge: CachedDirectoryKnowledge
    let hasMore: Bool
    let projection: NodeProjectionState
    /// Present only when this saved result was selected after a failed live request.
    let liveFailure: NodeFailure?
}

enum OfflineNodeBrowserResult: Equatable, Sendable {
    case live([Node])
    case saved(NodeCachedDirectoryPresentation)
    case failed(NodeFailure)
    case savedUnavailable(NodeCacheUIFailure)
}

enum OfflineNodeDetailsResult: Equatable, Sendable {
    case loaded(Node, NodeProjectionState)
    case unavailable(NodeCacheUIFailure)
}

@MainActor
protocol OfflineNodeBrowserServiceProtocol {
    func browse(
        libraryId: LibraryId,
        parent: NodeParentScope,
        request: OfflineNodeBrowserRequest
    ) async -> OfflineNodeBrowserResult

    func savedNode(
        libraryId: LibraryId,
        nodeId: NodeId,
        expectedParent: NodeParentScope,
        ancestry: [NodeId]
    ) async -> OfflineNodeDetailsResult
}

/// Composes the existing authenticated live reader and P039 projection without owning credentials,
/// SQL, synchronization, acknowledgment, or mutation behavior.
@MainActor
final class OfflineNodeBrowserService: OfflineNodeBrowserServiceProtocol {
    private let liveRepository: any NodeRepositoryProtocol
    private let projection: (any NodeProjectionRepositoryProtocol)?
    private let scopeProvider: (any InboundSyncCoordinatorProtocol)?

    init(
        liveRepository: any NodeRepositoryProtocol,
        projection: (any NodeProjectionRepositoryProtocol)?,
        scopeProvider: (any InboundSyncCoordinatorProtocol)?
    ) {
        self.liveRepository = liveRepository
        self.projection = projection
        self.scopeProvider = scopeProvider
    }

    func browse(
        libraryId: LibraryId,
        parent: NodeParentScope,
        request: OfflineNodeBrowserRequest
    ) async -> OfflineNodeBrowserResult {
        if request != .savedOnly {
            switch await liveRepository.listChildren(libraryId: libraryId, parent: parent) {
            case .loaded(let nodes):
                guard Self.isValid(nodes, libraryId: libraryId, parent: parent) else {
                    return .failed(.protocolFailure)
                }
                return .live(nodes)
            case .failed(let failure):
                guard request == .liveFirst, Self.isEligibleForFallback(failure) else {
                    return .failed(failure)
                }
                return await savedItems(
                    libraryId: libraryId, parent: parent, liveFailure: failure)
            }
        }
        return await savedItems(libraryId: libraryId, parent: parent, liveFailure: nil)
    }

    func savedNode(
        libraryId: LibraryId,
        nodeId: NodeId,
        expectedParent: NodeParentScope,
        ancestry: [NodeId]
    ) async -> OfflineNodeDetailsResult {
        guard let projection, let scopeProvider else { return .unavailable(.unavailable) }
        guard ancestry.last == expectedParent.expectedParentId,
            ancestry.count <= 128, Set(ancestry).count == ancestry.count
        else { return .unavailable(.unavailable) }
        do {
            let scope = try await scopeProvider.scope(libraryId: libraryId)
            guard scope.libraryId == libraryId else { return .unavailable(.unavailable) }
            switch await projection.node(scope: scope, nodeId: nodeId) {
            case .unavailable(let failure):
                return .unavailable(Self.uiFailure(for: failure))
            case .missing(let state):
                return .unavailable(Self.uiFailure(for: state))
            case .found(let record, let state):
                guard record.scope == scope, record.id == nodeId,
                    record.lifecycle == .active,
                    let node = record.completeNode,
                    node.id == nodeId, node.libraryId == libraryId,
                    node.parentId == expectedParent.expectedParentId,
                    node.id != expectedParent.expectedParentId,
                    node.kind == .file, node.state == .active,
                    node.trashedAt == nil, node.restoreDeadline == nil, !node.purgeEligible
                else { return .unavailable(Self.uiFailure(for: state)) }
                if let failure = await validatedCachedAncestry(
                    ancestry, libraryId: libraryId, scope: scope, projection: projection)
                {
                    return .unavailable(failure)
                }
                return .loaded(node, state)
            }
        } catch {
            return .unavailable(.unavailable)
        }
    }

    /// Rechecks known directory ancestors after the cached Node read. P039 treats a missing
    /// ancestor row as unknown; a present inactive or noncanonical ancestor blocks publication.
    private func validatedCachedAncestry(
        _ ancestry: [NodeId], libraryId: LibraryId, scope: ClientMutationScope,
        projection: any NodeProjectionRepositoryProtocol
    ) async -> NodeCacheUIFailure? {
        for index in ancestry.indices.reversed() {
            switch await projection.node(scope: scope, nodeId: ancestry[index]) {
            case .unavailable(let failure): return Self.uiFailure(for: failure)
            case .missing:
                // P039's active-ancestry policy stops safely when it reaches an unknown row.
                return nil
            case .found(let record, let state):
                guard record.scope == scope, record.id == ancestry[index],
                    record.lifecycle == .active,
                    let ancestor = record.completeNode,
                    ancestor.libraryId == libraryId, ancestor.kind == .directory,
                    ancestor.state == .active, ancestor.trashedAt == nil,
                    ancestor.restoreDeadline == nil, !ancestor.purgeEligible
                else { return Self.uiFailure(for: state) }
                let expectedParent = index == 0 ? nil : ancestry[index - 1]
                guard ancestor.parentId == expectedParent else { return .unavailable }
            }
        }
        return nil
    }

    private func savedItems(
        libraryId: LibraryId,
        parent: NodeParentScope,
        liveFailure: NodeFailure?
    ) async -> OfflineNodeBrowserResult {
        guard let projection, let scopeProvider else {
            return liveFailure.map(OfflineNodeBrowserResult.failed)
                ?? .savedUnavailable(.unavailable)
        }

        do {
            // P040's scope capture validates the currently authenticated Keychain identity locally;
            // it does not make an HTTP request or create a new offline session.
            let scope = try await scopeProvider.scope(libraryId: libraryId)
            guard scope.libraryId == libraryId else {
                return liveFailure.map(OfflineNodeBrowserResult.failed)
                    ?? .savedUnavailable(.unavailable)
            }

            switch await projection.children(
                scope: scope,
                parentId: parent.expectedParentId,
                limit: NodeProjectionPolicy.maximumChildren
            ) {
            case .unavailable(let failure):
                return liveFailure.map(OfflineNodeBrowserResult.failed)
                    ?? .savedUnavailable(Self.uiFailure(for: failure))
            case .loaded(let cached):
                guard
                    Self.isValid(
                        cached.nodes, libraryId: libraryId, parent: parent
                    )
                else {
                    return liveFailure.map(OfflineNodeBrowserResult.failed)
                        ?? .savedUnavailable(.unavailable)
                }

                var knowledge = cached.knowledge
                if cached.projection.completeness == .rebaselineRequired {
                    knowledge = .staleKnown
                } else if cached.hasMore {
                    knowledge = .partial
                } else if knowledge == .complete,
                    cached.projection.completeness != .complete
                {
                    // Directory completeness is meaningful only when the projection itself has
                    // proven a complete snapshot. Incremental feed progress cannot prove that.
                    knowledge = .partial
                }
                return .saved(
                    NodeCachedDirectoryPresentation(
                        nodes: Self.presentationOrder(cached.nodes),
                        knowledge: knowledge,
                        hasMore: cached.hasMore,
                        projection: cached.projection,
                        liveFailure: liveFailure))
            }
        } catch {
            // A failed identity capture is never a reason to read another saved scope. Preserve the
            // original network failure for fallback and keep explicit local-read failure generic.
            return liveFailure.map(OfflineNodeBrowserResult.failed)
                ?? .savedUnavailable(.unavailable)
        }
    }

    static func isEligibleForFallback(_ failure: NodeFailure) -> Bool {
        switch failure {
        case .offline, .dnsFailure, .timeout, .serverUnavailable:
            true
        case .httpFailure(let statusCode): (500...599).contains(statusCode)
        case .unauthenticated, .credentialUnavailable, .invalidCredential, .originMismatch,
            .staleSession, .authenticationRejected, .deviceRevoked, .tlsFailure,
            .redirectRejected, .unexpectedContentType, .protocolFailure, .resourceLimit,
            .repeatedCursor, .cancelled:
            false
        }
    }

    private static func isValid(
        _ nodes: [Node], libraryId: LibraryId, parent: NodeParentScope
    ) -> Bool {
        var identifiers: Set<NodeId> = []
        return nodes.allSatisfy { node in
            node.libraryId == libraryId
                && node.parentId == parent.expectedParentId
                && node.id != parent.expectedParentId
                && node.state == .active
                && node.trashedAt == nil
                && node.restoreDeadline == nil
                && !node.purgeEligible
                && identifiers.insert(node.id).inserted
        }
    }

    private static func presentationOrder(_ nodes: [Node]) -> [Node] {
        nodes.sorted {
            if $0.kind != $1.kind { return $0.kind == .directory }
            let comparison = $0.name.compare(
                $1.name,
                options: [.caseInsensitive, .numeric],
                locale: Locale(identifier: "en_US_POSIX")
            )
            if comparison != .orderedSame { return comparison == .orderedAscending }
            return $0.id.rawValue < $1.id.rawValue
        }
    }

    private static func uiFailure(for failure: SyncProjectionFailure) -> NodeCacheUIFailure {
        switch failure {
        case .reconciliationRequired, .revisionRegression, .missingMaterialization,
            .conflictingEvent, .stalePage, .invalidParent:
            .synchronizationRecoveryRequired
        case .malformedMetadata, .scopeMismatch, .staleSession, .storageCapacity, .cancelled,
            .storage, .transport:
            .unavailable
        }
    }

    private static func uiFailure(for state: NodeProjectionState) -> NodeCacheUIFailure {
        state.completeness == .rebaselineRequired
            ? .synchronizationRecoveryRequired : .unavailable
    }
}

import Foundation

/// Returns a complete directory for one transient session/library/parent operation.
@MainActor
public final class AuthenticatedNodeRepository: NodeRepositoryProtocol {
    private let provider: any AuthenticatedNodeRequestProviderProtocol
    private let bridge: any RustBridgeProtocol
    private let decoder: NodeResponseDecoder

    init(provider: any AuthenticatedNodeRequestProviderProtocol, bridge: any RustBridgeProtocol) {
        self.provider = provider
        self.bridge = bridge
        decoder = NodeResponseDecoder(bridge: bridge)
    }

    public func listChildren(libraryId: LibraryId, parent: NodeParentScope) async
        -> NodeRepositoryResult
    {
        var scope: LibraryRequestScope?
        do {
            try Task.checkCancellation()
            let current = try await provider.begin()
            scope = current
            _ = try await LibraryId.validated(libraryId.rawValue, using: bridge)
            _ = try await NodeId.validated(parent.expectedParentId.rawValue, using: bridge)
            try await provider.validate(current)
            var cursor: String?
            var seenCursors: Set<Data> = []
            var seenIds: Set<NodeId> = []
            var nodes: [Node] = []
            for _ in 0..<NodeBrowserPolicy.maximumPages {
                try Task.checkCancellation()
                let response = try await provider.requestChildrenPage(
                    libraryId: libraryId, parent: parent, cursor: cursor, scope: current)
                try AuthenticatedLibraryCatalogRepository.validateHTTP(response)
                let page = try await decoder.decode(response.body)
                // HTTP, Keychain and Rust calls suspend; validate identity again before publication.
                try await provider.validate(current)
                guard page.nodes.count <= NodeBrowserPolicy.maximumNodes - nodes.count else {
                    throw NodeFailure.resourceLimit
                }
                for node in page.nodes {
                    guard node.libraryId == libraryId,
                        node.parentId == parent.expectedParentId,
                        node.id != parent.expectedParentId,
                        node.state == .active,
                        node.trashedAt == nil, node.restoreDeadline == nil,
                        seenIds.insert(node.id).inserted
                    else { throw NodeFailure.protocolFailure }
                }
                nodes.append(contentsOf: page.nodes)
                if !page.hasMore {
                    try Task.checkCancellation()
                    return .loaded(nodes)
                }
                guard nodes.count < NodeBrowserPolicy.maximumNodes else {
                    throw NodeFailure.resourceLimit
                }
                guard let next = page.nextCursor else { throw NodeFailure.protocolFailure }
                guard seenCursors.insert(Data(next.utf8)).inserted else {
                    throw NodeFailure.repeatedCursor
                }
                cursor = next
            }
            throw NodeFailure.resourceLimit
        } catch {
            let failure =
                Task.isCancelled
                ? NodeFailure.cancelled : AuthenticatedLibraryCatalogRepository.classify(error)
            if let scope { provider.handle(failure, scope: scope) }
            return .failed(failure)
        }
    }

    /// Single-resource read used by file details; no snapshot fields constrain valid updates.
    public func getNode(libraryId: LibraryId, nodeId: NodeId, expectedParent: NodeParentScope) async
        -> NodeDetailsRepositoryResult
    {
        let result = await getNodeMetadata(libraryId: libraryId, nodeId: nodeId)
        switch result {
        case .loaded(let node):
            guard node.id == nodeId, node.libraryId == libraryId,
                node.parentId == expectedParent.expectedParentId,
                node.id != expectedParent.expectedParentId
            else { return .inconsistent }
            guard node.kind == .file, node.state == .active,
                node.trashedAt == nil, node.restoreDeadline == nil, !node.purgeEligible
            else { return .unavailable }
            return .loaded(node)
        case .unavailable, .inconsistent, .failed:
            return result
        }
    }

    /// General safe metadata read used to verify directory revisions and restore parents.
    public func getNodeMetadata(libraryId: LibraryId, nodeId: NodeId) async
        -> NodeMetadataRepositoryResult
    {
        var scope: LibraryRequestScope?
        do {
            try Task.checkCancellation()
            let current = try await provider.begin()
            scope = current
            _ = try await LibraryId.validated(libraryId.rawValue, using: bridge)
            _ = try await NodeId.validated(nodeId.rawValue, using: bridge)
            try await provider.validate(current)
            let response = try await provider.requestNode(nodeId: nodeId, scope: current)
            do {
                try AuthenticatedLibraryCatalogRepository.validateHTTP(response)
            } catch NodeFailure.httpFailure(statusCode: 404) {
                try await provider.validate(current)
                try Task.checkCancellation()
                return .unavailable
            }
            let node = try await decoder.decodeSingle(response.body)
            // Rust validation also suspends; never publish a resource from a replaced session.
            try await provider.validate(current)
            try Task.checkCancellation()
            return .loaded(node)
        } catch {
            let failure =
                Task.isCancelled
                ? NodeFailure.cancelled : AuthenticatedLibraryCatalogRepository.classify(error)
            if let scope { provider.handle(failure, scope: scope) }
            return .failed(failure)
        }
    }

}

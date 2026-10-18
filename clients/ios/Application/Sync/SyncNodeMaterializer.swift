import Foundation

@MainActor
struct SyncNodeMaterializer {
    let repository: any NodeRepositoryProtocol
    let bridge: any RustBridgeProtocol

    func prepare(_ page: SyncFeedPage, validateSession: () async throws -> Void) async throws
        -> SyncNodeMaterializationPlan
    {
        guard !page.events.isEmpty, page.events.count <= SyncFeedPolicy.maximumEvents else {
            throw SyncProjectionFailure.malformedMetadata
        }
        var final: [NodeId: SyncJournalEvent] = [:]
        for event in page.events {
            if let previous = final[event.resourceId] {
                guard previous.kind != .nodePurged,
                    !SyncDecimalValidation.less(
                        event.resourceRevision.rawValue, previous.resourceRevision.rawValue),
                    event.resourceRevision != previous.resourceRevision || event.kind == .nodePurged
                else { throw SyncProjectionFailure.revisionRegression }
            }
            try Self.validateEvent(event)
            final[event.resourceId] = event
        }
        var nodes: [NodeId: PreparedProjectionNode] = [:]
        var parents: [NodeId: PreparedProjectionNode] = [:]
        var lookups = 0
        for id in final.keys.sorted(by: { $0.rawValue < $1.rawValue }) {
            try Task.checkCancellation()
            try await validateSession()
            let event = final[id]!
            switch event.kind {
            case .nodePurged, .nodeTrashed:
                // Purge has no live resource. Trash is a logical tombstone, not a Restore target.
                continue
            case .nodeCreated, .nodeRenamed, .nodeMoved, .nodeRestored,
                .fileContentCommitted, .fileVersionRestored:
                lookups += 1
                let node = try await lookup(page.scope.libraryId, id)
                try await validateSession()
                guard node.id == id, node.libraryId == page.scope.libraryId else {
                    throw SyncProjectionFailure.scopeMismatch
                }
                guard node.revision.rawValue == event.resourceRevision.rawValue else {
                    throw SyncDecimalValidation.less(
                        node.revision.rawValue, event.resourceRevision.rawValue)
                        ? SyncProjectionFailure.revisionRegression
                        : SyncProjectionFailure.reconciliationRequired
                }
                guard node.state == .active, node.parentId != node.id,
                    event.parentId == node.parentId,
                    event.nodeKind == nil || event.nodeKind == node.kind,
                    event.nodeState == nil || event.nodeState == node.state,
                    event.currentVersionId == node.currentVersionId,
                    node.parentId != nil
                else { throw SyncProjectionFailure.malformedMetadata }
                if [.fileContentCommitted, .fileVersionRestored].contains(event.kind),
                    node.kind != .file
                {
                    throw SyncProjectionFailure.malformedMetadata
                }
                nodes[id] = try await PreparedProjectionNode.validated(node, bridge: bridge)
            }
        }
        for node in nodes.values.sorted(by: { $0.node.id.rawValue < $1.node.id.rawValue }) {
            guard let id = node.node.parentId else { throw SyncProjectionFailure.invalidParent }
            if let parent = nodes[id] {
                parents[id] = parent
                continue
            }
            if parents[id] != nil { continue }
            if let finalParent = final[id], [.nodePurged, .nodeTrashed].contains(finalParent.kind) {
                throw SyncProjectionFailure.invalidParent
            }
            lookups += 1
            guard lookups <= NodeProjectionPolicy.maximumLookups else {
                throw SyncProjectionFailure.storageCapacity
            }
            try Task.checkCancellation()
            let parent = try await lookup(page.scope.libraryId, id)
            try await validateSession()
            guard parent.id == id, parent.libraryId == page.scope.libraryId,
                parent.kind == .directory, parent.state == .active, parent.parentId != node.node.id
            else { throw SyncProjectionFailure.invalidParent }
            parents[id] = try await PreparedProjectionNode.validated(parent, bridge: bridge)
        }
        guard nodes.count + parents.count <= NodeProjectionPolicy.maximumNodesPerPage,
            nodes.values.reduce(0, { $0 + $1.bytes.count })
                + parents.values.reduce(0, { $0 + $1.bytes.count })
                <= NodeProjectionPolicy.maximumPlanBytes
        else { throw SyncProjectionFailure.storageCapacity }
        try Task.checkCancellation()
        try await validateSession()
        return SyncNodeMaterializationPlan(page: page, nodes: nodes, parents: parents)
    }

    private func lookup(_ library: LibraryId, _ id: NodeId) async throws -> Node {
        switch await repository.getNodeMetadata(libraryId: library, nodeId: id) {
        case .loaded(let node): return node
        case .unavailable: throw SyncProjectionFailure.missingMaterialization
        case .inconsistent: throw SyncProjectionFailure.malformedMetadata
        case .failed(let failure): throw SyncProjectionFailure.transport(failure)
        }
    }

    nonisolated static func validateEvent(_ event: SyncJournalEvent) throws {
        guard event.resourceRevision.rawValue != "0", event.parentId != event.resourceId else {
            throw SyncProjectionFailure.malformedMetadata
        }
        switch event.kind {
        case .nodePurged:
            guard event.parentId == nil, event.nodeState == nil, event.currentVersionId == nil
            else { throw SyncProjectionFailure.malformedMetadata }
        case .nodeTrashed:
            guard event.nodeState == nil || event.nodeState == .trashed else {
                throw SyncProjectionFailure.malformedMetadata
            }
        case .nodeCreated, .nodeRenamed, .nodeMoved, .nodeRestored, .fileContentCommitted,
            .fileVersionRestored:
            guard event.nodeState == nil || event.nodeState == .active else {
                throw SyncProjectionFailure.malformedMetadata
            }
        }
    }
}

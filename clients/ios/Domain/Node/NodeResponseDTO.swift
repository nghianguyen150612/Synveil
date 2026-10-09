import Foundation

/// Wire-only types: absent optional strings are permitted; explicit JSON null is not.
struct NodeCollectionResponseDTO: Decodable {
    let data: [NodeResourceDTO]
    let page: LibraryPageDTO
    let meta: LibraryMetadataDTO
}

struct NodeResponseDTO: Decodable {
    let data: NodeResourceDTO
    let meta: LibraryMetadataDTO
}

struct NodeResourceDTO: Decodable {
    let id: String
    let type: String
    let revision: String
    let attributes: NodeAttributesDTO
}

struct NodeAttributesDTO: Decodable {
    let libraryId: String
    let parentId: String?
    let currentVersionId: String?
    let name: String
    let kind: NodeKind
    let state: NodeState
    let createdAt: String
    let updatedAt: String
    let trashedAt: String?
    let restoreDeadline: String?
    let purgeEligible: Bool

    enum CodingKeys: String, CodingKey {
        case name, kind, state
        case libraryId = "library_id"
        case parentId = "parent_id"
        case currentVersionId = "current_version_id"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case trashedAt = "trashed_at"
        case restoreDeadline = "restore_deadline"
        case purgeEligible = "purge_eligible"
    }
}

struct NodeResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol

    func decode(_ body: Data) async throws -> NodeCollectionPage {
        guard body.count <= NodeBrowserPolicy.maximumResponseBytes else {
            throw NodeFailure.resourceLimit
        }
        do {
            let root = try JSONSerialization.jsonObject(with: body)
            try LibraryWireValidation.keys(root, required: ["data", "page", "meta"])
            guard let object = root as? [String: Any],
                let items = object["data"] as? [[String: Any]]
            else { throw NodeFailure.protocolFailure }
            guard items.count <= NodeBrowserPolicy.pageSize else {
                throw NodeFailure.resourceLimit
            }
            try LibraryWireValidation.keys(
                object["page"], required: ["has_more"], optional: ["next_cursor"])
            if let cursor = (object["page"] as? [String: Any])?["next_cursor"],
                !(cursor is String)
            {
                throw NodeFailure.protocolFailure
            }
            try LibraryWireValidation.keys(object["meta"], required: ["request_id"])
            for item in items { try validateResourceShape(item) }
            let dto = try JSONDecoder().decode(NodeCollectionResponseDTO.self, from: body)
            guard LibraryWireValidation.validRequestId(dto.meta.requestId) else {
                throw NodeFailure.protocolFailure
            }
            if dto.page.hasMore {
                guard let cursor = dto.page.nextCursor,
                    !cursor.isEmpty,
                    cursor.unicodeScalars.count <= NodeBrowserPolicy.maximumCursorLength
                else { throw NodeFailure.protocolFailure }
            } else if dto.page.nextCursor != nil {
                throw NodeFailure.protocolFailure
            }
            var nodes: [Node] = []
            var ids: Set<NodeId> = []
            for item in dto.data {
                try Task.checkCancellation()
                let node = try await mapResource(item)
                guard ids.insert(node.id).inserted else { throw NodeFailure.protocolFailure }
                nodes.append(node)
            }
            try Task.checkCancellation()
            return NodeCollectionPage(
                nodes: nodes, hasMore: dto.page.hasMore, nextCursor: dto.page.nextCursor,
                requestId: dto.meta.requestId)
        } catch is CancellationError {
            throw NodeFailure.cancelled
        } catch let failure as NodeFailure {
            throw failure
        } catch {
            throw NodeFailure.protocolFailure
        }
    }

    func decodeSingle(_ body: Data) async throws -> Node {
        guard body.count <= NodeBrowserPolicy.maximumResponseBytes else {
            throw NodeFailure.resourceLimit
        }
        do {
            try Task.checkCancellation()
            let root = try JSONSerialization.jsonObject(with: body)
            try LibraryWireValidation.keys(root, required: ["data", "meta"])
            guard let object = root as? [String: Any] else { throw NodeFailure.protocolFailure }
            try LibraryWireValidation.keys(object["meta"], required: ["request_id"])
            try validateResourceShape(object["data"])
            let dto = try JSONDecoder().decode(NodeResponseDTO.self, from: body)
            guard LibraryWireValidation.validRequestId(dto.meta.requestId) else {
                throw NodeFailure.protocolFailure
            }
            let node = try await mapResource(dto.data)
            try Task.checkCancellation()
            return node
        } catch is CancellationError {
            throw NodeFailure.cancelled
        } catch let failure as NodeFailure {
            throw failure
        } catch {
            throw NodeFailure.protocolFailure
        }
    }

    private func validateResourceShape(_ value: Any?) throws {
        try LibraryWireValidation.keys(value, required: ["id", "type", "revision", "attributes"])
        guard let item = value as? [String: Any] else { throw NodeFailure.protocolFailure }
        let optional: Set<String> = [
            "parent_id", "current_version_id", "trashed_at", "restore_deadline",
        ]
        try LibraryWireValidation.keys(
            item["attributes"],
            required: [
                "library_id", "name", "kind", "state", "created_at", "updated_at", "purge_eligible",
            ], optional: optional)
        guard let attributes = item["attributes"] as? [String: Any] else {
            throw NodeFailure.protocolFailure
        }
        for key in optional {
            if let value = attributes[key], !(value is String) { throw NodeFailure.protocolFailure }
        }
    }

    /// Collection and single-resource reads use exactly the same Rust and wire validation.
    private func mapResource(_ item: NodeResourceDTO) async throws -> Node {
        try Task.checkCancellation()
        guard item.type == "node" else { throw NodeFailure.protocolFailure }
        let attributes = item.attributes
        let id = try await NodeId.validated(item.id, using: bridge)
        let libraryId = try await LibraryId.validated(attributes.libraryId, using: bridge)
        var parentId: NodeId?
        if let value = attributes.parentId {
            parentId = try await NodeId.validated(value, using: bridge)
        }
        var versionId: FileVersionId?
        if let value = attributes.currentVersionId {
            versionId = try await FileVersionId.validated(value, using: bridge)
        }
        guard try await bridge.validateLogicalName(attributes.name) else {
            throw NodeFailure.protocolFailure
        }
        return Node(
            id: id, libraryId: libraryId, parentId: parentId, currentVersionId: versionId,
            revision: try NodeRevision(validating: item.revision), name: attributes.name,
            kind: attributes.kind, state: attributes.state,
            createdAt: try LibraryWireValidation.timestamp(attributes.createdAt),
            updatedAt: try LibraryWireValidation.timestamp(attributes.updatedAt),
            trashedAt: try attributes.trashedAt.map(LibraryWireValidation.timestamp),
            restoreDeadline: try attributes.restoreDeadline.map(LibraryWireValidation.timestamp),
            purgeEligible: attributes.purgeEligible)
    }

}

import Foundation

struct RebaselineBootstrapDTO: Codable, Sendable {
    let bootstrapId: String, deviceId: String, libraryId: String, state: RebaselineServerState
    let generation: String, snapshotEpoch: String, snapshotResumeSequence: String,
        manifestItemCount: String
    let createdAt: String, expiresAt: String, completedAt: String?
    enum CodingKeys: String, CodingKey {
        case bootstrapId = "bootstrap_id", deviceId = "device_id", libraryId = "library_id", state,
            generation
        case snapshotEpoch = "snapshot_epoch", snapshotResumeSequence = "snapshot_resume_sequence"
        case manifestItemCount = "manifest_item_count", createdAt = "created_at", expiresAt =
            "expires_at", completedAt = "completed_at"
    }
}
struct RebaselineContentDTO: Codable, Sendable {
    let byteLength: String, sha256: String
    enum CodingKeys: String, CodingKey { case byteLength = "byte_length", sha256 }
}
struct RebaselineNodeDTO: Codable, Sendable {
    let nodeId: String, parentNodeId: String?, name: String
    let kind: NodeKind, state: NodeState, revision: String, currentVersionId: String?
    let currentContent: RebaselineContentDTO?
    enum CodingKeys: String, CodingKey {
        case nodeId = "node_id", parentNodeId = "parent_node_id", name, kind, state, revision
        case currentVersionId = "current_version_id", currentContent = "current_content"
    }
    func validated(bridge: any RustBridgeProtocol) async throws -> RebaselineSnapshotNode {
        let id = try await NodeId.validated(nodeId, using: bridge)
        let parent: NodeId?
        if let parentNodeId {
            parent = try await NodeId.validated(parentNodeId, using: bridge)
        } else {
            parent = nil
        }
        let version: FileVersionId?
        if let currentVersionId {
            version = try await FileVersionId.validated(currentVersionId, using: bridge)
        } else {
            version = nil
        }
        guard state != .purging, parent != id, name.utf8.count <= 1024,
            try await bridge.validateLogicalName(name),
            parent != nil || (kind == .directory && state == .active),
            kind != .directory || (version == nil && currentContent == nil),
            (version == nil) == (currentContent == nil)
        else { throw RebaselineFailure.protocolFailure }
        _ = try SyncDecimalValidation.validate(revision, nonzero: true)
        let content: RebaselineSnapshotContent?
        if let dto = currentContent {
            guard dto.sha256.utf8.count == 64,
                dto.sha256.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) })
            else { throw RebaselineFailure.protocolFailure }
            content = RebaselineSnapshotContent(
                byteLength: try SyncDecimalValidation.validate(dto.byteLength), sha256: dto.sha256)
        } else {
            content = nil
        }
        return RebaselineSnapshotNode(
            id: id, parentId: parent, name: name, kind: kind, state: state,
            revision: try NodeRevision(validating: revision), currentVersionId: version,
            content: content)
    }
}

struct RebaselineResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol
    private func envelope(_ response: HTTPTransportResponse) throws -> [String: Any] {
        guard response.statusCode == 200,
            response.body.count <= RebaselinePolicy.maximumResponseBytes,
            response.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                .lowercased().split(separator: ";").first?.trimmingCharacters(in: .whitespaces)
                == "application/json"
        else { throw RebaselineFailure.protocolFailure }
        let root = try JSONSerialization.jsonObject(with: response.body)
        try LibraryWireValidation.keys(root, required: ["data", "meta"])
        let object = root as! [String: Any]
        try LibraryWireValidation.keys(object["meta"], required: ["request_id"])
        guard let meta = object["meta"] as? [String: Any],
            let request = meta["request_id"] as? String,
            LibraryWireValidation.validRequestId(request),
            response.headers.first(where: { $0.key.lowercased() == "x-request-id" }).map({
                $0.value == request
            }) ?? true,
            let data = object["data"] as? [String: Any]
        else { throw RebaselineFailure.protocolFailure }
        return data
    }
    private func decode<T: Decodable>(_ value: Any, as type: T.Type) throws -> T {
        try JSONDecoder().decode(
            type,
            from: JSONSerialization.data(
                withJSONObject: value, options: [.sortedKeys, .fragmentsAllowed]))
    }
    private func bootstrap(_ data: Any, scope: ClientMutationScope) async throws
        -> RebaselineBootstrap
    {
        try LibraryWireValidation.keys(
            data,
            required: [
                "bootstrap_id", "device_id", "library_id", "state", "generation", "snapshot_epoch",
                "snapshot_resume_sequence", "manifest_item_count", "created_at", "expires_at",
            ], optional: ["completed_at"])
        let object = data as! [String: Any]
        if let optional = object["completed_at"], !(optional is String) {
            throw RebaselineFailure.protocolFailure
        }
        let dto = try decode(data, as: RebaselineBootstrapDTO.self)
        let device = try await ClientMutationDeviceId.validated(dto.deviceId, using: bridge)
        let library = try await LibraryId.validated(dto.libraryId, using: bridge)
        guard device == scope.deviceId, library == scope.libraryId else {
            throw RebaselineFailure.scopeMismatch
        }
        let count = try SyncDecimalValidation.validate(dto.manifestItemCount)
        guard let rows = Int(count.rawValue), rows <= RebaselinePolicy.maximumRows else {
            throw RebaselineFailure.capacity
        }
        let created = try LibraryWireValidation.timestamp(dto.createdAt)
        let expires = try LibraryWireValidation.timestamp(dto.expiresAt)
        let completed = try dto.completedAt.map(LibraryWireValidation.timestamp)
        guard created < expires, (dto.state == .completed) == (completed != nil),
            completed.map({ $0 >= created }) ?? true
        else { throw RebaselineFailure.protocolFailure }
        return RebaselineBootstrap(
            id: try await RebaselineBootstrapId.validated(dto.bootstrapId, bridge: bridge),
            scope: scope,
            state: dto.state,
            generation: RebaselineGeneration(
                value: try SyncDecimalValidation.validate(dto.generation, nonzero: true)),
            position: SyncJournalPosition(
                epoch: try SyncDecimalValidation.validate(dto.snapshotEpoch, nonzero: true),
                sequence: try SyncDecimalValidation.validate(dto.snapshotResumeSequence)),
            itemCount: rows, createdAt: created, expiresAt: expires, completedAt: completed)
    }
    func start(_ response: HTTPTransportResponse, scope: ClientMutationScope) async throws
        -> RebaselineBootstrap
    {
        guard response.body.count <= 16384 else { throw RebaselineFailure.capacity }
        do { return try await bootstrap(envelope(response), scope: scope) } catch {
            throw RebaselineFailure.classify(error)
        }
    }
    func page(
        _ response: HTTPTransportResponse, expected: RebaselineBootstrap,
        limit: Int = RebaselinePolicy.pageSize
    ) async throws -> RebaselineManifestPage {
        do {
            guard (1...1000).contains(limit) else { throw RebaselineFailure.protocolFailure }
            let data = try envelope(response)
            try LibraryWireValidation.keys(
                data, required: ["bootstrap", "nodes", "has_more"],
                optional: ["next_cursor", "completion_token"])
            let value = try await bootstrap(data["bootstrap"]!, scope: expected.scope)
            guard expected.sameManifest(as: value) else {
                throw RebaselineFailure.generationMismatch
            }
            guard value.state == .open else {
                throw value.state == .expired ? RebaselineFailure.expired : .reconciliationRequired
            }
            for key in ["next_cursor", "completion_token"] {
                if let value = data[key], !(value is String) {
                    throw RebaselineFailure.protocolFailure
                }
            }
            let cursor = data["next_cursor"] as? String, token = data["completion_token"] as? String
            let more = try decode(data["has_more"]!, as: Bool.self)
            guard let rows = data["nodes"] as? [[String: Any]], rows.count <= limit,
                more
                    ? (!rows.isEmpty && cursor != nil && token == nil)
                    : (cursor == nil && token != nil),
                cursor.map({ RebaselinePolicy.validOpaque($0, maximum: 320) }) ?? true,
                token.map({ RebaselinePolicy.validOpaque($0, maximum: 336) }) ?? true
            else { throw RebaselineFailure.protocolFailure }
            var nodes: [RebaselineSnapshotNode] = []
            var previous: String?
            for row in rows {
                try LibraryWireValidation.keys(
                    row, required: ["node_id", "name", "kind", "state", "revision"],
                    optional: ["parent_node_id", "current_version_id", "current_content"])
                for key in ["parent_node_id", "current_version_id"] {
                    if let value = row[key], !(value is String) {
                        throw RebaselineFailure.protocolFailure
                    }
                }
                if let content = row["current_content"] {
                    try LibraryWireValidation.keys(content, required: ["byte_length", "sha256"])
                }
                let node = try await decode(row, as: RebaselineNodeDTO.self).validated(
                    bridge: bridge)
                guard previous.map({ $0 < node.id.rawValue }) ?? true else {
                    throw RebaselineFailure.protocolFailure
                }
                previous = node.id.rawValue
                nodes.append(node)
            }
            try Task.checkCancellation()
            return RebaselineManifestPage(
                bootstrap: value, nodes: nodes, nextCursor: cursor,
                evidence: token.map { RebaselineCompletionEvidence(bootstrap: value, token: $0) },
                responseBody: response.body,
                canonicalData: try JSONSerialization.data(
                    withJSONObject: data, options: [.sortedKeys]))
        } catch { throw RebaselineFailure.classify(error) }
    }
    func completion(_ response: HTTPTransportResponse, expected: RebaselineBootstrap) async throws
        -> RebaselineCompletionResult
    {
        guard response.body.count <= 16384 else { throw RebaselineFailure.capacity }
        do {
            let data = try envelope(response)
            try LibraryWireValidation.keys(data, required: ["bootstrap", "checkpoint", "replayed"])
            let value = try await bootstrap(data["bootstrap"]!, scope: expected.scope)
            guard value.sameManifest(as: expected), value.state == .completed else {
                throw RebaselineFailure.generationMismatch
            }
            try LibraryWireValidation.keys(
                data["checkpoint"],
                required: ["journal_epoch", "acknowledged_sequence", "updated_at"])
            let checkpoint = data["checkpoint"] as! [String: Any]
            guard let epoch = checkpoint["journal_epoch"] as? String,
                let sequence = checkpoint["acknowledged_sequence"] as? String,
                let updated = checkpoint["updated_at"] as? String
            else { throw RebaselineFailure.protocolFailure }
            let position = SyncJournalPosition(
                epoch: try SyncDecimalValidation.validate(epoch, nonzero: true),
                sequence: try SyncDecimalValidation.validate(sequence))
            let date = try LibraryWireValidation.timestamp(updated)
            guard position == expected.position, date >= value.createdAt else {
                throw RebaselineFailure.reconciliationRequired
            }
            return RebaselineCompletionResult(
                bootstrap: value, position: position, updatedAt: date,
                replayed: try decode(data["replayed"]!, as: Bool.self), responseBody: response.body)
        } catch { throw RebaselineFailure.classify(error) }
    }
    func confirmedBase(_ response: HTTPTransportResponse, scope: ClientMutationScope) async throws
        -> ClientMutationBase
    {
        let data = try envelope(response)
        guard let value = data["bootstrap"] else { throw RebaselineFailure.protocolFailure }
        let expected = try await bootstrap(value, scope: scope)
        let result = try await completion(response, expected: expected)
        return try ClientMutationBase(
            scope: scope, epoch: result.position.epoch, sequence: result.position.sequence)
    }
}

import Foundation

private struct ClientMutationResponseDTO: Decodable {
    let data: ClientMutationAppliedDTO
    let meta: LibraryMetadataDTO
}

private struct ClientMutationAppliedDTO: Decodable {
    let outcome: String
    let mutationId: String
    let kind: ClientMutationKind
    let replayed: Bool
    let node: ClientMutationNodeDTO
    let journalEventId: String
    let journalSequence: String
    enum CodingKeys: String, CodingKey {
        case outcome, kind, replayed, node
        case mutationId = "mutation_id", journalEventId = "journal_event_id", journalSequence =
            "journal_sequence"
    }
}

private struct ClientMutationNodeDTO: Decodable {
    let id: String
    let libraryId: String
    let parentNodeId: String?
    let kind: NodeKind
    let state: ClientMutationNodeState
    let name: String
    let revision: String
    let currentVersionId: String?
    let trashedAt: String?
    let createdAt: String
    let updatedAt: String
    enum CodingKeys: String, CodingKey {
        case id, kind, state, name, revision
        case libraryId = "library_id", parentNodeId = "parent_node_id", currentVersionId =
            "current_version_id"
        case trashedAt = "trashed_at", createdAt = "created_at", updatedAt = "updated_at"
    }
}

private struct ClientMutationErrorDTO: Decodable {
    let error: ClientMutationErrorDataDTO
}

private struct ClientMutationErrorDataDTO: Decodable {
    let code: String
    let message: String
    let requestId: String
    let retryable: Bool
    enum CodingKeys: String, CodingKey {
        case code, message, retryable
        case requestId = "request_id"
    }
}

private struct ClientMutationConflictDTO: Decodable {
    let outcome: String
    let replayed: Bool
    let conflictId: String
    let reason: ClientMutationConflictReason
    let resourceId: String
    let expectedRevision: String?
    let currentRevision: String?
    let currentState: NodeState?
    let currentParentId: String?
    let currentName: String?
    let serverEpoch: String
    let serverSequence: String
    enum CodingKeys: String, CodingKey {
        case outcome, replayed, reason
        case conflictId = "conflict_id", resourceId = "resource_id", expectedRevision =
            "expected_revision"
        case currentRevision = "current_revision", currentState = "current_state", currentParentId =
            "current_parent_id"
        case currentName = "current_name", serverEpoch = "server_epoch", serverSequence =
            "server_sequence"
    }
}

/// Strict shape checking precedes typed decoding. Invalid dispatched responses remain ambiguous.
struct ClientMutationResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol

    func decode(_ response: HTTPTransportResponse, for mutation: PreparedClientMutation)
        async throws
        -> ClientMutationSubmissionResult
    {
        try Task.checkCancellation()
        guard response.body.count <= ClientMutationPolicy.maximumResponseBytes else {
            throw ClientMutationFailure.resourceLimit
        }
        if (300..<400).contains(response.statusCode) { return .failed(.redirectRejected) }
        guard let contentType = header("Content-Type", response: response)?.lowercased(),
            contentType.split(separator: ";").first?.trimmingCharacters(in: .whitespaces)
                == "application/json"
        else { throw ClientMutationFailure.protocolFailure }
        do {
            let object = try JSONSerialization.jsonObject(with: response.body)
            if response.statusCode == 200 {
                return .applied(try await applied(object, response: response, mutation: mutation))
            }
            return try await rejection(object, response: response, mutation: mutation)
        } catch is CancellationError { throw CancellationError() } catch let failure
            as ClientMutationFailure
        {
            if failure == .invalidPreparation { throw ClientMutationFailure.protocolFailure }
            throw failure
        } catch { throw ClientMutationFailure.protocolFailure }
    }

    private func applied(
        _ root: Any, response: HTTPTransportResponse, mutation: PreparedClientMutation
    ) async throws
        -> ClientMutationAppliedResult
    {
        try LibraryWireValidation.keys(root, required: ["data", "meta"])
        let object = root as! [String: Any]
        try LibraryWireValidation.keys(object["meta"], required: ["request_id"])
        try LibraryWireValidation.keys(
            object["data"],
            required: [
                "outcome", "mutation_id", "kind", "replayed", "node", "journal_event_id",
                "journal_sequence",
            ])
        let data = object["data"] as! [String: Any]
        try shape(
            data["node"],
            required: [
                "id", "library_id", "kind", "state", "name", "revision", "created_at", "updated_at",
            ],
            optional: ["parent_node_id", "current_version_id", "trashed_at"])
        let dto = try JSONDecoder().decode(ClientMutationResponseDTO.self, from: response.body)
        try requestId(dto.meta.requestId, response: response)
        let value = dto.data
        guard value.outcome == "APPLIED", value.mutationId == mutation.id.rawValue,
            value.kind == mutation.kind
        else { throw ClientMutationFailure.protocolFailure }
        let id = try await ClientMutationId.validated(value.mutationId, using: bridge)
        let node = try await mapNode(value.node)
        guard node.libraryId == mutation.base.scope.libraryId else {
            throw ClientMutationFailure.protocolFailure
        }
        switch mutation.payload.intent {
        case .createDirectory(let parent, _, _):
            guard node.kind == .directory, node.parentNodeId == parent, node.id != parent else {
                throw ClientMutationFailure.protocolFailure
            }
        case .renameNode(let target, _, _), .moveNode(let target, _, _, _),
            .trashNode(let target, _), .restoreNode(let target, _, _, _):
            guard node.id == target else { throw ClientMutationFailure.protocolFailure }
        }
        let event = try await ClientMutationJournalEventId.validated(
            value.journalEventId, using: bridge)
        return ClientMutationAppliedResult(
            mutationId: id, kind: value.kind, replayed: value.replayed,
            node: node, journalEventId: event,
            journalSequence: try ClientMutationDecimal(validating: value.journalSequence),
            requestId: dto.meta.requestId)
    }

    private func mapNode(_ dto: ClientMutationNodeDTO) async throws -> ClientMutationNodeResult {
        let id = try await NodeId.validated(dto.id, using: bridge)
        let library = try await LibraryId.validated(dto.libraryId, using: bridge)
        var parent: NodeId?
        if let text = dto.parentNodeId { parent = try await NodeId.validated(text, using: bridge) }
        var version: FileVersionId?
        if let text = dto.currentVersionId {
            version = try await FileVersionId.validated(text, using: bridge)
        }
        guard try await bridge.validateLogicalName(dto.name) else {
            throw ClientMutationFailure.protocolFailure
        }
        return ClientMutationNodeResult(
            id: id, libraryId: library, parentNodeId: parent,
            kind: dto.kind, state: dto.state, name: dto.name,
            revision: try NodeRevision(validating: dto.revision),
            currentVersionId: version,
            trashedAt: try dto.trashedAt.map(LibraryWireValidation.timestamp),
            createdAt: try LibraryWireValidation.timestamp(dto.createdAt),
            updatedAt: try LibraryWireValidation.timestamp(dto.updatedAt))
    }

    private func rejection(
        _ root: Any, response: HTTPTransportResponse, mutation: PreparedClientMutation
    ) async throws
        -> ClientMutationSubmissionResult
    {
        try LibraryWireValidation.keys(root, required: ["error"])
        let error = (root as! [String: Any])["error"]
        try LibraryWireValidation.keys(
            error, required: ["code", "message", "request_id", "retryable"], optional: ["details"])
        let object = error as! [String: Any]
        if let details = object["details"], !(details is [String: Any]) {
            throw ClientMutationFailure.protocolFailure
        }
        let dto = try JSONDecoder().decode(ClientMutationErrorDTO.self, from: response.body).error
        try requestId(dto.requestId, response: response)
        guard !dto.message.isEmpty, dto.message.unicodeScalars.count <= 512 else {
            throw ClientMutationFailure.protocolFailure
        }
        // Codes and their documented status are both required; text and retryable never establish outcome.
        switch (response.statusCode, dto.code) {
        case (400, "invalid_mutation"): return .failed(.permanentRejection(.invalidMutation))
        case (401, "authentication_failed"): return .failed(.authenticationRejected)
        case (403, "device_revoked"), (401, "device_revoked"): return .failed(.deviceRevoked)
        case (403, "permission_denied"): return .failed(.permanentRejection(.permissionDenied))
        case (404, "not_found"): return .failed(.permanentRejection(.notFound))
        case (409, "mutation_conflict"):
            var conflict: ClientMutationConflict?
            if let details = object["details"] {
                conflict = try await self.conflict(details, mutation: mutation)
            }
            return .conflict(conflict, requestId: dto.requestId)
        case (409, "mutation_id_conflict"): return .mutationIdConflict(requestId: dto.requestId)
        case (409, "sync_rebaseline_required"): return .rebaselineRequired(requestId: dto.requestId)
        case (413, "payload_too_large"): return .failed(.payloadTooLarge)
        case (503, "dependency_unavailable"):
            return .failed(.transientFailure(.dependencyUnavailable))
        case (500, "invalid_persisted_data"):
            return .failed(.permanentRejection(.invalidPersistedData))
        case (500, "internal_error"): return .failed(.transientFailure(.internalError))
        default: throw ClientMutationFailure.protocolFailure
        }
    }

    private func conflict(_ value: Any, mutation: PreparedClientMutation) async throws
        -> ClientMutationConflict
    {
        try shape(
            value,
            required: [
                "outcome", "replayed", "conflict_id", "reason", "resource_id", "server_epoch",
                "server_sequence",
            ],
            optional: [
                "expected_revision", "current_revision", "current_state", "current_parent_id",
                "current_name",
            ])
        let bytes = try JSONSerialization.data(withJSONObject: value)
        let dto = try JSONDecoder().decode(ClientMutationConflictDTO.self, from: bytes)
        guard dto.outcome == "CONFLICT" else { throw ClientMutationFailure.protocolFailure }
        let id = try await ClientMutationConflictId.validated(dto.conflictId, using: bridge)
        let resource = try await NodeId.validated(dto.resourceId, using: bridge)
        // NAME_OCCUPIED identifies the existing sibling, not the submitted source/parent.
        guard dto.reason == .nameOccupied || mutation.payload.intent.resourceIds.contains(resource)
        else {
            throw ClientMutationFailure.protocolFailure
        }
        var parent: NodeId?
        if let text = dto.currentParentId {
            parent = try await NodeId.validated(text, using: bridge)
        }
        if let name = dto.currentName, !(try await bridge.validateLogicalName(name)) {
            throw ClientMutationFailure.protocolFailure
        }
        return ClientMutationConflict(
            conflictId: id, reason: dto.reason, resourceId: resource,
            expectedRevision: try dto.expectedRevision.map { try NodeRevision(validating: $0) },
            currentRevision: try dto.currentRevision.map { try NodeRevision(validating: $0) },
            currentState: dto.currentState, currentParentId: parent, currentName: dto.currentName,
            serverEpoch: try ClientMutationDecimal(validating: dto.serverEpoch),
            serverSequence: try ClientMutationDecimal(validating: dto.serverSequence),
            replayed: dto.replayed)
    }

    private func header(_ name: String, response: HTTPTransportResponse) -> String? {
        response.headers.first { $0.key.lowercased() == name.lowercased() }?.value
    }

    private func requestId(_ value: String, response: HTTPTransportResponse) throws {
        guard LibraryWireValidation.validRequestId(value) else {
            throw ClientMutationFailure.protocolFailure
        }
        if let header = header("X-Request-Id", response: response) {
            guard LibraryWireValidation.validRequestId(header), header == value else {
                throw ClientMutationFailure.protocolFailure
            }
        }
    }

    private func shape(_ value: Any?, required: Set<String>, optional: Set<String>) throws {
        try LibraryWireValidation.keys(value, required: required, optional: optional)
        let object = value as! [String: Any]
        for key in optional {
            if let field = object[key], !(field is String) {
                throw ClientMutationFailure.protocolFailure
            }
        }
    }
}

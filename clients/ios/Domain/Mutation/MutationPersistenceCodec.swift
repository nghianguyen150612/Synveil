import Foundation

/// Version 1 reuses P034's sole request encoder. Storage discrepancies are rejected, never repaired.
struct MutationPersistenceCodec: Sendable {
    let bridge: any RustBridgeProtocol

    func validateScope(_ scope: ClientMutationScope) async throws {
        guard scope.serverEndpoint.isSecureScheme,
            try await bridge.validateNodeID(scope.ownerUserId)
        else { throw MutationQueueFailure.malformedRecord }
        _ = try await ClientMutationDeviceId.validated(scope.deviceId.rawValue, using: bridge)
        _ = try await LibraryId.validated(scope.libraryId.rawValue, using: bridge)
    }

    func payloadBytes(_ mutation: PreparedClientMutation) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return try encoder.encode(mutation.payload)
    }

    func rehydrate(_ row: StoredMutationRecord) async throws -> MutationQueueRecord {
        do {
            guard row.encodingVersion == MutationQueuePolicy.encodingVersion,
                row.request.count <= ClientMutationPolicy.maximumRequestBytes,
                row.payload.count <= ClientMutationPolicy.maximumRequestBytes,
                row.order > 0, row.createdAt.timeIntervalSince1970.isFinite
            else { throw MutationQueueFailure.malformedRecord }
            try await validateScope(row.scope)
            let id = try await ClientMutationId.validated(row.mutationId, using: bridge)
            let base = try ClientMutationBase(
                scope: row.scope,
                epoch: SyncDecimalValidation.validate(row.epoch, nonzero: true),
                sequence: SyncDecimalValidation.validate(row.sequence))
            guard let kind = ClientMutationKind(rawValue: row.kind) else {
                throw MutationQueueFailure.malformedRecord
            }
            let intent = try await decodePayload(row.payload, kind: kind)
            let mutation = try await PreparedClientMutation(
                id: id, base: base, intent: intent, bridge: bridge)
            guard mutation.requestBody == row.request, try payloadBytes(mutation) == row.payload
            else {
                throw MutationQueueFailure.malformedRecord
            }
            if let attempt = row.attempt {
                guard UUID(uuidString: attempt.id) != nil, UUID(uuidString: attempt.owner) != nil,
                    attempt.startedAt.timeIntervalSince1970.isFinite, row.state != .pending
                else { throw MutationQueueFailure.malformedRecord }
            } else if row.state != .pending {
                throw MutationQueueFailure.malformedRecord
            }
            let evidence = try row.evidence.map {
                try JSONDecoder().decode(MutationOutcomeEvidence.self, from: $0)
            }
            try await validateEvidence(evidence, state: row.state, mutation: mutation)
            return MutationQueueRecord(
                mutation: mutation, enqueueOrder: row.order, createdAt: row.createdAt,
                state: row.state, attempt: row.attempt, evidence: evidence)
        } catch is CancellationError { throw CancellationError() } catch {
            throw MutationQueueFailure.malformedRecord
        }
    }

    private func decodePayload(_ bytes: Data, kind: ClientMutationKind) async throws
        -> ClientMutationIntent
    {
        guard let payload = try JSONSerialization.jsonObject(with: bytes) as? [String: String]
        else {
            throw MutationQueueFailure.malformedRecord
        }
        func exact(_ keys: Set<String>) throws {
            guard Set(payload.keys) == keys else { throw MutationQueueFailure.malformedRecord }
        }
        func node(_ key: String) async throws -> NodeId {
            guard let text = payload[key] else { throw MutationQueueFailure.malformedRecord }
            return try await NodeId.validated(text, using: bridge)
        }
        func revision(_ key: String) throws -> NodeRevision {
            guard let text = payload[key] else { throw MutationQueueFailure.malformedRecord }
            return try NodeRevision(validating: text)
        }
        switch kind {
        case .createDirectory:
            try exact(["parent_node_id", "expected_parent_revision", "name"])
            return try await .createDirectory(
                parentNodeId: node("parent_node_id"),
                expectedParentRevision: revision("expected_parent_revision"), name: payload["name"]!
            )
        case .renameNode:
            try exact(["node_id", "expected_revision", "new_name"])
            return try await .renameNode(
                nodeId: node("node_id"), expectedRevision: revision("expected_revision"),
                newName: payload["new_name"]!)
        case .moveNode:
            try exact([
                "node_id", "expected_revision", "new_parent_node_id",
                "expected_new_parent_revision",
            ])
            return try await .moveNode(
                nodeId: node("node_id"), expectedRevision: revision("expected_revision"),
                newParentNodeId: node("new_parent_node_id"),
                expectedNewParentRevision: revision("expected_new_parent_revision"))
        case .trashNode:
            try exact(["node_id", "expected_revision"])
            return try await .trashNode(
                nodeId: node("node_id"), expectedRevision: revision("expected_revision"))
        case .restoreNode:
            try exact([
                "node_id", "expected_revision", "expected_parent_node_id",
                "expected_parent_revision",
            ])
            return try await .restoreNode(
                nodeId: node("node_id"), expectedRevision: revision("expected_revision"),
                expectedParentNodeId: node("expected_parent_node_id"),
                expectedParentRevision: revision("expected_parent_revision"))
        }
    }

    func evidence(
        for result: ClientMutationSubmissionResult, mutation: PreparedClientMutation,
        preDispatchFailure: Bool = false
    )
        async throws -> (MutationQueueState, Data)
    {
        let evidence: MutationOutcomeEvidence
        let state: MutationQueueState
        switch result {
        case .applied(let value):
            var node: [String: Any] = [
                "id": value.node.id.rawValue, "library_id": value.node.libraryId.rawValue,
                "kind": value.node.kind.rawValue, "state": value.node.state.rawValue,
                "name": value.node.name,
                "revision": value.node.revision.rawValue,
                "created_at": timestamp(value.node.createdAt),
                "updated_at": timestamp(value.node.updatedAt),
            ]
            node["parent_node_id"] = value.node.parentNodeId?.rawValue
            node["current_version_id"] = value.node.currentVersionId?.rawValue
            node["trashed_at"] = value.node.trashedAt.map(timestamp)
            let body = try JSONSerialization.data(
                withJSONObject: [
                    "data": [
                        "outcome": "APPLIED", "mutation_id": value.mutationId.rawValue,
                        "kind": value.kind.rawValue,
                        "replayed": value.replayed,
                        "journal_event_id": value.journalEventId.rawValue,
                        "journal_sequence": value.journalSequence.rawValue, "node": node,
                    ], "meta": ["request_id": value.requestId],
                ], options: [.sortedKeys, .withoutEscapingSlashes])
            state = .applied
            evidence = MutationOutcomeEvidence(
                category: .applied, responseStatus: 200, responseBody: body,
                rejection: nil, uncertainty: nil)
        case .conflict(let value, let requestId):
            var details: [String: Any]?
            if let value {
                var data: [String: Any] = [
                    "outcome": "CONFLICT", "replayed": value.replayed,
                    "conflict_id": value.conflictId.rawValue, "reason": value.reason.rawValue,
                    "resource_id": value.resourceId.rawValue,
                    "server_epoch": value.serverEpoch.rawValue,
                    "server_sequence": value.serverSequence.rawValue,
                ]
                data["expected_revision"] = value.expectedRevision?.rawValue
                data["current_revision"] = value.currentRevision?.rawValue
                data["current_state"] = value.currentState?.rawValue
                data["current_parent_id"] = value.currentParentId?.rawValue
                data["current_name"] = value.currentName
                details = data
            }
            state = .conflict
            evidence = try errorEvidence(
                .conflict, code: "mutation_conflict", requestId: requestId, details: details)
        case .rebaselineRequired(let requestId):
            state = .blockedRebaseline
            evidence = try errorEvidence(
                .rebaseline, code: "sync_rebaseline_required", requestId: requestId)
        case .mutationIdConflict(let requestId):
            state = .failedPermanent
            evidence = try errorEvidence(
                .identityConflict, code: "mutation_id_conflict", requestId: requestId)
        case .failed(.permanentRejection(let code)):
            guard
                [.invalidMutation, .permissionDenied, .notFound, .invalidPersistedData].contains(
                    code)
            else {
                throw MutationQueueFailure.invalidTransition
            }
            state = .failedPermanent
            evidence = MutationOutcomeEvidence(
                category: .permanentRejection, responseStatus: nil,
                responseBody: nil, rejection: code.rawValue, uncertainty: nil)
        case .failed(let failure), .outcomeUnknown(let failure):
            // Even a local/cancellation failure after taking attempt ownership remains conservatively unknown.
            state = .outcomeUnknown
            let cause =
                preDispatchFailure
                ? "LOCAL_PRE_DISPATCH"
                : failure == .cancelled
                    ? "CANCELLED"
                    : failure == .staleSession ? "STALE_SESSION" : "UNVERIFIED_OUTCOME"
            evidence = MutationOutcomeEvidence(
                category: .outcomeUnknown, responseStatus: nil,
                responseBody: nil, rejection: nil, uncertainty: cause)
        }
        try await validateEvidence(evidence, state: state, mutation: mutation)
        return (state, try JSONEncoder().encode(evidence))
    }

    private func timestamp(_ date: Date) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.string(from: date)
    }

    private func errorEvidence(
        _ category: MutationOutcomeEvidence.Category, code: String, requestId: String,
        details: [String: Any]? = nil
    ) throws -> MutationOutcomeEvidence {
        var error: [String: Any] = [
            "code": code, "message": "Persisted verified outcome", "request_id": requestId,
            "retryable": false,
        ]
        error["details"] = details
        return MutationOutcomeEvidence(
            category: category, responseStatus: 409,
            responseBody: try JSONSerialization.data(
                withJSONObject: ["error": error], options: [.sortedKeys]),
            rejection: nil, uncertainty: nil)
    }

    func validateEvidence(
        _ evidence: MutationOutcomeEvidence?, state: MutationQueueState,
        mutation: PreparedClientMutation
    ) async throws {
        if state == .pending || state == .submitting {
            guard evidence == nil else { throw MutationQueueFailure.malformedRecord }
            return
        }
        guard let evidence else { throw MutationQueueFailure.malformedRecord }
        switch (state, evidence.category) {
        case (.outcomeUnknown, .outcomeUnknown):
            guard
                [
                    "CANCELLED", "STALE_SESSION", "UNVERIFIED_OUTCOME", "INTERRUPTED",
                    "LOCAL_PRE_DISPATCH", "LEASE_COMMIT_ACKNOWLEDGEMENT_LOST",
                ].contains(
                    evidence.uncertainty),
                evidence.responseStatus == nil, evidence.responseBody == nil,
                evidence.rejection == nil
            else { throw MutationQueueFailure.malformedRecord }
        case (.failedPermanent, .permanentRejection):
            guard let rejection = evidence.rejection,
                ["invalid_mutation", "permission_denied", "not_found", "invalid_persisted_data"]
                    .contains(rejection),
                evidence.responseStatus == nil, evidence.responseBody == nil,
                evidence.uncertainty == nil
            else { throw MutationQueueFailure.malformedRecord }
        case (.applied, .applied), (.conflict, .conflict), (.blockedRebaseline, .rebaseline),
            (.failedPermanent, .identityConflict):
            guard let status = evidence.responseStatus, let body = evidence.responseBody,
                evidence.rejection == nil, evidence.uncertainty == nil
            else { throw MutationQueueFailure.malformedRecord }
            let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
                HTTPTransportResponse(
                    statusCode: status, headers: ["Content-Type": "application/json"], body: body),
                for: mutation)
            switch (state, result) {
            case (.applied, .applied), (.conflict, .conflict),
                (.blockedRebaseline, .rebaselineRequired), (.failedPermanent, .mutationIdConflict):
                break
            default: throw MutationQueueFailure.malformedRecord
            }
        default: throw MutationQueueFailure.malformedRecord
        }
    }
}

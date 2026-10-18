import Foundation

private struct SyncFeedEnvelopeDTO: Decodable {
    let data: SyncFeedDataDTO
    let meta: LibraryMetadataDTO
}

private struct SyncFeedDataDTO: Decodable {
    let deviceId: String
    let libraryId: String
    let epoch: String
    let fromSequence: String
    let throughSequence: String
    let highWatermark: String
    let hasMore: Bool
    let changes: [SyncEventDTO]
    let ackToken: String?
    enum CodingKeys: String, CodingKey {
        case deviceId = "device_id"
        case libraryId = "library_id"
        case epoch
        case fromSequence = "from_sequence"
        case throughSequence = "through_sequence"
        case highWatermark = "high_watermark"
        case hasMore = "has_more"
        case changes
        case ackToken = "ack_token"
    }

}

private struct SyncEventDTO: Decodable {
    let eventId: String
    let schemaVersion: Int
    let sequence: String
    let resourceKind: String
    let resourceId: String
    let changeKind: SyncChangeKind
    let resourceRevision: String
    let occurredAt: String
    let parentNodeId: String?
    let nodeKind: NodeKind?
    let nodeState: NodeState?
    let currentVersionId: String?
    enum CodingKeys: String, CodingKey {
        case eventId = "event_id"
        case schemaVersion = "schema_version"
        case sequence
        case resourceKind = "resource_kind"
        case resourceId = "resource_id"
        case changeKind = "change_kind"
        case resourceRevision = "resource_revision"
        case occurredAt = "occurred_at"
        case parentNodeId = "parent_node_id"
        case nodeKind = "node_kind"
        case nodeState = "node_state"
        case currentVersionId = "current_version_id"
    }

}

struct SyncFeedResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol

    func decode(
        _ response: HTTPTransportResponse, scope: ClientMutationScope,
        expected: SyncJournalPosition, limit: Int = SyncFeedPolicy.maximumEvents
    ) async throws -> SyncFeedPage {
        do {
            guard (1...500).contains(limit), response.statusCode == 200,
                response.body.count <= SyncFeedPolicy.maximumResponseBytes,
                response.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                    .lowercased().split(separator: ";").first?.trimmingCharacters(in: .whitespaces)
                    == "application/json"
            else { throw SyncFeedFailure.protocolFailure }
            let root = try JSONSerialization.jsonObject(with: response.body)
            try LibraryWireValidation.keys(root, required: ["data", "meta"])
            let envelope = root as! [String: Any]
            try LibraryWireValidation.keys(envelope["meta"], required: ["request_id"])
            try LibraryWireValidation.keys(
                envelope["data"],
                required: [
                    "device_id", "library_id", "epoch", "from_sequence", "through_sequence",
                    "high_watermark", "has_more", "changes",
                ], optional: ["ack_token"])
            let data = envelope["data"] as! [String: Any]
            if let token = data["ack_token"], !(token is String) {
                throw SyncFeedFailure.protocolFailure
            }
            guard let changes = data["changes"] as? [[String: Any]], changes.count <= limit else {
                throw SyncFeedFailure.protocolFailure
            }
            for change in changes {
                try LibraryWireValidation.keys(
                    change,
                    required: [
                        "event_id", "schema_version", "sequence", "resource_kind", "resource_id",
                        "change_kind", "resource_revision", "occurred_at",
                    ],
                    optional: ["parent_node_id", "node_kind", "node_state", "current_version_id"])
                for key in ["parent_node_id", "node_kind", "node_state", "current_version_id"] {
                    if let value = change[key], !(value is String) {
                        throw SyncFeedFailure.protocolFailure
                    }
                }
            }
            let wire = try JSONDecoder().decode(SyncFeedEnvelopeDTO.self, from: response.body)
            let dto = wire.data
            let device = try await ClientMutationDeviceId.validated(dto.deviceId, using: bridge)
            let library = try await LibraryId.validated(dto.libraryId, using: bridge)
            guard device == scope.deviceId, library == scope.libraryId else {
                throw SyncFeedFailure.scopeMismatch
            }
            guard LibraryWireValidation.validRequestId(wire.meta.requestId) else {
                throw SyncFeedFailure.protocolFailure
            }
            if let header = response.headers.first(where: { $0.key.lowercased() == "x-request-id" }
            )?.value,
                header != wire.meta.requestId
            {
                throw SyncFeedFailure.protocolFailure
            }
            let epoch = try SyncDecimalValidation.validate(dto.epoch, nonzero: true)
            let from = try SyncDecimalValidation.validate(dto.fromSequence)
            let through = try SyncDecimalValidation.validate(dto.throughSequence)
            let high = try SyncDecimalValidation.validate(dto.highWatermark)
            guard epoch == expected.epoch else { throw SyncFeedFailure.rebaselineRequired }
            guard from == expected.sequence else { throw SyncFeedFailure.checkpointConflict }
            guard !SyncDecimalValidation.less(through.rawValue, from.rawValue),
                !SyncDecimalValidation.less(high.rawValue, through.rawValue),
                dto.hasMore == SyncDecimalValidation.less(through.rawValue, high.rawValue)
            else { throw SyncFeedFailure.protocolFailure }
            var sequence = UInt64(from.rawValue)!
            var identities = Set<ClientMutationJournalEventId>()
            var events: [SyncJournalEvent] = []
            for event in dto.changes {
                guard event.schemaVersion == 1, event.resourceKind == "NODE" else {
                    throw SyncFeedFailure.protocolFailure
                }
                let next = sequence.addingReportingOverflow(1)
                guard !next.overflow else { throw SyncFeedFailure.protocolFailure }
                let number = try SyncDecimalValidation.validate(event.sequence)
                guard number.rawValue == String(next.partialValue) else {
                    throw SyncFeedFailure.protocolFailure
                }
                sequence = next.partialValue
                let id = try await ClientMutationJournalEventId.validated(
                    event.eventId, using: bridge)
                guard identities.insert(id).inserted else { throw SyncFeedFailure.protocolFailure }
                let resource = try await NodeId.validated(event.resourceId, using: bridge)
                let parent: NodeId?
                if let value = event.parentNodeId {
                    parent = try await NodeId.validated(value, using: bridge)
                } else {
                    parent = nil
                }
                let version: FileVersionId?
                if let value = event.currentVersionId {
                    version = try await FileVersionId.validated(value, using: bridge)
                } else {
                    version = nil
                }
                events.append(
                    SyncJournalEvent(
                        id: id, schemaVersion: event.schemaVersion, sequence: number,
                        resourceId: resource, kind: event.changeKind,
                        resourceRevision: try SyncDecimalValidation.validate(
                            event.resourceRevision),
                        occurredAt: try LibraryWireValidation.timestamp(event.occurredAt),
                        parentId: parent,
                        nodeKind: event.nodeKind, nodeState: event.nodeState,
                        currentVersionId: version))
            }
            guard String(sequence) == through.rawValue else {
                throw SyncFeedFailure.protocolFailure
            }
            let evidence: SyncAckEvidence?
            if events.isEmpty {
                guard dto.ackToken == nil, from == through, !dto.hasMore else {
                    throw SyncFeedFailure.protocolFailure
                }
                evidence = nil
            } else {
                guard let token = dto.ackToken, SyncFeedPolicy.validToken(token) else {
                    throw SyncFeedFailure.protocolFailure
                }
                evidence = SyncAckEvidence(
                    scope: scope, epoch: epoch, from: from, through: through, highWatermark: high,
                    token: token)
            }
            try Task.checkCancellation()
            return SyncFeedPage(
                scope: scope, start: SyncJournalPosition(epoch: epoch, sequence: from),
                through: through, highWatermark: high, hasMore: dto.hasMore, events: events,
                evidence: evidence, requestId: wire.meta.requestId, responseBody: response.body,
                canonicalData: try JSONSerialization.data(
                    withJSONObject: data, options: [.sortedKeys]))
        } catch is CancellationError { throw CancellationError() } catch let failure
            as SyncFeedFailure
        { throw failure } catch { throw SyncFeedFailure.protocolFailure }
    }
}

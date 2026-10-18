import Foundation

private struct SyncCheckpointResponseDTO: Decodable {
    let data: SyncCheckpointDataDTO
    let meta: LibraryMetadataDTO
}

private struct SyncCheckpointDataDTO: Decodable {
    let deviceId: String
    let libraryId: String
    let epoch: String
    let acknowledgedSequence: String
    let createdAt: String
    let updatedAt: String
    let lastSeenHighWatermark: String?
    enum CodingKeys: String, CodingKey {
        case deviceId = "device_id", libraryId = "library_id", epoch
        case acknowledgedSequence = "acknowledged_sequence", createdAt = "created_at"
        case updatedAt = "updated_at", lastSeenHighWatermark = "last_seen_high_watermark"
    }
}

struct SyncCheckpointResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol

    func decode(_ response: HTTPTransportResponse, scope: ClientMutationScope) async throws
        -> SyncCheckpoint
    {
        guard response.body.count <= MutationQueuePolicy.maximumCheckpointBytes,
            response.statusCode == 200,
            response.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                .lowercased().split(separator: ";").first?.trimmingCharacters(in: .whitespaces)
                == "application/json"
        else { throw MutationQueueFailure.invalidCheckpoint }
        do {
            let object = try JSONSerialization.jsonObject(with: response.body)
            try LibraryWireValidation.keys(object, required: ["data", "meta"])
            let root = object as! [String: Any]
            try LibraryWireValidation.keys(root["meta"], required: ["request_id"])
            try LibraryWireValidation.keys(
                root["data"],
                required: [
                    "device_id", "library_id", "epoch", "acknowledged_sequence", "created_at",
                    "updated_at",
                ], optional: ["last_seen_high_watermark"])
            if let watermark = (root["data"] as? [String: Any])?["last_seen_high_watermark"],
                !(watermark is String)
            {
                throw MutationQueueFailure.invalidCheckpoint
            }
            let dto = try JSONDecoder().decode(SyncCheckpointResponseDTO.self, from: response.body)
            let device = try await ClientMutationDeviceId.validated(
                dto.data.deviceId, using: bridge)
            let library = try await LibraryId.validated(dto.data.libraryId, using: bridge)
            guard device == scope.deviceId, library == scope.libraryId,
                LibraryWireValidation.validRequestId(dto.meta.requestId)
            else { throw MutationQueueFailure.invalidCheckpoint }
            if let header = response.headers.first(where: { $0.key.lowercased() == "x-request-id" }
            )?.value {
                guard header == dto.meta.requestId else {
                    throw MutationQueueFailure.invalidCheckpoint
                }
            }
            let epoch = try SyncDecimalValidation.validate(dto.data.epoch, nonzero: true)
            let sequence = try SyncDecimalValidation.validate(dto.data.acknowledgedSequence)
            let watermark = try dto.data.lastSeenHighWatermark.map {
                try SyncDecimalValidation.validate($0)
            }
            if let watermark, SyncDecimalValidation.less(watermark.rawValue, sequence.rawValue) {
                throw MutationQueueFailure.invalidCheckpoint
            }
            let created = try LibraryWireValidation.timestamp(dto.data.createdAt)
            let updated = try LibraryWireValidation.timestamp(dto.data.updatedAt)
            guard created <= updated else { throw MutationQueueFailure.invalidCheckpoint }
            try Task.checkCancellation()
            return SyncCheckpoint(
                base: try ClientMutationBase(scope: scope, epoch: epoch, sequence: sequence),
                createdAt: created, updatedAt: updated, lastSeenHighWatermark: watermark,
                requestId: dto.meta.requestId, responseBody: response.body)
        } catch is CancellationError { throw CancellationError() } catch {
            throw MutationQueueFailure.invalidCheckpoint
        }
    }
}

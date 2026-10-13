import Foundation

/// Logical scheduling states, separate from P045's server/upload publication state.
enum BackgroundUploadState: String, Codable, Sendable, CaseIterable {
    case foregroundOnly, preparing, queued, scheduled, transferring, awaitingEvidence
    case reconciling, paused, suspended, completedChunk, awaitingFinalization
    case failedRecoverable, failedTerminal, aborted, quarantined

    var label: String {
        switch self {
        case .foregroundOnly: "Foreground only"
        case .preparing: "Preparing background payload"
        case .queued: "Prepared — waiting for submission"
        case .scheduled, .suspended: "Waiting for the system"
        case .transferring: "System transfer active — progress not yet confirmed"
        case .awaitingEvidence, .reconciling: "Waiting for server reconciliation"
        case .paused: "Background transfer paused"
        case .completedChunk: "Chunk confirmed by server"
        case .awaitingFinalization: "Bytes accepted — Resume and Complete required"
        case .failedRecoverable: "Background recovery required"
        case .failedTerminal: "Background transfer failed"
        case .aborted: "Upload aborted"
        case .quarantined: "Background authorization no longer valid"
        }
    }

    func permits(_ next: Self, explicitResume: Bool = false) -> Bool {
        if next == self { return true }
        if next == .quarantined { return true }
        if [.quarantined, .aborted, .failedTerminal].contains(self) { return false }
        if next == .paused || next == .failedTerminal { return true }
        switch self {
        case .foregroundOnly: return next == .preparing && explicitResume
        case .paused:
            return explicitResume && [.preparing, .reconciling, .foregroundOnly].contains(next)
        case .preparing:
            return [.queued, .failedRecoverable].contains(next)
                || (next == .foregroundOnly && explicitResume)
        case .queued:
            return [.scheduled, .awaitingEvidence, .reconciling, .failedRecoverable].contains(next)
        case .scheduled, .transferring, .suspended:
            return [.transferring, .suspended, .awaitingEvidence, .reconciling].contains(next)
        case .awaitingEvidence: return [.reconciling, .failedRecoverable].contains(next)
        case .reconciling:
            return [.completedChunk, .awaitingFinalization, .failedRecoverable, .aborted].contains(
                next)
        case .completedChunk:
            return [.preparing, .foregroundOnly, .awaitingFinalization].contains(next)
        case .awaitingFinalization: return next == .foregroundOnly && explicitResume
        case .failedRecoverable:
            return next == .reconciling
                || ([.preparing, .foregroundOnly].contains(next) && explicitResume)
        case .failedTerminal, .aborted, .quarantined: return false
        }
    }
}

struct BackgroundChunkIntent: Codable, Equatable, Sendable {
    let id: String
    let offset: UInt64
    let length: Int
    let sha256: String
}

/// Only allowlisted, bounded response headers and a safe failure category are persisted.
/// No bearer, request URL, source path, error description or response body is serialized.
struct BackgroundUploadEvidence: Codable, Equatable, Sendable {
    let taskIdentifier: Int
    let status: Int?
    let offset: String?
    let requestId: String?
    let bodyWasEmpty: Bool
    let failure: String?

    func acceptedOffset(chunk: BackgroundChunkIntent, expected: UInt64) throws -> UInt64 {
        guard failure == nil, let status else { throw UploadFailure.completionUnknown }
        var headers: [String: String] = [:]
        headers["Upload-Offset"] = offset
        headers["X-Request-Id"] = requestId
        return try UploadSessionResponseDecoder.patchOffset(
            HTTPTransportResponse(
                statusCode: status, headers: headers,
                body: bodyWasEmpty ? Data() : Data([0])),
            prior: chunk.offset, chunkBytes: chunk.length, expectedBytes: expected)
    }
}

struct BackgroundUploadJob: Codable, Equatable, Sendable {
    let operationId: String
    var encodingVersion: Int = 1
    var generation: Int
    var authorized: Bool
    var state: BackgroundUploadState
    var chunk: BackgroundChunkIntent?
    var resolved: Bool
    var taskIdentifier: Int?
    var evidence: BackgroundUploadEvidence?

    var blocksForeground: Bool { chunk != nil && !resolved }

    mutating func transition(_ next: BackgroundUploadState, explicitResume: Bool = false) throws {
        guard state.permits(next, explicitResume: explicitResume) else {
            throw UploadFailure.invalidMetadata
        }
        state = next
    }

    static func decodeStored(_ bytes: Data) throws -> Self {
        let root = try JSONSerialization.jsonObject(with: bytes)
        try LibraryWireValidation.keys(
            root,
            required: [
                "operationId", "encodingVersion", "generation", "authorized", "state", "resolved",
            ],
            optional: ["chunk", "taskIdentifier", "evidence"])
        guard let object = root as? [String: Any] else { throw UploadFailure.invalidMetadata }
        if let chunk = object["chunk"] {
            try LibraryWireValidation.keys(chunk, required: ["id", "offset", "length", "sha256"])
        }
        if let evidence = object["evidence"] {
            try LibraryWireValidation.keys(
                evidence, required: ["taskIdentifier", "bodyWasEmpty"],
                optional: ["status", "offset", "requestId", "failure"])
        }
        let job = try JSONDecoder().decode(Self.self, from: bytes)
        try job.validate()
        return job
    }

    func validate() throws {
        _ = try UploadOperationId(validating: operationId)
        guard encodingVersion == 1, (0...1000).contains(generation),
            taskIdentifier.map({ $0 > 0 }) ?? true,
            evidence.map({ $0.taskIdentifier > 0 && $0.taskIdentifier == taskIdentifier }) ?? true,
            evidence?.status.map({ (100...599).contains($0) }) ?? true,
            !authorized
                || ![.paused, .quarantined, .failedTerminal, .aborted, .foregroundOnly].contains(
                    state),
            evidence?.offset.map({ $0.utf8.count <= 32 }) ?? true,
            evidence?.requestId.map({ $0.utf8.count <= 128 }) ?? true,
            evidence?.failure.map({ UploadFailure(rawValue: $0) != nil }) ?? true
        else { throw UploadFailure.invalidMetadata }
        if let chunk {
            _ = try UploadOperationId(validating: chunk.id)
            guard (1...UploadPolicy.chunkBytes).contains(chunk.length),
                chunk.offset <= DownloadPolicy.quota,
                UInt64(chunk.length) <= DownloadPolicy.quota - chunk.offset,
                LibraryWireValidation.matches(chunk.sha256, pattern: "^sha256:[0-9a-f]{64}$")
            else { throw UploadFailure.invalidMetadata }
        } else {
            guard resolved, taskIdentifier == nil, evidence == nil else {
                throw UploadFailure.invalidMetadata
            }
        }
    }
}

struct BackgroundSystemTask: Equatable, Sendable {
    let identifier: Int
    let intentId: String?
    var requestURL: URL? = nil
    var method: String? = nil
    var offset: String? = nil
    var length: String? = nil
}

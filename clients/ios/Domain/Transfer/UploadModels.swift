import Foundation

struct UploadOperationId: Hashable, Sendable {
    let rawValue: String
    init() { rawValue = UUID().uuidString.lowercased() }
    init(validating value: String) throws {
        guard UUID(uuidString: value)?.uuidString.lowercased() == value else {
            throw UploadFailure.invalidMetadata
        }
        rawValue = value
    }
}

struct UploadExpectedContent: Equatable, Sendable {
    let byteLength: UInt64
    let sha256: String

    init(byteLength: UInt64, sha256: String) throws {
        guard byteLength <= DownloadPolicy.quota,
            LibraryWireValidation.matches(sha256, pattern: "^sha256:[0-9a-f]{64}$")
        else { throw UploadFailure.invalidMetadata }
        self.byteLength = byteLength
        self.sha256 = sha256
    }

    init(length: String, sha256: String) throws {
        guard let byteLength = DownloadPolicy.decimal(length) else {
            throw UploadFailure.invalidMetadata
        }
        try self.init(byteLength: byteLength, sha256: sha256)
    }
}

enum UploadTarget: Equatable, Sendable {
    case createFile(library: LibraryId, parent: NodeId, name: String)
    case replaceContent(library: LibraryId, node: NodeId, expectedRevision: NodeRevision)

    var library: LibraryId {
        switch self {
        case .createFile(let library, _, _), .replaceContent(let library, _, _): library
        }
    }

    var operation: String {
        switch self {
        case .createFile: "CREATE_FILE"
        case .replaceContent: "REPLACE_CONTENT"
        }
    }

    var node: NodeId? {
        if case .replaceContent(_, let node, _) = self { return node }
        return nil
    }

    var parent: NodeId? {
        if case .createFile(_, let parent, _) = self { return parent }
        return nil
    }

    var name: String? {
        if case .createFile(_, _, let name) = self { return name }
        return nil
    }

    var expectedRevision: NodeRevision? {
        if case .replaceContent(_, _, let revision) = self { return revision }
        return nil
    }
}

/// Exact request bytes are held with the immutable target and reused for CREATE recovery.
struct UploadIntent: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    let idempotencyKey: ClientMutationId
    let target: UploadTarget
    let expected: UploadExpectedContent
    let requestBytes: Data

    var description: String { "[REDACTED_UPLOAD_INTENT]" }
    var debugDescription: String { description }

    static func prepare(
        target: UploadTarget, expected: UploadExpectedContent,
        generator: any ClientMutationIdentityGeneratorProtocol,
        bridge: any RustBridgeProtocol
    ) async throws -> Self {
        _ = try await LibraryId.validated(target.library.rawValue, using: bridge)
        switch target {
        case .createFile(_, let parent, let name):
            _ = try await NodeId.validated(parent.rawValue, using: bridge)
            guard try await bridge.validateLogicalName(name) else {
                throw UploadFailure.invalidMetadata
            }
        case .replaceContent(_, let node, let revision):
            _ = try await NodeId.validated(node.rawValue, using: bridge)
            _ = try NodeRevision(validating: revision.rawValue)
        }
        let key = try await ClientMutationId.generate(using: generator, validator: bridge)
        let requestBytes = try Self.encode(target: target, key: key, expected: expected)
        guard requestBytes.count <= UploadPolicy.maximumJSONBytes else {
            throw UploadFailure.payloadTooLarge
        }
        return Self(
            idempotencyKey: key, target: target, expected: expected, requestBytes: requestBytes)
    }

    static func restore(
        idempotencyKey: ClientMutationId, target: UploadTarget,
        expected: UploadExpectedContent, requestBytes: Data
    ) throws -> Self {
        let canonical = try encode(target: target, key: idempotencyKey, expected: expected)
        guard canonical == requestBytes, canonical.count <= UploadPolicy.maximumJSONBytes else {
            throw UploadFailure.invalidMetadata
        }
        return Self(
            idempotencyKey: idempotencyKey, target: target, expected: expected,
            requestBytes: requestBytes)
    }

    private static func encode(
        target: UploadTarget, key: ClientMutationId, expected: UploadExpectedContent
    ) throws -> Data {
        var object: [String: String] = [
            "operation": target.operation,
            "idempotency_key": key.rawValue,
            "library_id": target.library.rawValue,
            "expected_bytes": String(expected.byteLength),
            "expected_sha256": expected.sha256,
        ]
        switch target {
        case .createFile(_, let parent, let name):
            object["parent_id"] = parent.rawValue
            object["name"] = name
        case .replaceContent(_, let node, let revision):
            object["node_id"] = node.rawValue
            object["expected_revision"] = revision.rawValue
        }
        return try JSONSerialization.data(
            withJSONObject: object, options: [.sortedKeys, .withoutEscapingSlashes])
    }
}

enum UploadServerState: String, Sendable, CaseIterable {
    case open = "OPEN", verifying = "VERIFYING", committing = "COMMITTING"
    case committed = "COMMITTED", failed = "FAILED", expired = "EXPIRED", aborted = "ABORTED"
}

enum UploadLocalState: String, Sendable, CaseIterable {
    case preparingSource = "PREPARING_SOURCE"
    case sourceStaged = "SOURCE_STAGED"
    case sessionCreating = "SESSION_CREATING"
    case sessionOpen = "SESSION_OPEN"
    case uploading = "UPLOADING"
    case readyToComplete = "READY_TO_COMPLETE"
    case completing = "COMPLETING"
    case committed = "COMMITTED"
    case outcomeUnknown = "OUTCOME_UNKNOWN"
    case recoveryRequired = "RECOVERY_REQUIRED"
    case cancelledLocal = "CANCELLED_LOCAL"
    case abortPending = "ABORT_PENDING"
    case aborted = "ABORTED"
    case expired = "EXPIRED"
    case failedRecoverable = "FAILED_RECOVERABLE"
    case failedPermanent = "FAILED_PERMANENT"
    case storageUnavailable = "STORAGE_UNAVAILABLE"
    case sessionInvalidated = "SESSION_INVALIDATED"

    var canResume: Bool {
        [
            .sourceStaged, .sessionCreating, .sessionOpen, .uploading, .readyToComplete,
            .completing, .outcomeUnknown, .recoveryRequired, .cancelledLocal,
            .failedRecoverable,
        ].contains(self)
    }

    var label: String {
        switch self {
        case .preparingSource: "Preparing file"
        case .sourceStaged: "File ready"
        case .sessionCreating: "Creating upload session"
        case .sessionOpen: "Upload session ready"
        case .uploading: "Uploading"
        case .readyToComplete: "Ready to complete"
        case .completing: "Verifying upload"
        case .committed: "Uploaded"
        case .outcomeUnknown: "Checking server status required"
        case .recoveryRequired: "Recovery required"
        case .cancelledLocal: "Paused — Resume available"
        case .abortPending: "Abort status unknown"
        case .aborted: "Upload aborted"
        case .expired: "Upload session expired"
        case .failedRecoverable: "Failed — Resume available"
        case .failedPermanent: "Failed — Recovery required"
        case .storageUnavailable: "Private storage unavailable"
        case .sessionInvalidated: "Device session changed"
        }
    }
}

struct UploadCompletion: Equatable, Sendable {
    let sessionId: String
    let nodeId: NodeId
    let fileVersionId: FileVersionId
    let revision: NodeRevision
    let expected: UploadExpectedContent
    let committedAt: Date
    let requestId: String

    func persistedEvidence() throws -> Data {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return try JSONSerialization.data(
            withJSONObject: [
                "data": [
                    "id": sessionId,
                    "type": "upload_completion",
                    "attributes": [
                        "node_id": nodeId.rawValue,
                        "file_version_id": fileVersionId.rawValue,
                        "node_revision": revision.rawValue,
                        "bytes": String(expected.byteLength),
                        "sha256": expected.sha256,
                        "committed_at": formatter.string(from: committedAt),
                    ],
                ],
                "meta": ["request_id": requestId],
            ], options: [.sortedKeys, .withoutEscapingSlashes])
    }
}

struct UploadRecord: Equatable, Sendable {
    let id: UploadOperationId
    let stagingId: UploadOperationId
    let stagingReleased: Bool
    let intent: UploadIntent
    let scope: ClientMutationScope
    let credentialId: String
    let originatingSessionId: String
    let serverSessionId: String?
    let serverTargetNodeId: NodeId?
    let authoritativeOffset: UInt64
    let serverState: UploadServerState?
    let state: UploadLocalState
    let uncertain: Bool
    let attemptCount: Int
    let completion: UploadCompletion?
    let failure: UploadFailure?
}

struct UploadProgress: Equatable, Sendable {
    let operationId: UploadOperationId
    let acknowledgedBytes: UInt64
    let totalBytes: UInt64
    let state: UploadLocalState
    init(_ record: UploadRecord) {
        operationId = record.id
        acknowledgedBytes = record.authoritativeOffset
        totalBytes = record.intent.expected.byteLength
        state = record.state
    }
}

struct UploadStorageReservation: Sendable {
    let operationId: UploadOperationId
    let bytes: UInt64
}

enum UploadFailure: String, Error, Sendable, CaseIterable {
    case unauthenticated, deviceRevoked, staleSession, scopeMismatch, offline, dnsFailure, timeout
    case tlsFailure, redirectRejected, serverUnavailable, invalidMetadata, invalidTarget
    case revisionConflict, nameConflict, unexpectedStatus, malformedOffset, offsetMismatch
    case chunkTooLarge, unsupportedMediaType, capacityUnavailable, sessionExpired, sessionAborted
    case serverFailed, integrityMismatch, sourceUnavailable, sourceChanged, invalidSource
    case insufficientStorage, stagingQuotaExceeded, stagingMissing, filesystemFailure,
        storageFailure
    case cancelled, duplicateOperation, payloadTooLarge, completionUnknown, conflict

    var localState: UploadLocalState {
        switch self {
        case .cancelled: .cancelledLocal
        case .staleSession, .scopeMismatch, .unauthenticated, .deviceRevoked: .sessionInvalidated
        case .insufficientStorage, .stagingQuotaExceeded, .filesystemFailure, .storageFailure:
            .storageUnavailable
        case .offline, .dnsFailure, .timeout, .serverUnavailable, .completionUnknown:
            .failedRecoverable
        case .revisionConflict, .nameConflict, .conflict, .sessionExpired: .failedPermanent
        default: .failedPermanent
        }
    }

    var message: String {
        switch self {
        case .offline, .dnsFailure, .timeout, .serverUnavailable:
            "The upload stopped. Check your connection, then choose Resume."
        case .revisionConflict:
            "This file changed on the server. Refresh it and start a new replacement with the current revision."
        case .nameConflict:
            "A file with this exact name already exists in the destination folder."
        case .insufficientStorage, .stagingQuotaExceeded:
            "There is not enough private storage to stage this file. Free space and try again."
        case .sessionExpired:
            "The server upload session expired. Start a new upload if you still need this file."
        case .cancelled: "The upload was paused. Its saved source and server identity are retained."
        case .staleSession, .scopeMismatch, .unauthenticated, .deviceRevoked:
            "The authenticated device session changed. This upload cannot use the old authorization."
        case .sourceChanged, .integrityMismatch:
            "The staged source failed integrity verification and will not be uploaded."
        default:
            "The upload could not be safely completed. Check server status or contact the server owner."
        }
    }
}

enum UploadPolicy {
    static let chunkBytes = 4 * 1024 * 1024
    static let sourceBufferBytes = 64 * 1024
    static let maximumJSONBytes = 16 * 1024
    static let maximumResponseBytes = 64 * 1024
    static let emptyDigest = DownloadPolicy.emptyDigest

    static func classify(_ error: Error) -> UploadFailure {
        if error is CancellationError { return .cancelled }
        if let failure = error as? UploadFailure { return failure }
        if let failure = error as? DownloadFailure {
            switch failure {
            case .cancelled: return .cancelled
            case .insufficientStorage: return .insufficientStorage
            case .stagingQuotaExceeded: return .stagingQuotaExceeded
            case .stagingMissing: return .stagingMissing
            case .filesystemFailure: return .filesystemFailure
            case .sha256Mismatch, .integrityFailure: return .integrityMismatch
            default: return .storageFailure
            }
        }
        if let failure = error as? LibraryFailure {
            switch failure {
            case .unauthenticated, .authenticationRejected: return .unauthenticated
            case .deviceRevoked: return .deviceRevoked
            case .staleSession, .invalidCredential, .credentialUnavailable: return .staleSession
            case .originMismatch: return .scopeMismatch
            case .offline: return .offline
            case .dnsFailure: return .dnsFailure
            case .timeout: return .timeout
            case .tlsFailure: return .tlsFailure
            case .redirectRejected: return .redirectRejected
            case .serverUnavailable: return .serverUnavailable
            case .cancelled: return .cancelled
            default: return .unexpectedStatus
            }
        }
        if let failure = error as? SynveilTransportError {
            switch failure {
            case .offline: return .offline
            case .dnsFailure: return .dnsFailure
            case .timeout: return .timeout
            case .tlsError: return .tlsFailure
            case .redirectRejected: return .redirectRejected
            case .cancelled: return .cancelled
            case .configurationError: return .scopeMismatch
            case .bodyLimitExceeded, .unexpectedContentType, .malformedResponse,
                .protocolError, .httpError:
                return .invalidMetadata
            }
        }
        if let error = error as? URLError {
            switch error.code {
            case .cancelled: return .cancelled
            case .timedOut: return .timeout
            case .cannotFindHost, .dnsLookupFailed: return .dnsFailure
            case .secureConnectionFailed, .serverCertificateUntrusted, .serverCertificateHasBadDate,
                .serverCertificateHasUnknownRoot, .serverCertificateNotYetValid,
                .clientCertificateRejected, .clientCertificateRequired:
                return .tlsFailure
            default: return .offline
            }
        }
        return .storageFailure
    }
}

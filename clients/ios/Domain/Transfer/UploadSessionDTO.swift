import Foundation

struct UploadSessionEvidence: Equatable, Sendable {
    let id: String
    let operation: String
    let state: UploadServerState
    let receivedBytes: UInt64
    let expected: UploadExpectedContent
    let libraryId: LibraryId
    let parentId: NodeId?
    let nodeId: NodeId
    let name: String?
    let expectedRevision: NodeRevision?
    let createdAt: Date
    let updatedAt: Date
    let expiresAt: Date
    let requestId: String
    let completion: UploadCompletion?
}

struct UploadSessionResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol

    func decode(
        _ response: HTTPTransportResponse,
        intent: UploadIntent,
        expectedSessionId: String? = nil,
        expectedOffset: UInt64? = nil,
        requireLocation: Bool = false
    ) async throws -> UploadSessionEvidence {
        guard response.statusCode == (requireLocation ? 201 : 200),
            response.body.count <= UploadPolicy.maximumResponseBytes,
            Self.isJSON(response)
        else { throw UploadFailure.unexpectedStatus }
        do {
            let root = try JSONSerialization.jsonObject(with: response.body)
            try LibraryWireValidation.keys(root, required: ["data", "meta"])
            let envelope = root as? [String: Any]
            try LibraryWireValidation.keys(envelope?["meta"], required: ["request_id"])
            guard let resource = envelope?["data"] as? [String: Any] else {
                throw UploadFailure.invalidMetadata
            }
            try LibraryWireValidation.keys(resource, required: ["id", "type", "attributes"])
            guard resource["type"] as? String == "upload_session",
                let id = resource["id"] as? String,
                let attributes = resource["attributes"] as? [String: Any]
            else { throw UploadFailure.invalidMetadata }
            let typedSessionId = try await ClientMutationId.validated(id, using: bridge)
            guard expectedSessionId.map({ $0 == typedSessionId.rawValue }) ?? true else {
                throw UploadFailure.invalidMetadata
            }
            try LibraryWireValidation.keys(
                attributes,
                required: [
                    "operation", "state", "received_bytes", "expected_bytes", "created_at",
                    "updated_at", "expires_at", "target",
                ],
                optional: [
                    "expected_sha256", "last_error_code", "terminal_failure_code", "completion",
                ])
            guard let meta = envelope?["meta"] as? [String: Any],
                let requestId = meta["request_id"] as? String,
                LibraryWireValidation.validRequestId(requestId),
                requestHeader(response).map({ $0 == requestId }) ?? true,
                let operation = attributes["operation"] as? String,
                operation == intent.target.operation,
                let stateRaw = attributes["state"] as? String,
                let state = UploadServerState(rawValue: stateRaw),
                let receivedRaw = attributes["received_bytes"] as? String,
                let received = DownloadPolicy.decimal(receivedRaw),
                let expectedRaw = attributes["expected_bytes"] as? String,
                let expectedBytes = DownloadPolicy.decimal(expectedRaw),
                expectedBytes == intent.expected.byteLength,
                let expectedHash = attributes["expected_sha256"] as? String,
                expectedHash == intent.expected.sha256,
                received <= expectedBytes,
                let offset = uploadOffset(response), offset == received,
                expectedOffset.map({ $0 == received }) ?? true
            else { throw UploadFailure.invalidMetadata }

            if requireLocation {
                guard header(response, "location") == "/api/v1/upload-sessions/\(id)"
                else { throw UploadFailure.invalidMetadata }
            }
            for key in ["last_error_code", "terminal_failure_code"] {
                if let raw = attributes[key] {
                    guard let code = raw as? String,
                        LibraryWireValidation.matches(code, pattern: "^[a-z][a-z0-9_]{1,63}$")
                    else { throw UploadFailure.invalidMetadata }
                }
            }
            let target = try await decodeTarget(attributes["target"], intent: intent)
            let createdAt = try LibraryWireValidation.timestamp(
                attributes["created_at"] as? String ?? "")
            let updatedAt = try LibraryWireValidation.timestamp(
                attributes["updated_at"] as? String ?? "")
            let expiresAt = try LibraryWireValidation.timestamp(
                attributes["expires_at"] as? String ?? "")
            guard createdAt <= updatedAt else { throw UploadFailure.invalidMetadata }

            let completion: UploadCompletion?
            if let rawCompletion = attributes["completion"] {
                guard let object = rawCompletion as? [String: Any] else {
                    throw UploadFailure.invalidMetadata
                }
                completion = try await decodeCompletionResource(
                    object, sessionId: id, nodeId: target.nodeId,
                    expected: intent.expected, requestId: requestId)
            } else {
                completion = nil
            }
            guard (state == .committed) == (completion != nil) else {
                throw UploadFailure.invalidMetadata
            }
            if state == .committed, received != expectedBytes {
                throw UploadFailure.invalidMetadata
            }
            return UploadSessionEvidence(
                id: id, operation: operation, state: state, receivedBytes: received,
                expected: intent.expected, libraryId: target.libraryId,
                parentId: target.parentId, nodeId: target.nodeId, name: target.name,
                expectedRevision: target.expectedRevision, createdAt: createdAt,
                updatedAt: updatedAt, expiresAt: expiresAt, requestId: requestId,
                completion: completion)
        } catch let failure as UploadFailure {
            throw failure
        } catch {
            throw UploadFailure.invalidMetadata
        }
    }

    func decodeCompletion(
        _ response: HTTPTransportResponse, sessionId: String,
        intent: UploadIntent, targetNodeId: NodeId
    ) async throws -> UploadCompletion {
        guard response.statusCode == 200,
            response.body.count <= UploadPolicy.maximumResponseBytes,
            Self.isJSON(response)
        else { throw UploadFailure.unexpectedStatus }
        do {
            let root = try JSONSerialization.jsonObject(with: response.body)
            try LibraryWireValidation.keys(root, required: ["data", "meta"])
            guard let envelope = root as? [String: Any],
                let meta = envelope["meta"] as? [String: Any]
            else { throw UploadFailure.invalidMetadata }
            try LibraryWireValidation.keys(meta, required: ["request_id"])
            guard let requestId = meta["request_id"] as? String,
                LibraryWireValidation.validRequestId(requestId),
                requestHeader(response).map({ $0 == requestId }) ?? true,
                let resource = envelope["data"] as? [String: Any]
            else { throw UploadFailure.invalidMetadata }
            return try await decodeCompletionResource(
                resource, sessionId: sessionId, nodeId: targetNodeId,
                expected: intent.expected, requestId: requestId)
        } catch let failure as UploadFailure {
            throw failure
        } catch {
            throw UploadFailure.invalidMetadata
        }
    }

    static func patchOffset(
        _ response: HTTPTransportResponse, prior: UInt64,
        chunkBytes: Int, expectedBytes: UInt64
    ) throws -> UInt64 {
        guard response.statusCode == 204, response.body.isEmpty,
            chunkBytes > 0, chunkBytes <= UploadPolicy.chunkBytes,
            prior <= expectedBytes,
            UInt64(chunkBytes) <= expectedBytes - prior,
            let requestId = responseHeader(response, "x-request-id"),
            LibraryWireValidation.validRequestId(requestId),
            let raw = responseHeader(response, "upload-offset"),
            DownloadPolicy.decimal(raw) == prior + UInt64(chunkBytes)
        else { throw UploadFailure.offsetMismatch }
        return prior + UInt64(chunkBytes)
    }

    static func failure(_ response: HTTPTransportResponse) -> UploadFailure {
        if (300...399).contains(response.statusCode) { return .redirectRejected }
        let code: String
        do {
            guard response.body.count <= UploadPolicy.maximumResponseBytes,
                response.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                    .components(separatedBy: ";").first?.trimmingCharacters(
                        in: .whitespacesAndNewlines
                    )
                    .lowercased() == "application/json"
            else { return .unexpectedStatus }
            let root = try JSONSerialization.jsonObject(with: response.body)
            try LibraryWireValidation.keys(root, required: ["error"])
            guard let envelope = root as? [String: Any],
                let error = envelope["error"] as? [String: Any]
            else { return .unexpectedStatus }
            try LibraryWireValidation.keys(
                error, required: ["code", "message", "request_id", "retryable"],
                optional: ["details"])
            guard let raw = error["code"] as? String,
                LibraryWireValidation.matches(raw, pattern: "^[a-z][a-z0-9_]{1,63}$"),
                let message = error["message"] as? String, !message.isEmpty,
                message.utf8.count <= 512,
                let requestId = error["request_id"] as? String,
                LibraryWireValidation.validRequestId(requestId),
                let retryable = error["retryable"] as? Bool,
                error["details"] == nil || error["details"] is [String: Any],
                response.headers.first(where: { $0.key.lowercased() == "x-request-id" })
                    .map({ $0.value == requestId }) ?? true
            else { return .unexpectedStatus }
            _ = retryable
            code = raw
        } catch {
            return .unexpectedStatus
        }
        switch response.statusCode {
        case 401: return code == "device_revoked" ? .deviceRevoked : .unauthenticated
        case 403: return .permissionDenied
        case 404: return .sourceUnavailable
        case 409:
            switch code {
            case "invalid_offset": return .offsetMismatch
            case "version_conflict": return .revisionConflict
            case "invalid_state", "completion_conflict", "idempotency_conflict": return .conflict
            case "upload_in_progress": return .conflict
            default: return .conflict
            }
        case 410: return .sessionExpired
        case 412: return .revisionConflict
        case 413: return .chunkTooLarge
        case 415: return .unsupportedMediaType
        case 507: return .capacityUnavailable
        case 500...599: return .serverUnavailable
        default: return .unexpectedStatus
        }
    }

    private func decodeTarget(
        _ value: Any?, intent: UploadIntent
    ) async throws -> (
        libraryId: LibraryId, parentId: NodeId?, nodeId: NodeId,
        name: String?, expectedRevision: NodeRevision?
    ) {
        guard let target = value as? [String: Any] else {
            throw UploadFailure.invalidMetadata
        }
        switch intent.target {
        case .createFile(let library, let parent, let name):
            try LibraryWireValidation.keys(
                target, required: ["operation", "library_id", "parent_id", "node_id", "name"])
            guard target["operation"] as? String == "CREATE_FILE",
                target["library_id"] as? String == library.rawValue,
                target["parent_id"] as? String == parent.rawValue,
                target["name"] as? String == name,
                let nodeRaw = target["node_id"] as? String
            else { throw UploadFailure.invalidTarget }
            return (
                library, parent, try await NodeId.validated(nodeRaw, using: bridge), name, nil
            )
        case .replaceContent(let library, let node, let revision):
            try LibraryWireValidation.keys(
                target, required: ["operation", "library_id", "node_id", "expected_revision"])
            guard target["operation"] as? String == "REPLACE_CONTENT",
                target["library_id"] as? String == library.rawValue,
                target["node_id"] as? String == node.rawValue,
                target["expected_revision"] as? String == revision.rawValue
            else { throw UploadFailure.invalidTarget }
            return (library, nil, node, nil, revision)
        }
    }

    private func decodeCompletionResource(
        _ resource: [String: Any], sessionId: String, nodeId: NodeId,
        expected: UploadExpectedContent, requestId: String
    ) async throws -> UploadCompletion {
        try LibraryWireValidation.keys(resource, required: ["id", "type", "attributes"])
        guard resource["id"] as? String == sessionId,
            resource["type"] as? String == "upload_completion",
            let attributes = resource["attributes"] as? [String: Any]
        else { throw UploadFailure.invalidMetadata }
        try LibraryWireValidation.keys(
            attributes,
            required: [
                "node_id", "file_version_id", "node_revision", "bytes", "sha256", "committed_at",
            ])
        guard let rawNode = attributes["node_id"] as? String,
            try await NodeId.validated(rawNode, using: bridge) == nodeId,
            let rawVersion = attributes["file_version_id"] as? String,
            let rawRevision = attributes["node_revision"] as? String,
            let rawBytes = attributes["bytes"] as? String,
            let bytes = DownloadPolicy.decimal(rawBytes), bytes == expected.byteLength,
            let hash = attributes["sha256"] as? String, hash == expected.sha256,
            let committedAt = attributes["committed_at"] as? String
        else { throw UploadFailure.integrityMismatch }
        return UploadCompletion(
            sessionId: sessionId, nodeId: nodeId,
            fileVersionId: try await FileVersionId.validated(rawVersion, using: bridge),
            revision: try NodeRevision(validating: rawRevision), expected: expected,
            committedAt: try LibraryWireValidation.timestamp(committedAt), requestId: requestId)
    }

    private func requestHeader(_ response: HTTPTransportResponse) -> String? {
        header(response, "x-request-id")
    }

    private func uploadOffset(_ response: HTTPTransportResponse) -> UInt64? {
        guard let raw = header(response, "upload-offset") else { return nil }
        return DownloadPolicy.decimal(raw)
    }

    private func header(_ response: HTTPTransportResponse, _ name: String) -> String? {
        Self.responseHeader(response, name)
    }

    private static func responseHeader(_ response: HTTPTransportResponse, _ name: String) -> String?
    {
        let values = response.headers.filter { $0.key.lowercased() == name }
        guard values.count == 1, let value = values.first?.value,
            !value.unicodeScalars.contains(where: { $0.value == 10 || $0.value == 13 })
        else { return nil }
        return value
    }

    private static func isJSON(_ response: HTTPTransportResponse) -> Bool {
        let values = response.headers.filter { $0.key.lowercased() == "content-type" }
        guard values.count == 1, let value = values.first?.value else { return false }
        return value.components(separatedBy: ";").first?
            .trimmingCharacters(in: .whitespacesAndNewlines).lowercased() == "application/json"
    }
}

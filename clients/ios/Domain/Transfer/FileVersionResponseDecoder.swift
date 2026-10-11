import Foundation

struct FileVersionMetadata: Equatable, Sendable {
    let id: FileVersionId
    let nodeId: NodeId
    let expected: DownloadExpectedContent
    let createdAt: Date
    let isCurrent: Bool
}

private struct FileVersionEnvelope: Decodable {
    struct Resource: Decodable {
        struct Attributes: Decodable {
            let nodeId: String
            let createdAt: String
            let byteLength: String
            let sha256: String
            let isCurrent: Bool
            enum CodingKeys: String, CodingKey {
                case nodeId = "node_id", createdAt = "created_at", byteLength = "byte_length"
                case sha256, isCurrent = "is_current"
            }
        }
        let id: String
        let type: String
        let attributes: Attributes
    }
    let data: Resource
    let meta: LibraryMetadataDTO
}

struct FileVersionResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol
    func decode(_ response: HTTPTransportResponse, version: FileVersionId, node: NodeId)
        async throws
        -> FileVersionMetadata
    {
        do {
            try await AuthenticatedLibraryCatalogRepository.validateHTTP(response)
            let root = try JSONSerialization.jsonObject(with: response.body)
            try LibraryWireValidation.keys(root, required: ["data", "meta"])
            let object = root as? [String: Any]
            try LibraryWireValidation.keys(object?["meta"], required: ["request_id"])
            let resource = object?["data"] as? [String: Any]
            try LibraryWireValidation.keys(resource, required: ["id", "type", "attributes"])
            try LibraryWireValidation.keys(
                resource?["attributes"],
                required: ["node_id", "created_at", "byte_length", "sha256", "is_current"])
            let envelope = try JSONDecoder().decode(FileVersionEnvelope.self, from: response.body)
            guard envelope.data.type == "file_version",
                LibraryWireValidation.validRequestId(envelope.meta.requestId),
                response.headers.first(where: { $0.key.lowercased() == "x-request-id" })
                    .map({ $0.value == envelope.meta.requestId }) ?? true
            else { throw DownloadFailure.invalidMetadata }
            let attributes = envelope.data.attributes
            let id = try await FileVersionId.validated(envelope.data.id, using: bridge)
            let owner = try await NodeId.validated(attributes.nodeId, using: bridge)
            guard id == version, owner == node else { throw DownloadFailure.invalidMetadata }
            return FileVersionMetadata(
                id: id, nodeId: owner,
                expected: try DownloadExpectedContent(
                    length: attributes.byteLength, sha256: attributes.sha256),
                createdAt: try LibraryWireValidation.timestamp(attributes.createdAt),
                isCurrent: attributes.isCurrent)
        } catch let failure as DownloadFailure { throw failure } catch let failure as LibraryFailure
        {
            throw DownloadPolicy.classify(failure)
        } catch { throw DownloadFailure.invalidMetadata }
    }
}

struct DownloadResponseMetadata: Equatable, Sendable {
    let length: UInt64
    let notModified: Bool
    static func validate(
        status: Int, headers: [String: String], expected: DownloadExpectedContent,
        range: DownloadRange?, revalidating: Bool = false
    ) throws -> Self {
        // Case-fold once and reject ambiguous duplicate header names.
        var fields: [String: String] = [:]
        for (key, value) in headers {
            guard fields.updateValue(value, forKey: key.lowercased()) == nil,
                !key.unicodeScalars.contains(where: { $0.value == 13 || $0.value == 10 }),
                !value.unicodeScalars.contains(where: { $0.value == 13 || $0.value == 10 })
            else { throw DownloadFailure.unexpectedStatus }
        }
        if status == 401 { throw DownloadFailure.unauthenticated }
        if status == 403 { throw DownloadFailure.deviceRevoked }
        if status == 404 { throw DownloadFailure.sourceUnavailable }
        if status >= 500 { throw DownloadFailure.serverUnavailable }
        if (300...399).contains(status), status != 304 { throw DownloadFailure.redirectRejected }
        if status == 416 {
            guard range != nil, fields["content-range"] == "bytes */\(expected.byteLength)" else {
                throw DownloadFailure.unexpectedStatus
            }
            throw DownloadFailure.invalidRange
        }
        guard fields["etag"] == expected.etag else { throw DownloadFailure.etagMismatch }
        if status == 304 {
            guard revalidating, range == nil, fields["content-range"] == nil,
                fields["content-length"] == nil || fields["content-length"] == "0"
            else { throw DownloadFailure.unexpectedStatus }
            return Self(length: 0, notModified: true)
        }
        guard !revalidating, status == (range == nil ? 200 : 206) else {
            throw range == nil ? DownloadFailure.unexpectedStatus : DownloadFailure.invalidRange
        }
        guard fields["content-type"]?.lowercased() == "application/octet-stream",
            fields["content-encoding"] == nil || fields["content-encoding"] == "identity",
            fields["transfer-encoding"] == nil,
            fields["accept-ranges"] == nil || fields["accept-ranges"] == "bytes",
            fields["cache-control"]?.lowercased() == "private, no-store"
        else { throw DownloadFailure.unexpectedStatus }
        // Content-Disposition is deliberately ignored, never a filesystem input.
        guard let raw = fields["content-length"], let length = DownloadPolicy.decimal(raw),
            length == (range?.length ?? expected.byteLength)
        else { throw DownloadFailure.invalidContentLength }
        if let range {
            guard
                fields["content-range"]
                    == "bytes \(range.start)-\(range.end)/\(expected.byteLength)"
            else { throw DownloadFailure.invalidRange }
        } else if fields["content-range"] != nil {
            throw DownloadFailure.invalidRange
        }
        return Self(length: length, notModified: false)
    }
}

protocol FileContentTransportProtocol: Sendable {
    func stream(
        _ request: HTTPTransportRequest, expected: DownloadExpectedContent, range: DownloadRange?,
        revalidating: Bool, receive: @escaping @Sendable (Data) async throws -> Void
    ) async throws
}

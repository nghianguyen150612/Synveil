import Foundation

/// Outcome of checking a locally validated DeviceBearer against the server.
public enum AuthenticatedSessionValidationResult: Equatable, Sendable {
    case authorized
    case authenticationRejected
    case deviceRevoked
    case serverUnavailable
    case dnsFailure
    case networkFailure
    case timeout
    case tlsFailure
    case unexpectedContentType
    case unexpectedProtocolResponse
    case redirectRejected
    case cancelled
}

/// Read-only authorization check for a restored device session.
public protocol AuthenticatedSessionValidationProtocol: Sendable {
    func validateAuthorization(
        for session: DeviceCredentialSession
    ) async -> AuthenticatedSessionValidationResult
}

/// Validates DeviceBearer authorization through the implemented libraries API.
///
/// The bounded `GET /api/v1/libraries?limit=1` response is inspected only to
/// establish that the documented operation returned a valid response. Library
/// data is not retained or exposed by this service.
public final class AuthenticatedSessionValidationService:
    AuthenticatedSessionValidationProtocol, Sendable
{
    private static let responseBodyLimit = 16 * 1024
    private static let opaqueRequestIdRegex = try! NSRegularExpression(
        pattern: "^[A-Za-z0-9._~-]{8,128}$"
    )
    private static let errorCodeRegex = try! NSRegularExpression(
        pattern: "^[a-z][a-z0-9_]{1,63}$"
    )
    private static let revisionRegex = try! NSRegularExpression(pattern: "^(0|[1-9][0-9]*)$")

    private let transport: HTTPTransportProtocol

    public init(transport: HTTPTransportProtocol) {
        self.transport = transport
    }

    public func validateAuthorization(
        for session: DeviceCredentialSession
    ) async -> AuthenticatedSessionValidationResult {
        guard let url = makeLibrariesURL(for: session.serverEndpoint) else {
            return .unexpectedProtocolResponse
        }

        let request = HTTPTransportRequest(
            url: url,
            method: .get,
            headers: [
                "Authorization": "Bearer \(session.record.credential.rawValue)",
                "Accept": "application/json",
            ]
        )

        let response: HTTPTransportResponse
        do {
            response = try await transport.send(request)
        } catch let error as SynveilTransportError {
            return mapTransportError(error)
        } catch is CancellationError {
            return .cancelled
        } catch {
            return Task.isCancelled ? .cancelled : .networkFailure
        }

        if Task.isCancelled {
            return .cancelled
        }
        guard response.body.count <= Self.responseBodyLimit else {
            return .unexpectedProtocolResponse
        }
        if (300...399).contains(response.statusCode) {
            return .redirectRejected
        }
        guard isJSONContentType(response.headers) else {
            return .unexpectedContentType
        }

        switch response.statusCode {
        case 200:
            return isValidLibraryCollection(response.body)
                ? .authorized
                : .unexpectedProtocolResponse
        case 401:
            guard let code = validErrorCode(response.body) else {
                return .unexpectedProtocolResponse
            }
            switch code {
            case "authentication_failed":
                return .authenticationRejected
            case "device_revoked":
                return .deviceRevoked
            default:
                return .unexpectedProtocolResponse
            }
        case 503:
            guard validErrorCode(response.body) != nil else {
                return .unexpectedProtocolResponse
            }
            return .serverUnavailable
        default:
            return .unexpectedProtocolResponse
        }
    }

    private func makeLibrariesURL(for endpoint: ServerEndpoint) -> URL? {
        guard
            var components = URLComponents(
                url: endpoint.url,
                resolvingAgainstBaseURL: false
            )
        else {
            return nil
        }

        let basePath = components.percentEncodedPath
        let normalizedBasePath =
            basePath == "/"
            ? ""
            : basePath.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let prefix = normalizedBasePath.isEmpty ? "" : "/\(normalizedBasePath)"
        components.percentEncodedPath = "\(prefix)/api/v1/libraries"
        components.queryItems = [URLQueryItem(name: "limit", value: "1")]
        components.fragment = nil

        guard
            let url = components.url,
            url.scheme?.lowercased() == endpoint.scheme,
            url.host?.lowercased() == endpoint.host,
            url.port == endpoint.port
        else {
            return nil
        }
        return url
    }

    private func isJSONContentType(_ headers: [String: String]) -> Bool {
        guard let rawValue = header("content-type", in: headers) else {
            return false
        }
        return rawValue.components(separatedBy: ";").first?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased() == "application/json"
    }

    private func validErrorCode(_ body: Data) -> String? {
        guard
            let object = try? JSONSerialization.jsonObject(with: body),
            let root = object as? [String: Any],
            hasKeys(root, required: ["error"]),
            let error = root["error"] as? [String: Any],
            hasKeys(
                error,
                required: ["code", "message", "request_id", "retryable"],
                optional: ["details"]
            ),
            error["details"] == nil || error["details"] is [String: Any],
            let envelope = try? JSONDecoder().decode(ErrorEnvelope.self, from: body),
            envelope.error.message.utf8.count > 0,
            envelope.error.message.utf8.count <= 512,
            isValid(envelope.error.code, regex: Self.errorCodeRegex),
            isValid(envelope.error.requestId, regex: Self.opaqueRequestIdRegex)
        else {
            return nil
        }
        return envelope.error.code
    }

    private func isValidLibraryCollection(_ body: Data) -> Bool {
        guard
            let object = try? JSONSerialization.jsonObject(with: body),
            let root = object as? [String: Any],
            hasKeys(root, required: ["data", "page", "meta"]),
            let items = root["data"] as? [[String: Any]],
            items.count <= 1,
            let page = root["page"] as? [String: Any],
            hasKeys(page, required: ["has_more"], optional: ["next_cursor"]),
            let metadata = root["meta"] as? [String: Any],
            hasKeys(metadata, required: ["request_id"]),
            items.allSatisfy(isExactLibraryShape),
            let response = try? JSONDecoder().decode(LibraryCollectionResponse.self, from: body),
            response.data.count <= 1,
            isValid(response.meta.requestId, regex: Self.opaqueRequestIdRegex),
            response.data.allSatisfy(isValidLibrary)
        else {
            return false
        }

        if let nextCursor = response.page.nextCursor, nextCursor.utf8.count > 512 {
            return false
        }
        return true
    }

    private func isValidLibrary(_ library: LibraryResource) -> Bool {
        guard
            library.type == "library",
            isCanonicalOpaqueId(library.id),
            isValid(library.revision, regex: Self.revisionRegex),
            !library.attributes.name.isEmpty,
            library.attributes.name.utf8.count <= 1024,
            isCanonicalOpaqueId(library.attributes.rootNodeId),
            ["ACTIVE", "READ_ONLY", "QUARANTINED"].contains(library.attributes.status),
            isISO8601DateTime(library.attributes.createdAt),
            isISO8601DateTime(library.attributes.updatedAt)
        else {
            return false
        }

        return true
    }

    private func isExactLibraryShape(_ resource: [String: Any]) -> Bool {
        guard
            hasKeys(resource, required: ["id", "type", "revision", "attributes"]),
            let attributes = resource["attributes"] as? [String: Any]
        else {
            return false
        }
        return hasKeys(
            attributes,
            required: ["name", "root_node_id", "status", "created_at", "updated_at"]
        )
    }

    private func isISO8601DateTime(_ value: String) -> Bool {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if formatter.date(from: value) != nil {
            return true
        }
        formatter.formatOptions = [.withInternetDateTime]
        return formatter.date(from: value) != nil
    }

    private func isCanonicalOpaqueId(_ value: String) -> Bool {
        guard let uuid = UUID(uuidString: value) else {
            return false
        }
        return uuid.uuidString.lowercased() == value
    }

    private func hasKeys(
        _ value: [String: Any],
        required: Set<String>,
        optional: Set<String> = []
    ) -> Bool {
        let keys = Set(value.keys)
        return required.isSubset(of: keys) && keys.isSubset(of: required.union(optional))
    }

    private func header(_ name: String, in headers: [String: String]) -> String? {
        headers.first { $0.key.lowercased() == name }?.value
    }

    private func isValid(_ value: String, regex: NSRegularExpression) -> Bool {
        let range = NSRange(location: 0, length: value.utf16.count)
        return regex.firstMatch(in: value, options: [], range: range) != nil
    }

    private func mapTransportError(
        _ error: SynveilTransportError
    ) -> AuthenticatedSessionValidationResult {
        switch error {
        case .offline:
            return .networkFailure
        case .dnsFailure:
            return .dnsFailure
        case .timeout:
            return .timeout
        case .tlsError:
            return .tlsFailure
        case .redirectRejected:
            return .redirectRejected
        case .unexpectedContentType:
            return .unexpectedContentType
        case .cancelled:
            return .cancelled
        case .httpError, .bodyLimitExceeded, .malformedResponse, .protocolError,
            .configurationError:
            return .unexpectedProtocolResponse
        }
    }

    private struct ErrorEnvelope: Decodable {
        let error: ErrorPayload
    }

    private struct ErrorPayload: Decodable {
        let code: String
        let message: String
        let requestId: String
        let retryable: Bool

        enum CodingKeys: String, CodingKey {
            case code
            case message
            case requestId = "request_id"
            case retryable
        }
    }

    private struct LibraryCollectionResponse: Decodable {
        let data: [LibraryResource]
        let page: LibraryPage
        let meta: ResponseMetadata
    }

    private struct LibraryPage: Decodable {
        let hasMore: Bool
        let nextCursor: String?

        enum CodingKeys: String, CodingKey {
            case hasMore = "has_more"
            case nextCursor = "next_cursor"
        }
    }

    private struct LibraryResource: Decodable {
        let id: String
        let type: String
        let revision: String
        let attributes: LibraryAttributes
    }

    private struct LibraryAttributes: Decodable {
        let name: String
        let rootNodeId: String
        let status: String
        let createdAt: String
        let updatedAt: String

        enum CodingKeys: String, CodingKey {
            case name
            case rootNodeId = "root_node_id"
            case status
            case createdAt = "created_at"
            case updatedAt = "updated_at"
        }
    }

    private struct ResponseMetadata: Decodable {
        let requestId: String

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
        }
    }
}

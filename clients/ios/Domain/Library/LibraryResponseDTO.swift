import Foundation

/// Wire-only DTOs. Unknown keys are checked against OpenAPI before Codable decoding.
struct LibraryCollectionResponseDTO: Codable {
    let data: [LibraryResourceDTO]
    let page: LibraryPageDTO
    let meta: LibraryMetadataDTO
}

struct LibraryResourceDTO: Codable {
    let id: String
    let type: String
    let revision: String
    let attributes: LibraryAttributesDTO
}

struct LibraryAttributesDTO: Codable {
    let name: String
    let rootNodeId: String
    let status: LibraryStatus
    let createdAt: String
    let updatedAt: String

    enum CodingKeys: String, CodingKey {
        case name, status
        case rootNodeId = "root_node_id"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
    }
}

struct LibraryPageDTO: Codable {
    let hasMore: Bool
    let nextCursor: String?

    enum CodingKeys: String, CodingKey {
        case hasMore = "has_more"
        case nextCursor = "next_cursor"
    }
}

struct LibraryMetadataDTO: Codable {
    let requestId: String

    enum CodingKeys: String, CodingKey {
        case requestId = "request_id"
    }
}

struct LibraryErrorEnvelopeDTO: Decodable {
    let error: LibraryErrorDTO
}

struct LibraryErrorDTO: Decodable {
    let code: String
    let message: String
    let requestId: String
    let retryable: Bool

    enum CodingKeys: String, CodingKey {
        case code, message, retryable
        case requestId = "request_id"
    }
}

struct LibraryResponseDecoder: Sendable {
    let bridge: any RustBridgeProtocol

    func decode(_ body: Data) async throws -> LibraryCollectionPage {
        guard body.count <= LibraryCatalogPolicy.maximumResponseBytes else {
            throw LibraryFailure.resourceLimit
        }
        do {
            let root = try JSONSerialization.jsonObject(with: body)
            try LibraryWireValidation.keys(root, required: ["data", "page", "meta"])
            guard let object = root as? [String: Any],
                let items = object["data"] as? [[String: Any]]
            else {
                throw LibraryFailure.protocolFailure
            }
            guard items.count <= LibraryCatalogPolicy.pageSize else {
                throw LibraryFailure.resourceLimit
            }
            try LibraryWireValidation.keys(
                object["page"], required: ["has_more"], optional: ["next_cursor"])
            if let page = object["page"] as? [String: Any], let cursor = page["next_cursor"],
                !(cursor is String)
            {
                throw LibraryFailure.protocolFailure
            }
            try LibraryWireValidation.keys(object["meta"], required: ["request_id"])
            for item in items {
                try LibraryWireValidation.keys(
                    item, required: ["id", "type", "revision", "attributes"])
                try LibraryWireValidation.keys(
                    item["attributes"],
                    required: ["name", "root_node_id", "status", "created_at", "updated_at"])
            }
            let dto = try JSONDecoder().decode(LibraryCollectionResponseDTO.self, from: body)
            guard LibraryWireValidation.validRequestId(dto.meta.requestId) else {
                throw LibraryFailure.protocolFailure
            }
            if dto.page.hasMore {
                guard let cursor = dto.page.nextCursor, LibraryWireValidation.validCursor(cursor)
                else {
                    throw LibraryFailure.protocolFailure
                }
            } else if dto.page.nextCursor != nil {
                throw LibraryFailure.protocolFailure
            }
            var libraries: [Library] = []
            var ids: Set<LibraryId> = []
            for item in dto.data {
                try Task.checkCancellation()
                guard item.type == "library" else { throw LibraryFailure.protocolFailure }
                let id = try await LibraryId.validated(item.id, using: bridge)
                guard ids.insert(id).inserted else { throw LibraryFailure.protocolFailure }
                let rootNode = try await NodeId.validated(item.attributes.rootNodeId, using: bridge)
                guard try await bridge.validateLogicalName(item.attributes.name) else {
                    throw LibraryFailure.protocolFailure
                }
                libraries.append(
                    Library(
                        id: id,
                        revision: try LibraryRevision(validating: item.revision),
                        name: item.attributes.name,
                        rootNodeId: rootNode,
                        status: item.attributes.status,
                        createdAt: try LibraryWireValidation.timestamp(item.attributes.createdAt),
                        updatedAt: try LibraryWireValidation.timestamp(item.attributes.updatedAt)
                    ))
            }
            try Task.checkCancellation()
            return LibraryCollectionPage(
                libraries: libraries, hasMore: dto.page.hasMore, nextCursor: dto.page.nextCursor,
                requestId: dto.meta.requestId)
        } catch is CancellationError {
            throw LibraryFailure.cancelled
        } catch let failure as LibraryFailure {
            throw failure
        } catch {
            throw LibraryFailure.protocolFailure
        }
    }

    static func errorCode(_ body: Data) throws -> String {
        do {
            let root = try JSONSerialization.jsonObject(with: body)
            try LibraryWireValidation.keys(root, required: ["error"])
            let error = (root as? [String: Any])?["error"]
            try LibraryWireValidation.keys(
                error, required: ["code", "message", "request_id", "retryable"],
                optional: ["details"])
            if let details = (error as? [String: Any])?["details"], !(details is [String: Any]) {
                throw LibraryFailure.protocolFailure
            }
            let dto = try JSONDecoder().decode(LibraryErrorEnvelopeDTO.self, from: body).error
            guard LibraryWireValidation.matches(dto.code, pattern: "^[a-z][a-z0-9_]{1,63}$"),
                LibraryWireValidation.validRequestId(dto.requestId),
                !dto.message.isEmpty, dto.message.utf8.count <= 512
            else { throw LibraryFailure.protocolFailure }
            return dto.code
        } catch {
            throw LibraryFailure.protocolFailure
        }
    }
}

/// Validation helpers contain no transport or session state.
enum LibraryWireValidation {
    static func keys(_ value: Any?, required: Set<String>, optional: Set<String> = []) throws {
        guard let object = value as? [String: Any] else { throw LibraryFailure.protocolFailure }
        let keys = Set(object.keys)
        guard required.isSubset(of: keys), keys.isSubset(of: required.union(optional)) else {
            throw LibraryFailure.protocolFailure
        }
    }

    static func matches(_ value: String, pattern: String) -> Bool {
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return false }
        let range = NSRange(location: 0, length: value.utf16.count)
        return regex.firstMatch(in: value, range: range)?.range == range
    }

    static func validRequestId(_ value: String) -> Bool {
        matches(value, pattern: "^[A-Za-z0-9._~-]{8,128}$")
    }

    static func validCursor(_ value: String) -> Bool {
        // Unicode scalar count follows JSON Schema maxLength; never trim or interpret cursor text.
        !value.isEmpty && value.unicodeScalars.count <= LibraryCatalogPolicy.maximumCursorLength
    }

    static func timestamp(_ value: String) throws -> Date {
        guard
            matches(
                value,
                pattern:
                    "^[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]+)?([Zz]|[+-]([01][0-9]|2[0-3]):[0-5][0-9])$"
            )
        else {
            throw LibraryFailure.protocolFailure
        }
        // Reject calendar overflow that ISO8601DateFormatter can otherwise normalize.
        let civil = String(value.prefix(19)).uppercased()
        let strict = DateFormatter()
        strict.locale = Locale(identifier: "en_US_POSIX")
        strict.calendar = Calendar(identifier: .gregorian)
        strict.timeZone = TimeZone(secondsFromGMT: 0)
        strict.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
        strict.isLenient = false
        guard let date = strict.date(from: civil), strict.string(from: date) == civil else {
            throw LibraryFailure.protocolFailure
        }
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let normalized = value.uppercased()
        if let date = formatter.date(from: normalized) { return date }
        formatter.formatOptions = [.withInternetDateTime]
        guard let date = formatter.date(from: normalized) else {
            throw LibraryFailure.protocolFailure
        }
        return date
    }
}

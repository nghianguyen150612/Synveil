import Foundation

public struct LibraryId: Hashable, Sendable {
    public let rawValue: String

    private init(_ value: String) { rawValue = value }

    public static func validated(_ value: String, using bridge: any RustBridgeProtocol) async throws
        -> Self
    {
        guard try await bridge.validateLibraryID(value) else {
            throw LibraryFailure.protocolFailure
        }
        return Self(value)
    }
}

public struct NodeId: Hashable, Sendable {
    public let rawValue: String

    private init(_ value: String) { rawValue = value }

    public static func validated(_ value: String, using bridge: any RustBridgeProtocol) async throws
        -> Self
    {
        guard try await bridge.validateNodeID(value) else { throw LibraryFailure.protocolFailure }
        return Self(value)
    }
}

/// Exact canonical decimal text; no floating point conversion or fixed-width overflow.
public struct LibraryRevision: Hashable, Sendable {
    public let rawValue: String

    public init(validating value: String) throws {
        guard LibraryWireValidation.matches(value, pattern: "^(0|[1-9][0-9]*)$") else {
            throw LibraryFailure.protocolFailure
        }
        rawValue = value
    }
}

public enum LibraryStatus: String, Codable, Sendable {
    case active = "ACTIVE"
    case readOnly = "READ_ONLY"
    case quarantined = "QUARANTINED"
}

public struct Library: Equatable, Sendable {
    public let id: LibraryId
    public let revision: LibraryRevision
    public let name: String
    public let rootNodeId: NodeId
    public let status: LibraryStatus
    public let createdAt: Date
    public let updatedAt: Date
}

public struct LibraryCollectionPage: Equatable, Sendable {
    public let libraries: [Library]
    public let hasMore: Bool
    public let nextCursor: String?
    public let requestId: String
}

/// Failures never contain server messages, response bytes, URLs, or bearer values.
public enum LibraryFailure: Error, Equatable, Sendable {
    case unauthenticated
    case credentialUnavailable
    case invalidCredential
    case originMismatch
    case staleSession
    case authenticationRejected
    case deviceRevoked
    case offline
    case dnsFailure
    case timeout
    case tlsFailure
    case redirectRejected
    case serverUnavailable
    case httpFailure(statusCode: Int)
    case unexpectedContentType
    case protocolFailure
    case resourceLimit
    case repeatedCursor
    case cancelled
}

public enum LibraryRepositoryResult: Equatable, Sendable {
    case loaded([Library])
    case failed(LibraryFailure)
}

@MainActor
public protocol LibraryCatalogRepositoryProtocol {
    func listLibraries() async -> LibraryRepositoryResult
}

public enum LibraryCatalogPolicy {
    public static let pageSize = 100
    public static let maximumPages = 64
    public static let maximumLibraries = 4096
    public static let maximumCursorLength = 512
    public static let maximumResponseBytes = 1024 * 1024
}

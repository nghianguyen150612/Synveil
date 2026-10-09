import Foundation

/// Node reads share the established authenticated-read failure vocabulary and recovery boundary.
public typealias NodeFailure = LibraryFailure

public struct NodeRevision: Hashable, Sendable {
    public let rawValue: String

    public init(validating value: String) throws {
        rawValue = try LibraryRevision(validating: value).rawValue
    }
}

/// A file-version identity is never interchangeable with a NodeId.
public struct FileVersionId: Hashable, Sendable {
    public let rawValue: String

    private init(_ value: String) { rawValue = value }

    public static func validated(_ value: String, using bridge: any RustBridgeProtocol) async throws
        -> Self
    {
        // Rust's domain_id! macro uses the same canonical UUIDv7 parser for NodeId and
        // FileVersionId. Reuse that syntax validator without constructing/casting a NodeId.
        guard try await bridge.validateNodeID(value) else { throw NodeFailure.protocolFailure }
        return Self(value)
    }
}

public enum NodeKind: String, Codable, Sendable {
    case file = "FILE"
    case directory = "DIRECTORY"
}

public enum NodeState: String, Codable, Sendable {
    case active = "ACTIVE"
    case trashed = "TRASHED"
    case purging = "PURGING"
}

public struct Node: Equatable, Sendable {
    public let id: NodeId
    public let libraryId: LibraryId
    public let parentId: NodeId?
    public let currentVersionId: FileVersionId?
    public let revision: NodeRevision
    public let name: String
    public let kind: NodeKind
    public let state: NodeState
    public let createdAt: Date
    public let updatedAt: Date
    public let trashedAt: Date?
    public let restoreDeadline: Date?
    public let purgeEligible: Bool
}

/// Root metadata validates returned parentage; it is never sent as the root query's parent_id.
public enum NodeParentScope: Equatable, Sendable {
    case libraryRoot(rootNodeId: NodeId)
    case directory(NodeId)

    var expectedParentId: NodeId {
        switch self {
        case .libraryRoot(let id), .directory(let id): return id
        }
    }

    var queryParentId: NodeId? {
        switch self {
        case .libraryRoot: return nil
        case .directory(let id): return id
        }
    }
}

public struct NodeCollectionPage: Equatable, Sendable {
    public let nodes: [Node]
    public let hasMore: Bool
    public let nextCursor: String?
    public let requestId: String
}

public enum NodeRepositoryResult: Equatable, Sendable {
    case loaded([Node])
    case failed(NodeFailure)
}

@MainActor
public protocol NodeRepositoryProtocol {
    func listChildren(libraryId: LibraryId, parent: NodeParentScope) async -> NodeRepositoryResult
}

public enum NodeBrowserPolicy {
    public static let pageSize = 100
    public static let maximumPages = 64
    public static let maximumNodes = 4096
    public static let maximumCursorLength = 512
    public static let maximumResponseBytes = 1024 * 1024
}

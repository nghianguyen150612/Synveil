import Foundation

enum NodeProjectionCompleteness: String, Sendable {
    case uninitialized = "UNINITIALIZED", partial = "PARTIAL", complete = "COMPLETE"
    case rebaselineRequired = "REBASELINE_REQUIRED"
}

enum CachedNodeLifecycle: String, Sendable {
    case active = "ACTIVE", trashed = "TRASHED", purged = "PURGED"
}
enum CachedNodeProvenance: String, Sendable {
    case canonical = "CANONICAL", event = "EVENT", lastKnown = "LAST_KNOWN"
    case snapshotManifest = "SNAPSHOT_MANIFEST"
}

/// Only canonical, complete metadata at the recorded revision can be an ordinary cached item.
struct CachedNodeRecord: Equatable, Sendable {
    let scope: ClientMutationScope
    let position: SyncJournalPosition
    let id: NodeId
    let revision: ClientMutationDecimal
    let lifecycle: CachedNodeLifecycle
    let provenance: CachedNodeProvenance
    let metadata: Node?
    var snapshotMetadata: RebaselineSnapshotNode? = nil
    var completeNode: Node? {
        guard provenance == .canonical, lifecycle == .active,
            let metadata, metadata.state == .active,
            metadata.revision.rawValue == revision.rawValue
        else { return nil }
        return metadata
    }
}

struct CachedLibraryRecord: Equatable, Sendable {
    let scope: ClientMutationScope
    let epoch: ClientMutationDecimal
    let library: Library
}

struct NodeProjectionState: Equatable, Sendable {
    let completeness: NodeProjectionCompleteness
    /// The verified server cursor at initialization is not evidence of a complete tree.
    let incrementalAnchor: SyncJournalPosition?
    let locallyApplied: SyncJournalPosition?
    let serverConfirmed: SyncJournalPosition?
    let library: CachedLibraryRecord?
}

enum CachedNodeResult: Equatable, Sendable {
    case found(CachedNodeRecord, NodeProjectionState), missing(NodeProjectionState), unavailable(
        SyncProjectionFailure)
}

enum CachedDirectoryKnowledge: Equatable, Sendable { case partial, missing, staleKnown, complete }
struct CachedChildren: Equatable, Sendable {
    let nodes: [Node]
    let knowledge: CachedDirectoryKnowledge
    let hasMore: Bool
    let projection: NodeProjectionState
    var snapshotNodes: [RebaselineSnapshotNode] = []
}
enum CachedChildrenResult: Equatable, Sendable {
    case loaded(CachedChildren), unavailable(SyncProjectionFailure)
}
enum NodeProjectionStateResult: Equatable, Sendable {
    case loaded(NodeProjectionState), unavailable(SyncProjectionFailure)
}

enum SyncProjectionFailure: Error, Equatable, Sendable {
    case malformedMetadata, scopeMismatch, staleSession, missingMaterialization
    case revisionRegression, reconciliationRequired, invalidParent, conflictingEvent, stalePage
    case storageCapacity, cancelled, storage(MutationQueueFailure), transport(LibraryFailure)
}

enum SyncFeedApplicationResult: Equatable, Sendable {
    case applied(InboundSyncPageRecord, NodeProjectionState, existing: Bool)
    case failed(SyncProjectionFailure)
    /// SQLite committed, but the original session no longer has publication authority.
    case committedButSessionChanged
}

@MainActor
protocol NodeProjectionRepositoryProtocol {
    func node(scope: ClientMutationScope, nodeId: NodeId) async -> CachedNodeResult
    func children(scope: ClientMutationScope, parentId: NodeId, limit: Int) async
        -> CachedChildrenResult
    func projectionState(scope: ClientMutationScope) async -> NodeProjectionStateResult
}

enum NodeProjectionPolicy {
    static let maximumLookups = 1000  // at most 500 final subjects and 500 distinct parents
    static let maximumNodesPerPage = 1000
    static let maximumMetadataBytes = 16384
    static let maximumPlanBytes = 2 * 1024 * 1024
    static let maximumChildren = 500
    static let maximumNodes = 16384
    static let maximumEvents = 16384
    static let encodingVersion = 1

    static func classify(_ error: Error) -> SyncProjectionFailure {
        if let failure = error as? SyncProjectionFailure { return failure }
        if error is CancellationError { return .cancelled }
        if let failure = error as? MutationQueueFailure {
            if failure == .capacity || failure == .diskFull { return .storageCapacity }
            if failure == .scopeMismatch { return .scopeMismatch }
            if failure == .staleSession { return .staleSession }
            return .storage(failure)
        }
        if let failure = error as? LibraryFailure { return .transport(failure) }
        return .malformedMetadata
    }
}

/// Versioned private encoding stores available fields only; revision and IDs remain strings.
struct NodeProjectionMetadata: Codable, Sendable {
    let version: Int
    let id: String, library: String
    let parent: String?, fileVersion: String?
    let revision: String, name: String
    let kind: NodeKind, state: NodeState
    let created: Date, updated: Date, trashed: Date?, deadline: Date?
    let purgeEligible: Bool

    init(_ node: Node) {
        version = 1
        id = node.id.rawValue
        library = node.libraryId.rawValue
        parent = node.parentId?.rawValue
        fileVersion = node.currentVersionId?.rawValue
        revision = node.revision.rawValue
        name = node.name
        kind = node.kind
        state = node.state
        created = node.createdAt
        updated = node.updatedAt
        trashed = node.trashedAt
        deadline = node.restoreDeadline
        purgeEligible = node.purgeEligible
    }

    func validated(using bridge: any RustBridgeProtocol) async throws -> Node {
        guard version == NodeProjectionPolicy.encodingVersion,
            try await bridge.validateLogicalName(name), name.utf8.count <= 1024,
            [created, updated].allSatisfy(Self.validDate),
            trashed.map(Self.validDate) ?? true, deadline.map(Self.validDate) ?? true,
            created <= updated, parent != id,
            kind != .directory || fileVersion == nil,
            state != .active || (trashed == nil && deadline == nil && !purgeEligible),
            state == .active || trashed != nil,
            deadline == nil || (trashed != nil && deadline! >= trashed!)
        else { throw SyncProjectionFailure.malformedMetadata }
        _ = try SyncDecimalValidation.validate(revision, nonzero: true)
        let nodeId = try await NodeId.validated(id, using: bridge)
        let libraryId = try await LibraryId.validated(library, using: bridge)
        let parentId: NodeId?
        if let parent {
            parentId = try await NodeId.validated(parent, using: bridge)
        } else {
            parentId = nil
        }
        let versionId: FileVersionId?
        if let fileVersion {
            versionId = try await FileVersionId.validated(fileVersion, using: bridge)
        } else {
            versionId = nil
        }
        return Node(
            id: nodeId, libraryId: libraryId, parentId: parentId, currentVersionId: versionId,
            revision: try NodeRevision(validating: revision), name: name, kind: kind, state: state,
            createdAt: created, updatedAt: updated, trashedAt: trashed, restoreDeadline: deadline,
            purgeEligible: purgeEligible)
    }

    static func validDate(_ date: Date) -> Bool {
        date.timeIntervalSince1970.isFinite
            && (-62_135_596_800...253_402_300_799).contains(date.timeIntervalSince1970)
    }
}

/// Only the bounded validator can construct a prepared immutable Node.
struct PreparedProjectionNode: Sendable {
    let node: Node
    let bytes: Data
    private init(node: Node, bytes: Data) {
        self.node = node
        self.bytes = bytes
    }
    static func validated(_ node: Node, bridge: any RustBridgeProtocol) async throws -> Self {
        let metadata = NodeProjectionMetadata(node)
        let verified = try await metadata.validated(using: bridge)
        guard verified == node else { throw SyncProjectionFailure.malformedMetadata }
        let bytes = try JSONEncoder().encode(metadata)
        guard bytes.count <= NodeProjectionPolicy.maximumMetadataBytes else {
            throw SyncProjectionFailure.storageCapacity
        }
        return Self(node: node, bytes: bytes)
    }
}

struct SyncNodeMaterializationPlan: Sendable {
    let page: SyncFeedPage
    /// Final subject metadata only. Earlier facts remain journal evidence, never fabricated Nodes.
    let nodes: [NodeId: PreparedProjectionNode]
    /// Parent observations validate directory identity; they are not installed as page-cut metadata.
    let parents: [NodeId: PreparedProjectionNode]
}

struct LibraryProjectionMetadata: Codable, Sendable {
    let version: Int
    let id: String, revision: String, name: String, root: String
    let status: LibraryStatus
    let created: Date, updated: Date
    init(_ library: Library) {
        version = 1
        id = library.id.rawValue
        revision = library.revision.rawValue
        name = library.name
        root = library.rootNodeId.rawValue
        status = library.status
        created = library.createdAt
        updated = library.updatedAt
    }
    func validated(bridge: any RustBridgeProtocol) async throws -> Library {
        guard version == 1, NodeProjectionMetadata.validDate(created),
            NodeProjectionMetadata.validDate(updated),
            created <= updated, name.utf8.count <= 1024, try await bridge.validateLogicalName(name)
        else { throw SyncProjectionFailure.malformedMetadata }
        _ = try SyncDecimalValidation.validate(revision)
        return Library(
            id: try await LibraryId.validated(id, using: bridge),
            revision: try LibraryRevision(validating: revision),
            name: name, rootNodeId: try await NodeId.validated(root, using: bridge), status: status,
            createdAt: created, updatedAt: updated)
    }
}

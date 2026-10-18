import Foundation
import XCTest

@testable import Synveil

@MainActor
final class ProjectionNodes: NodeRepositoryProtocol {
    var values: [NodeId: NodeMetadataRepositoryResult] = [:]
    var calls: [NodeId] = []
    var gate: QueueGate?
    func listChildren(libraryId: LibraryId, parent: NodeParentScope) async -> NodeRepositoryResult {
        .loaded([])
    }
    func getNode(libraryId: LibraryId, nodeId: NodeId, expectedParent: NodeParentScope) async
        -> NodeDetailsRepositoryResult
    { await getNodeMetadata(libraryId: libraryId, nodeId: nodeId) }
    func getNodeMetadata(libraryId: LibraryId, nodeId: NodeId) async -> NodeMetadataRepositoryResult
    {
        calls.append(nodeId)
        if let gate {
            self.gate = nil
            await gate.arrive()
        }
        return values[nodeId] ?? .unavailable
    }
}

func projectionNode(
    scope: ClientMutationScope, id: Int = 1, parent: Int? = 2, revision: String = "8",
    name: String = "Exact e\u{301} / 📂", kind: NodeKind = .file, state: NodeState = .active,
    version: Int? = nil
) async throws -> Node {
    let parentId: NodeId?
    if let parent {
        parentId = try await NodeId.validated(queueUUID(parent), using: QueueValidator())
    } else {
        parentId = nil
    }
    let versionId: FileVersionId?
    if let version {
        versionId = try await FileVersionId.validated(queueUUID(version), using: QueueValidator())
    } else {
        versionId = nil
    }
    return Node(
        id: try await NodeId.validated(queueUUID(id), using: QueueValidator()),
        libraryId: scope.libraryId,
        parentId: parentId, currentVersionId: versionId,
        revision: try NodeRevision(validating: revision), name: name, kind: kind, state: state,
        createdAt: Date(timeIntervalSince1970: 1_700_000_000),
        updatedAt: Date(timeIntervalSince1970: 1_700_000_001),
        trashedAt: state == .active ? nil : Date(timeIntervalSince1970: 1_700_000_001),
        restoreDeadline: nil, purgeEligible: false)
}

func projectionEvent(
    _ kind: SyncChangeKind = .nodeRenamed, id: Int = 1, event: Int = 1001, revision: String = "8",
    sequence: String = "1", parent: Int? = 2, nodeKind: NodeKind = .file, version: Int? = nil
) -> [String: Any] {
    var object: [String: Any] = [
        "event_id": queueUUID(event), "schema_version": 1, "sequence": sequence,
        "resource_kind": "NODE", "resource_id": queueUUID(id), "change_kind": kind.rawValue,
        "resource_revision": revision, "occurred_at": "2026-10-09T12:00:00Z",
        "node_kind": nodeKind.rawValue,
    ]
    if kind != .nodePurged {
        if let parent { object["parent_node_id"] = queueUUID(parent) }
        object["node_state"] = kind == .nodeTrashed ? "TRASHED" : "ACTIVE"
        if let version { object["current_version_id"] = queueUUID(version) }
    }
    return object
}

@MainActor
func projectionPage(
    _ f: QueueFixture, events: [[String: Any]]? = nil, from: UInt64 = 0, token: String? = nil
) async throws -> SyncFeedPage {
    let changes = events ?? [projectionEvent()]
    var object = feedObject(scope: f.scope, count: changes.count, from: from)
    var data = object["data"] as! [String: Any]
    data["changes"] = changes
    if let token { data["ack_token"] = token }
    object["data"] = data
    return try await feedPage(scope: f.scope, object: object, from: String(from))
}

@MainActor
func projectionNodes(_ f: QueueFixture, node: Node? = nil) async throws -> ProjectionNodes {
    let repository = ProjectionNodes()
    let subject: Node
    if let node { subject = node } else { subject = try await projectionNode(scope: f.scope) }
    repository.values[subject.id] = .loaded(subject)
    let parent = try await projectionNode(scope: f.scope, id: 2, parent: nil, kind: .directory)
    repository.values[parent.id] = .loaded(parent)
    return repository
}

@MainActor
func projectionApply(
    _ f: QueueFixture, page: SyncFeedPage? = nil, repository: ProjectionNodes? = nil
) async throws -> SyncFeedApplicationResult {
    let resolved: SyncFeedPage
    if let page { resolved = page } else { resolved = try await projectionPage(f) }
    _ = try await f.database.stageFeed(
        resolved, credentialId: queueUUID(92), bridge: QueueValidator())
    let nodes: ProjectionNodes
    if let repository { nodes = repository } else { nodes = try await projectionNodes(f) }
    return await SyncFeedApplicationService(
        provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
        bridge: QueueValidator()
    ).apply(scope: f.scope, position: resolved.start)
}

@MainActor
func projectionCache(_ f: QueueFixture) -> SQLiteNodeProjectionRepository {
    SQLiteNodeProjectionRepository(
        database: f.database, provider: f.provider, bridge: QueueValidator())
}
func projectionStorage(_ f: QueueFixture) -> CommittedSQLiteSyncProjectionStorage {
    CommittedSQLiteSyncProjectionStorage(database: f.database, bridge: QueueValidator())
}
@MainActor
func projectionAck(_ f: QueueFixture) -> SyncAckService {
    SyncAckService(provider: f.provider, projection: projectionStorage(f), bridge: QueueValidator())
}
@MainActor
func projectionConfirm(_ f: QueueFixture, page: SyncFeedPage? = nil) async throws {
    let service = projectionAck(f)
    let receipt = try await service.receipt(scope: f.scope, position: page?.start ?? feedPosition())
    await f.transport.set(
        try queueHTTP(
            queueCheckpointObject(scope: f.scope, sequence: page?.through.rawValue ?? "1")))
    guard case .confirmed = await service.acknowledge(receipt) else {
        throw SyncFeedFailure.protocolFailure
    }
}

/// Remove only empty P039 objects to recreate an exact P038 fixture; no queue/inbox data changes.
func projectionDowngradeV3(_ url: URL) throws {
    try snapshotDowngradeV4(url)
    let statements = NodeProjectionSQLiteSchema.statements
    for sql in statements where sql.contains("CREATE TRIGGER") {
        let name = sql.split(separator: " ")[2]
        try queueRawSQL(url, "DROP TRIGGER \(name)")
    }
    for table in [
        "sync_ack_attempts", "projection_events", "projection_commits", "cached_nodes",
        "cached_libraries", "node_projection_state",
    ] { try queueRawSQL(url, "DROP TABLE \(table)") }
    try queueRawSQL(url, try XCTUnwrap(MutationQueueSQLiteStore.inboundSchema.last))
    try queueRawSQL(url, "PRAGMA user_version=3")
}

func projectionNodeHTTP(_ node: Node) throws -> HTTPTransportResponse {
    let formatter = ISO8601DateFormatter()
    var attributes: [String: Any] = [
        "library_id": node.libraryId.rawValue, "name": node.name,
        "kind": node.kind.rawValue, "state": node.state.rawValue,
        "created_at": formatter.string(from: node.createdAt),
        "updated_at": formatter.string(from: node.updatedAt),
        "purge_eligible": node.purgeEligible,
    ]
    attributes["parent_id"] = node.parentId?.rawValue
    attributes["current_version_id"] = node.currentVersionId?.rawValue
    return try queueHTTP([
        "data": [
            "id": node.id.rawValue, "type": "node", "revision": node.revision.rawValue,
            "attributes": attributes,
        ], "meta": ["request_id": "canonical-node-01"],
    ])
}

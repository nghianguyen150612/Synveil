import Foundation
import XCTest

@testable import Synveil

func snapshotBootstrapObject(
    _ scope: ClientMutationScope, count: Int = 3, generation: String = "1", id: Int = 700
) -> [String: Any] {
    [
        "bootstrap_id": queueUUID(id), "device_id": scope.deviceId.rawValue,
        "library_id": scope.libraryId.rawValue,
        "state": "OPEN", "generation": generation, "snapshot_epoch": "1",
        "snapshot_resume_sequence": "25",
        "manifest_item_count": String(count), "created_at": "2026-10-11T12:00:00Z",
        "expires_at": "2026-10-11T13:00:00Z",
    ]
}
func snapshotEnvelope(_ data: [String: Any]) -> [String: Any] {
    ["data": data, "meta": ["request_id": "snapshot-request-01"]]
}
func snapshotNodeObject(
    _ id: Int, parent: Int? = 1, kind: String = "FILE", state: String = "ACTIVE"
) -> [String: Any] {
    var row: [String: Any] = [
        "node_id": queueUUID(id), "name": "Exact e\u{301} 📂 \(id)", "kind": kind, "state": state,
        "revision": "7",
    ]
    if let parent { row["parent_node_id"] = queueUUID(parent) }
    return row
}
func snapshotRows() -> [[String: Any]] {
    [
        snapshotNodeObject(1, parent: nil, kind: "DIRECTORY"),
        snapshotNodeObject(2, kind: "DIRECTORY"), snapshotNodeObject(3, parent: 2),
    ]
}
func snapshotPageObject(
    _ scope: ClientMutationScope, rows: [[String: Any]], more: Bool = false,
    cursor: String = "opaque.server.cursor+_", bootstrap: [String: Any]? = nil
) -> [String: Any] {
    var data: [String: Any] = [
        "bootstrap": bootstrap ?? snapshotBootstrapObject(scope), "nodes": rows, "has_more": more,
    ]
    if more {
        data["next_cursor"] = cursor
    } else {
        data["completion_token"] = "opaque.original.terminal-evidence_"
    }
    return snapshotEnvelope(data)
}
func snapshotCompletionObject(_ scope: ClientMutationScope, replayed: Bool = false) -> [String: Any]
{
    var bootstrap = snapshotBootstrapObject(scope)
    bootstrap["state"] = "COMPLETED"
    bootstrap["completed_at"] = "2026-10-11T12:30:00Z"
    return snapshotEnvelope([
        "bootstrap": bootstrap,
        "checkpoint": [
            "journal_epoch": "1", "acknowledged_sequence": "25",
            "updated_at": "2026-10-11T12:30:00Z",
        ], "replayed": replayed,
    ])
}
func snapshotLibrary(_ scope: ClientMutationScope, root: Int = 1) async throws -> Library {
    Library(
        id: scope.libraryId, revision: try LibraryRevision(validating: "1"),
        name: "Library e\u{301} 📂",
        rootNodeId: try await NodeId.validated(queueUUID(root), using: QueueValidator()),
        status: .active,
        createdAt: Date(timeIntervalSince1970: 1_700_000_000),
        updatedAt: Date(timeIntervalSince1970: 1_700_000_001))
}
@MainActor
func snapshotCoordinator(_ f: QueueFixture, database: MutationQueueSQLiteStore? = nil)
    -> RebaselineCoordinator
{
    RebaselineCoordinator(
        database: database ?? f.database, queue: f.queue, provider: f.provider,
        bridge: QueueValidator())
}
@MainActor
func snapshotStart(
    _ f: QueueFixture, coordinator: RebaselineCoordinator? = nil, root: Int = 1, count: Int = 3
) async throws {
    let coordinator = coordinator ?? snapshotCoordinator(f)
    await f.transport.set(
        try queueHTTP(snapshotEnvelope(snapshotBootstrapObject(f.scope, count: count))))
    try await coordinator.start(
        scope: f.scope, library: snapshotLibrary(f.scope, root: root), confirmedByUser: true)
}
@MainActor
func snapshotPrepared(_ f: QueueFixture, coordinator: RebaselineCoordinator? = nil) async throws {
    let coordinator = coordinator ?? snapshotCoordinator(f)
    try await snapshotStart(f, coordinator: coordinator)
    await f.transport.setSequence([
        try queueHTTP(
            snapshotPageObject(f.scope, rows: Array(snapshotRows().prefix(2)), more: true)),
        try queueHTTP(snapshotPageObject(f.scope, rows: Array(snapshotRows().suffix(1)))),
    ])
    try await coordinator.download(scope: f.scope)
    try await coordinator.prepare(scope: f.scope)
}
@MainActor
func snapshotAssertFailure<T>(
    _ expected: RebaselineFailure? = nil, operation: () async throws -> T,
    file: StaticString = #filePath, line: UInt = #line
) async {
    do {
        _ = try await operation()
        XCTFail("Expected safe snapshot rejection", file: file, line: line)
    } catch {
        if let expected {
            XCTAssertEqual(RebaselineFailure.classify(error), expected, file: file, line: line)
        }
    }
}

/// Empty-only fixture downgrade preserves all existing v4 queue, inbox and projection evidence.
func snapshotDowngradeV4(_ url: URL) throws {
    for sql in RebaselineSQLiteSchema.statements where sql.contains("CREATE TRIGGER") {
        try queueRawSQL(url, "DROP TRIGGER \(sql.split(separator: " ")[2])")
    }
    for table in [
        "rebaseline_active", "rebaseline_completion_attempts", "rebaseline_nodes",
        "rebaseline_pages", "rebaseline_sessions", "rebaseline_start_attempts",
    ] {
        try queueRawSQL(url, "DROP TABLE \(table)")
    }
    for (old, new) in zip(NodeProjectionSQLiteSchema.statements, RebaselineSQLiteSchema.projection)
    where old != new {
        try queueRawSQL(url, "DROP TRIGGER \(new.split(separator: " ")[2])")
        try queueRawSQL(url, old)
    }
    try queueRawSQL(url, "PRAGMA user_version=4")
}

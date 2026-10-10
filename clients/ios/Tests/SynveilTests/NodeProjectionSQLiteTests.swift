import Foundation
import SQLite3
import XCTest

@testable import Synveil

@MainActor
final class NodeProjectionSQLiteTests: XCTestCase {
    func testBeforeMaterializationRollsBackEntirePage() async throws {
        try await rollback(.beforeMaterialization)
    }
    func testAfterMaterializationRollsBackEntirePage() async throws {
        try await rollback(.afterMaterialization)
    }
    func testAfterFirstNodeWriteRollsBackEntirePage() async throws {
        try await rollback(.afterFirstNodeWrite)
    }
    func testAfterFinalNodeWriteRollsBackEntirePage() async throws {
        try await rollback(.afterFinalNodeWrite)
    }
    func testBeforeProjectionCommitRollsBackEntirePage() async throws {
        try await rollback(.beforeProjectionCommit)
    }
    func testSQLiteCommitFailureRollsBackEntirePage() async throws {
        try await rollback(.beforeCommit)
    }
    private func rollback(_ point: MutationQueueFaultPoint) async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let original = try await queueEnqueued(f)
        let page = try await projectionPage(
            f, events: [projectionEvent(), projectionEvent(id: 3, event: 1002, sequence: "2")])
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let nodes = try await projectionNodes(f)
        let second = try await projectionNode(scope: f.scope, id: 3)
        nodes.values[second.id] = .loaded(second)
        fault.arm(point)
        let result = await SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator()
        ).apply(scope: f.scope, position: page.start)
        XCTAssertEqual(result, .failed(.storageCapacity))
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let stored = try await reopened.inboundPage(
            scope: f.scope, position: page.start, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(stored?.state, .receivedUnapplied)
        for table in [
            "cached_nodes", "projection_events", "projection_commits", "node_projection_state",
            "sync_ack_attempts",
        ] {
            XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM \(table)"), "0")
        }
        let mutation = try await queueRecord(f)
        XCTAssertEqual(mutation, original)
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testLostCommitAcknowledgementReadsBackCommittedEvidence() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        fault.arm(.afterCommit)
        let nodes = try await projectionNodes(f)
        let result = await SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator()
        ).apply(scope: f.scope, position: page.start)
        guard case .applied = result else { return XCTFail("Committed evidence must survive") }
        try await assertCommitted(f)
    }
    func testAfterProjectionCommitBeforeReturnReadsBackCommittedEvidence() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        fault.arm(.afterProjectionCommit)
        let nodes = try await projectionNodes(f)
        let result = await SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator()
        ).apply(scope: f.scope, position: page.start)
        guard case .applied = result else { return XCTFail("Committed evidence must survive") }
        try await assertCommitted(f)
    }
    private func assertCommitted(_ f: QueueFixture) async throws {
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let state = try await reopened.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(state.serverConfirmed?.sequence.rawValue, "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "APPLIED_ACK_PENDING")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_commits"), "1")
    }
    func testFreshSchemaV4() async throws {
        let f = try await queueFixture(self)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "4")
        XCTAssertEqual(
            try queueRawScalar(
                f.url,
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('cached_nodes','cached_libraries','node_projection_state','projection_commits','projection_events','sync_ack_attempts')"
            ), "6")
    }
    func testV3MigrationPreservesRequestsStagingCheckpointAndQuarantine() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let checkpoint = try await f.database.syncBase(scope: f.scope)
        try projectionDowngradeV3(f.url)
        let migrated = try MutationQueueSQLiteStore(url: f.url)
        let record = try await migrated.record(scope: f.scope, id: original.mutation.id.rawValue)
        XCTAssertEqual(record?.request, original.mutation.requestBody)
        XCTAssertEqual(record?.epoch, "1")
        XCTAssertEqual(record?.sequence, "0")
        let staged = try await migrated.inboundPage(
            scope: f.scope, position: page.start, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(staged?.page, page)
        let base = try await migrated.syncBase(scope: f.scope)
        XCTAssertEqual(base?.responseBody, checkpoint?.responseBody)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "4")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "0")
    }
    func testV3MigrationPreservesQuarantinedScope() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionPage(f)
        try await f.database.quarantineSessions()
        try projectionDowngradeV3(f.url)
        _ = try MutationQueueSQLiteStore(url: f.url)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
    }
    func testV3MigrationRollbackRetainsOriginalGateAndRows() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        try projectionDowngradeV3(f.url)
        XCTAssertThrowsError(
            try MutationQueueSQLiteStore(
                url: f.url, fault: { if $0 == .migration { throw MutationQueueFailure.diskFull } }))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "3")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "1")
        XCTAssertEqual(
            try queueRawScalar(
                f.url, "SELECT count(*) FROM sqlite_master WHERE name='cached_nodes'"), "0")
        XCTAssertThrowsError(
            try queueRawSQL(f.url, "UPDATE inbound_pages SET state='APPLIED_ACK_PENDING'"))
    }
    func testCorruptedV3SchemaRejectedBeforeMigration() async throws {
        let f = try await queueFixture(self)
        try projectionDowngradeV3(f.url)
        try queueRawSQL(f.url, "DROP TRIGGER inbound_immutable")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "3")
    }
    func testUnsupportedFutureSchemaFailsClosed() async throws {
        let f = try await queueFixture(self)
        try queueRawSQL(f.url, "PRAGMA user_version=5")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url))
    }
    func testStandaloneAppliedStateSetterRejected() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        for state in ["APPLIED_ACK_PENDING", "ACK_IN_FLIGHT", "ACK_CONFIRMED"] {
            XCTAssertThrowsError(
                try queueRawSQL(f.url, "UPDATE inbound_pages SET state='\(state)'"))
        }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testStandaloneNodeWriteRejected() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        XCTAssertThrowsError(try queueRawSQL(f.url, "UPDATE cached_nodes SET revision='1'"))
        XCTAssertThrowsError(try queueRawSQL(f.url, "DELETE FROM projection_commits"))
        XCTAssertThrowsError(
            try queueRawSQL(f.url, "UPDATE node_projection_state SET applied_sequence='99'"))
    }
    func testCommittedNodesAndPendingAckSurviveReopen() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        try await assertCommitted(f)
    }
    func testReopenInitiatesNoNetwork() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let before = await f.transport.requests().count
        _ = try MutationQueueSQLiteStore(url: f.url)
        let after = await f.transport.requests().count
        XCTAssertEqual(before, after)
    }
    func testDuplicateValidApplicationIsIdempotent() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let nodes = ProjectionNodes()
        let result = await SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator()
        ).apply(scope: f.scope, position: try feedPosition())
        guard case .applied(_, _, let existing) = result else { return XCTFail() }
        XCTAssertTrue(existing)
        XCTAssertTrue(nodes.calls.isEmpty)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_events"), "1")
    }
    func testConcurrentApplicationOfSamePageHasOneCommit() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let nodes = try await projectionNodes(f)
        let service = SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator())
        async let first = service.apply(scope: f.scope, position: page.start)
        async let second = service.apply(scope: f.scope, position: page.start)
        let results = await [first, second]
        for result in results { guard case .applied = result else { return XCTFail() } }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_commits"), "1")
    }
    func testCancellationBeforeTransactionLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let gate = QueueGate()
        let nodes = try await projectionNodes(f)
        nodes.gate = gate
        let service = SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator())
        let task = Task { await service.apply(scope: f.scope, position: page.start) }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.cancelled))
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testLogoutDuringMaterializationDoesNotApplyOldResponse() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let gate = QueueGate()
        let nodes = try await projectionNodes(f)
        nodes.gate = gate
        let service = SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator())
        let task = Task { await service.apply(scope: f.scope, position: page.start) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        guard case .failed = await task.value else { return XCTFail() }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testWrongCredentialRejected() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        await queueAssertFailure(.scopeMismatch) {
            try await f.database.cachedProjectionState(
                scope: f.scope, credentialId: queueUUID(93), bridge: QueueValidator())
        }
    }
    func testLogoutInvalidatesProjectionAccess() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        await f.controller.requestLogout()
        guard case .unavailable = await projectionCache(f).projectionState(scope: f.scope) else {
            return XCTFail()
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "1")
    }
    func testRebaselinePreservesProjectionAndOriginalToken() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await projectionApply(f, page: page)
        _ = try await queueEnqueued(f)
        try await f.database.blockInbound(scope: f.scope, credentialId: queueUUID(92))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT completeness FROM node_projection_state"),
            "REBASELINE_REQUIRED")
        let stored = try await f.database.inboundPage(
            scope: f.scope, position: page.start, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(stored?.page.evidence, page.evidence)
    }
    func testLibraryMetadataIsDurableWithoutClaimingCompleteTree() async throws {
        let f = try await queueFixture(self)
        let library = Library(
            id: f.scope.libraryId, revision: try LibraryRevision(validating: "8"),
            name: "Canonical Library",
            rootNodeId: try await NodeId.validated(queueUUID(2), using: QueueValidator()),
            status: .active,
            createdAt: Date(timeIntervalSince1970: 1_700_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_700_000_001))
        try await projectionCache(f).rememberLibrary(library, scope: f.scope)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let state = try await reopened.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.library?.library, library)
        XCTAssertEqual(state.completeness, .uninitialized)
    }
    func testDuplicateNamesRemainDistinctByNodeIdentity() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(
            f, events: [projectionEvent(), projectionEvent(id: 3, event: 1002, sequence: "2")])
        let nodes = try await projectionNodes(f)
        let second = try await projectionNode(scope: f.scope, id: 3)
        nodes.values[second.id] = .loaded(second)
        _ = try await projectionApply(f, page: page, repository: nodes)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        guard
            case .loaded(let children) = await projectionCache(f).children(
                scope: f.scope, parentId: parent, limit: 10)
        else { return XCTFail() }
        XCTAssertEqual(children.nodes.map(\.id.rawValue), [queueUUID(1), queueUUID(3)])
        XCTAssertEqual(children.nodes[0].name, children.nodes[1].name)
        XCTAssertEqual(children.knowledge, .partial)
    }
    func testDifferentLibraryCannotReadProjection() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let other = try await queueScope(library: 81)
        guard case .unavailable = await projectionCache(f).projectionState(scope: other) else {
            return XCTFail()
        }
    }
    func testDifferentDeviceCannotReadProjection() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let other = try await queueScope(device: 93)
        guard case .unavailable = await projectionCache(f).projectionState(scope: other) else {
            return XCTFail()
        }
    }
    func testDifferentOwnerCannotReadProjection() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let other = try await queueScope(owner: 94)
        guard case .unavailable = await projectionCache(f).projectionState(scope: other) else {
            return XCTFail()
        }
    }
    func testDifferentOriginCannotReadProjection() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let other = try await queueScope(endpoint: "https://other.example")
        guard case .unavailable = await projectionCache(f).projectionState(scope: other) else {
            return XCTFail()
        }
    }
    func testStorageCapacityRollsBackPreservingInboundEvidence() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let bounded = try MutationQueueSQLiteStore(url: f.url, maximumBytes: 1)
        let nodes = try await projectionNodes(f)
        let plan = try await SyncNodeMaterializer(repository: nodes, bridge: QueueValidator())
            .prepare(page) {}
        do {
            _ = try await bounded.applyProjection(
                plan, credentialId: queueUUID(92), bridge: QueueValidator())
            XCTFail()
        } catch { XCTAssertEqual(error as? MutationQueueFailure, .capacity) }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
    }
    func testSQLiteBusyLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let nodes = try await projectionNodes(f)
        let plan = try await SyncNodeMaterializer(repository: nodes, bridge: QueueValidator())
            .prepare(page) {}
        let second = try MutationQueueSQLiteStore(url: f.url, busyTimeoutMilliseconds: 0)
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(f.url.path, &db), SQLITE_OK)
        let handle = try XCTUnwrap(db)
        defer {
            sqlite3_exec(handle, "ROLLBACK", nil, nil, nil)
            sqlite3_close(handle)
        }
        XCTAssertEqual(sqlite3_exec(handle, "BEGIN IMMEDIATE", nil, nil, nil), SQLITE_OK)
        await queueAssertFailure(.busy) {
            try await second.applyProjection(
                plan, credentialId: queueUUID(92), bridge: QueueValidator())
        }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testMoveUpdatesMembershipAtomically() async throws { try await lifecycle(.nodeMoved) }
    func testTrashRemovesActiveMembershipAndKeepsLastKnownFields() async throws {
        try await lifecycle(.nodeTrashed)
    }
    func testRestoreReplacesTombstoneWithValidatedMetadata() async throws {
        try await lifecycle(.nodeRestored)
    }
    func testPurgeCannotBeResurrectedByNewerGet() async throws { try await lifecycle(.nodePurged) }
    private func lifecycle(_ kind: SyncChangeKind) async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        try await projectionConfirm(f)
        if kind == .nodeRestored {
            let trash = try await projectionPage(
                f,
                events: [projectionEvent(.nodeTrashed, event: 1002, revision: "9", sequence: "2")],
                from: 1)
            _ = try await projectionApply(f, page: trash)
            try await projectionConfirm(f, page: trash)
        }
        let from: UInt64 = kind == .nodeRestored ? 2 : 1
        let parent = kind == .nodeMoved ? 4 : 2
        let page = try await projectionPage(
            f,
            events: [
                projectionEvent(
                    kind, event: 1003, revision: "10", sequence: String(from + 1), parent: parent)
            ], from: from)
        let nodes = try await projectionNodes(
            f,
            node: projectionNode(
                scope: f.scope, parent: parent, revision: "10", name: "New canonical 🌈"))
        if parent == 4 {
            let directory = try await projectionNode(
                scope: f.scope, id: 4, parent: nil, kind: .directory)
            nodes.values[directory.id] = .loaded(directory)
        }
        guard case .applied = try await projectionApply(f, page: page, repository: nodes) else {
            return XCTFail()
        }
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        let current = try await f.database.cachedNode(
            scope: f.scope, nodeId: id, credentialId: queueUUID(92), bridge: QueueValidator())
        let oldParent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        guard
            case .loaded(let oldChildren) = await projectionCache(f).children(
                scope: f.scope, parentId: oldParent, limit: 10)
        else { return XCTFail() }
        if kind != .nodeRestored { XCTAssertTrue(oldChildren.nodes.isEmpty) }
        switch kind {
        case .nodeMoved:
            let newParent = try await NodeId.validated(queueUUID(4), using: QueueValidator())
            guard
                case .loaded(let children) = await projectionCache(f).children(
                    scope: f.scope, parentId: newParent, limit: 10)
            else { return XCTFail() }
            XCTAssertEqual(children.nodes.map(\.id), [id])
        case .nodeTrashed:
            XCTAssertEqual(current?.provenance, .lastKnown)
            XCTAssertEqual(current?.metadata?.revision.rawValue, "8")
            XCTAssertEqual(current?.revision.rawValue, "10")
            XCTAssertNil(current?.metadata?.trashedAt)
            XCTAssertNil(current?.completeNode)
        case .nodeRestored: XCTAssertEqual(current?.completeNode?.revision.rawValue, "10")
        case .nodePurged:
            XCTAssertEqual(current?.lifecycle, .purged)
            XCTAssertNil(current?.metadata)
            try await projectionConfirm(f, page: page)
            let stale = try await projectionPage(
                f,
                events: [
                    projectionEvent(
                        .nodeRestored, event: 1004, revision: "11", sequence: String(from + 2))
                ], from: from + 1)
            let newer = try await projectionNodes(
                f, node: projectionNode(scope: f.scope, revision: "11"))
            let result = try await projectionApply(f, page: stale, repository: newer)
            XCTAssertEqual(result, .failed(.reconciliationRequired))
            XCTAssertEqual(
                try queueRawScalar(f.url, "SELECT lifecycle FROM cached_nodes"), "PURGED")
        default: XCTFail()
        }
    }
    func testCheckpointRefreshCannotRegressPersistedPosition() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        try await projectionConfirm(f)
        let older = try await queueCheckpoint(scope: f.scope, sequence: "0")
        let status = try await f.database.persistCheckpoint(older)
        XCTAssertEqual(status, .reconciliationRequired)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "1")
    }
    func testSameNodeIdsInDifferentLibrariesHaveDistinctRows() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let scope = try await queueScope(library: 81)
        try await f.database.bindSession(scope: scope, credentialId: queueUUID(92))
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: scope))
        var object = feedObject(scope: scope, count: 1)
        var data = object["data"] as! [String: Any]
        data["changes"] = [projectionEvent()]
        object["data"] = data
        let page = try await feedPage(scope: scope, object: object)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let repository = ProjectionNodes()
        let node = try await projectionNode(scope: scope, name: "Other Library Node")
        let parent = try await projectionNode(scope: scope, id: 2, parent: nil, kind: .directory)
        repository.values[node.id] = .loaded(node)
        repository.values[parent.id] = .loaded(parent)
        let plan = try await SyncNodeMaterializer(repository: repository, bridge: QueueValidator())
            .prepare(page) {}
        _ = try await f.database.applyProjection(
            plan, credentialId: queueUUID(92), bridge: QueueValidator())
        let first = try await f.database.cachedNode(
            scope: f.scope, nodeId: node.id, credentialId: queueUUID(92), bridge: QueueValidator())
        let second = try await f.database.cachedNode(
            scope: scope, nodeId: node.id, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(first?.metadata?.libraryId, f.scope.libraryId)
        XCTAssertEqual(second?.metadata?.libraryId, scope.libraryId)
        XCTAssertEqual(second?.metadata?.name, "Other Library Node")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "2")
    }
    func testDuplicateEventIdentityAcrossPagesFailsClosed() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        try await projectionConfirm(f)
        let page = try await projectionPage(
            f, events: [projectionEvent(revision: "9", sequence: "2")], from: 1)
        let nodes = try await projectionNodes(
            f, node: projectionNode(scope: f.scope, revision: "9"))
        let result = try await projectionApply(f, page: page, repository: nodes)
        XCTAssertEqual(result, .failed(.conflictingEvent))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT revision FROM cached_nodes"), "8")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
    }
    func testCachedRevisionRegressionCannotOverwriteNode() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        try await projectionConfirm(f)
        let page = try await projectionPage(
            f, events: [projectionEvent(event: 1002, revision: "7", sequence: "2")], from: 1)
        let nodes = try await projectionNodes(
            f, node: projectionNode(scope: f.scope, revision: "7"))
        let result = try await projectionApply(f, page: page, repository: nodes)
        XCTAssertEqual(result, .failed(.revisionRegression))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT revision FROM cached_nodes"), "8")
    }
    func testCheckpointObservationAheadOfProjectionRequiresReconciliation() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let beyond = try await queueCheckpoint(scope: f.scope, sequence: "2")
        let status = try await f.database.persistCheckpoint(beyond)
        XCTAssertEqual(status, .reconciliationRequired)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "0")
    }
    func testChildrenLimitIsBounded() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let result = await projectionCache(f).children(scope: f.scope, parentId: parent, limit: 501)
        XCTAssertEqual(result, .unavailable(.storageCapacity))
    }
    func testChildrenZeroLimitFailsClosed() async throws {
        let f = try await queueFixture(self)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let result = await projectionCache(f).children(scope: f.scope, parentId: parent, limit: 0)
        XCTAssertEqual(result, .unavailable(.storageCapacity))
    }
    func testMissingSecondCanonicalNodeRollsBackWholePage() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(
            f, events: [projectionEvent(), projectionEvent(id: 3, event: 1002, sequence: "2")])
        let result = try await projectionApply(f, page: page)
        XCTAssertEqual(result, .failed(.missingMaterialization))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testRealUncommittedProjectionConnectionInterruptionRollsBack() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(f.url.path, &db), SQLITE_OK)
        let handle = try XCTUnwrap(db)
        // Dedicated fault fixture authorizes its private transaction; it never manufactures ACK proof.
        XCTAssertEqual(
            sqlite3_create_function_v2(
                handle, "synveil_projection_authorized", 0, SQLITE_UTF8, nil,
                { context, _, _ in sqlite3_result_int(context, 1) }, nil, nil, nil), SQLITE_OK)
        XCTAssertEqual(sqlite3_exec(handle, "BEGIN IMMEDIATE", nil, nil, nil), SQLITE_OK)
        XCTAssertEqual(
            sqlite3_exec(
                handle, "INSERT INTO node_projection_state VALUES(1,'1','0','0','0','PARTIAL',1)",
                nil, nil, nil), SQLITE_OK)
        let sql =
            "INSERT INTO cached_nodes VALUES(1,'1','\(queueUUID(1))','8','1',NULL,'PURGED','EVENT',NULL,1)"
        XCTAssertEqual(sqlite3_exec(handle, sql, nil, nil, nil), SQLITE_OK)
        XCTAssertEqual(sqlite3_close(handle), SQLITE_OK)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let state = try await reopened.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.completeness, .uninitialized)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testV3MigrationPreservesAppliedConflictUnknownAndAttemptHistory() async throws {
        let f = try await queueFixture(self)
        let applied = try await queueEnqueued(f)
        let first = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: applied.mutation.id)
        try await f.queue.finish(first, result: queueApplied(applied.mutation))
        let conflicted = try await queueEnqueued(
            f, mutation: queuePrepared(id: 11, node: 1, scope: f.scope))
        let second = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: conflicted.mutation.id)
        try await f.queue.finish(second, result: queueConflict(conflicted.mutation))
        let unknownScope = try await queueScope(library: 81)
        try await f.database.bindSession(scope: unknownScope, credentialId: queueUUID(92))
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: unknownScope))
        let unknown = try await queueEnqueued(
            f, mutation: queuePrepared(id: 14, node: 4, scope: unknownScope))
        let third = try await f.queue.acquireAttempt(
            scope: unknownScope, mutationId: unknown.mutation.id)
        try await f.queue.finish(third, result: .outcomeUnknown(.timeout))
        let retry = try await f.queue.acquireRecoveryAttempt(
            scope: unknownScope, mutationId: unknown.mutation.id)
        try await f.queue.finish(retry, result: .outcomeUnknown(.timeout))
        let originalHistory = try await f.database.attemptHistory(
            scope: unknownScope, id: unknown.mutation.id.rawValue)
        let originals =
            try await f.database.activityRecords(scope: f.scope, limit: 100)
            + f.database.activityRecords(scope: unknownScope, limit: 100)
        try projectionDowngradeV3(f.url)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let preserved =
            try await reopened.activityRecords(scope: f.scope, limit: 100)
            + reopened.activityRecords(scope: unknownScope, limit: 100)
        XCTAssertEqual(preserved.map(\.request), originals.map(\.request))
        XCTAssertEqual(preserved.map(\.payload), originals.map(\.payload))
        XCTAssertEqual(preserved.map(\.evidence), originals.map(\.evidence))
        XCTAssertEqual(preserved.map(\.attempt), originals.map(\.attempt))
        XCTAssertEqual(preserved.map(\.state), [.applied, .conflict, .outcomeUnknown])
        let preservedHistory = try await reopened.attemptHistory(
            scope: unknownScope, id: unknown.mutation.id.rawValue)
        XCTAssertEqual(originalHistory, preservedHistory)
        XCTAssertEqual(preservedHistory.count, 1)
    }

    func testFeedReplayAfterApplicationRetainsCommittedStateAndOriginalToken() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await projectionApply(f, page: page)
        await f.transport.set(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: page.responseBody))
        guard
            case .staged(let record, let existing) = await feedService(f).readAndStage(
                scope: f.scope)
        else { return XCTFail() }
        XCTAssertTrue(existing)
        XCTAssertEqual(record.state, .appliedAckPending)
        XCTAssertEqual(record.page.evidence, page.evidence)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_commits"), "1")
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testPostCommitCancellationNeverReportsRolledBackPage() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let nodes = try await projectionNodes(f)
        let gate = QueueGate()
        nodes.gate = gate
        let service = SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: nodes,
            bridge: QueueValidator())
        let task = Task { await service.apply(scope: f.scope, position: page.start) }
        await gate.wait()
        fault.arm(.afterProjectionCommit, action: { task.cancel() })
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .committedButSessionChanged)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "APPLIED_ACK_PENDING")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "1")
    }

    func testProductionNodeRepositoryMaterializesMetadataOnly() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let node = try await projectionNode(scope: f.scope)
        let parent = try await projectionNode(scope: f.scope, id: 2, parent: nil, kind: .directory)
        await f.transport.setSequence(try [projectionNodeHTTP(node), projectionNodeHTTP(parent)])
        let repository = AuthenticatedNodeRepository(provider: f.provider, bridge: QueueValidator())
        let service = SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: repository,
            bridge: QueueValidator())
        guard case .applied = await service.apply(scope: f.scope, position: page.start) else {
            return XCTFail()
        }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 3)
        XCTAssertEqual(requests[1].url.path, "/synveil/api/v1/nodes/\(node.id.rawValue)")
        XCTAssertEqual(requests[2].url.path, "/synveil/api/v1/nodes/\(parent.id.rawValue)")
        XCTAssertTrue(
            requests.allSatisfy { $0.method == .get && !$0.url.path.hasSuffix("/content") })
        XCTAssertTrue(requests.allSatisfy { $0.headers["Cookie"] == nil })
    }
    func testStaleProductionNodeResponseCannotOverwriteNewCredential() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        await f.transport.set(try projectionNodeHTTP(projectionNode(scope: f.scope)))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let repository = AuthenticatedNodeRepository(provider: f.provider, bridge: QueueValidator())
        let service = SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: repository,
            bridge: QueueValidator())
        let task = Task { await service.apply(scope: f.scope, position: page.start) }
        await gate.wait()
        await f.credentials.replace(try queueSession(f.scope, credential: 93))
        await gate.release()
        guard case .failed = await task.value else { return XCTFail() }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testDeviceRevocationDuringCanonicalReadLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        await f.transport.set(try syncError("device_revoked", status: 401))
        let repository = AuthenticatedNodeRepository(provider: f.provider, bridge: QueueValidator())
        let service = SyncFeedApplicationService(
            provider: f.provider, queue: f.queue, database: f.database, nodes: repository,
            bridge: QueueValidator())
        guard case .failed = await service.apply(scope: f.scope, position: page.start) else {
            return XCTFail()
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
        guard case .unavailable = await projectionCache(f).projectionState(scope: f.scope) else {
            return XCTFail()
        }
    }
    func testQuarantineFailureClosesProjectionAndAckAuthority() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        fault.arm(.beforeQuarantine)
        f.queue.invalidateSession()
        do {
            _ = try await f.queue.capture(scope: f.scope)
            XCTFail()
        } catch {}
        guard case .unavailable = await projectionCache(f).projectionState(scope: f.scope) else {
            return XCTFail()
        }
        do {
            _ = try await projectionStorage(f).claimAppliedPage(
                scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
            XCTFail()
        } catch {}
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "APPLIED_ACK_PENDING")
    }
    func testFiveHundredEventsUseBoundedDeduplicatedLookups() async throws {
        let f = try await queueFixture(self)
        var events: [[String: Any]] = []
        for index: Int in 0..<500 {
            let revision = String(8 + index)
            let sequence = String(1 + index)
            events.append(
                projectionEvent(event: 1000 + index, revision: revision, sequence: sequence))
        }
        let page = try await projectionPage(f, events: events)
        let nodes = try await projectionNodes(
            f, node: projectionNode(scope: f.scope, revision: "507"))
        guard case .applied = try await projectionApply(f, page: page, repository: nodes) else {
            return XCTFail()
        }
        XCTAssertEqual(nodes.calls.count, 2)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_events"), "500")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "500")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
    }

}

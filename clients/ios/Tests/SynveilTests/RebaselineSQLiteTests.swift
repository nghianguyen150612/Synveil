import Foundation
import XCTest

@testable import Synveil

@MainActor
final class RebaselineSQLiteTests: XCTestCase {
    func testFreshV5AndV4MigrationPreserveOriginalQueueBytes() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let bytes = try queueRawScalar(f.url, "SELECT hex(request) FROM mutations")
        try snapshotDowngradeV4(f.url)
        let migrated = try MutationQueueSQLiteStore(url: f.url)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "5")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT hex(request) FROM mutations"), bytes)
        let records = try await migrated.records(scope: f.scope, limit: 1)
        XCTAssertEqual(records.first?.mutationId, original.mutation.id.rawValue)
    }
    func testV4MigrationPreservesUnknownAttemptAndSignedAckEvidence() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let ack = projectionAck(f)
        let receipt = try await ack.receipt(scope: f.scope, position: feedPosition())
        await f.transport.fail(.timeout)
        _ = await ack.acknowledge(receipt)
        let pages = try queueRawScalar(f.url, "SELECT hex(response) FROM inbound_pages")
        let attempts = try queueRawScalar(f.url, "SELECT attempt_id FROM sync_ack_attempts")
        try snapshotDowngradeV4(f.url)
        _ = try MutationQueueSQLiteStore(url: f.url)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT hex(response) FROM inbound_pages"), pages)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT attempt_id FROM sync_ack_attempts"), attempts)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
    }
    func testMigrationRollbackAndFutureSchemaFailClosed() async throws {
        let f = try await queueFixture(self)
        try snapshotDowngradeV4(f.url)
        let fault = QueueFaultInjector()
        fault.arm(.migration)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url, fault: { try fault.hit($0) }))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "4")
        _ = try MutationQueueSQLiteStore(url: f.url)
        try queueRawSQL(f.url, "PRAGMA user_version=6")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url))
    }
    func testStagingPreservesOldActiveCacheUntilAtomicReplacement() async throws {
        let f = try await queueFixture(self)
        let old = try await projectionNode(scope: f.scope, id: 9)
        let page = try await projectionPage(f, events: [projectionEvent(id: 9)])
        _ = try await projectionApply(f, page: page, repository: projectionNodes(f, node: old))
        try await projectionConfirm(f, page: page)
        let oldBytes = try queueRawScalar(f.url, "SELECT hex(metadata) FROM cached_nodes")
        let coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT hex(metadata) FROM cached_nodes"), oldBytes)
        let prior = try await f.database.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(prior.completeness, .partial)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
        let state = try await f.database.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.completeness, .complete)
        XCTAssertEqual(state.locallyApplied?.sequence.rawValue, "25")
        XCTAssertEqual(state.serverConfirmed, state.locallyApplied)
        let absent = try await f.database.cachedNode(
            scope: f.scope, nodeId: old.id, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertNil(absent)
        let observed3 = try await f.queue.persistedBase(scope: f.scope).sequence.rawValue
        XCTAssertEqual(observed3, "25")
    }
    func testPreparedSnapshotAndSavedCursorSurviveReopenWithoutNetwork() async throws {
        let f = try await queueFixture(self)
        let coordinator = snapshotCoordinator(f)
        try await snapshotStart(f, coordinator: coordinator)
        await f.transport.set(
            try queueHTTP(
                snapshotPageObject(f.scope, rows: Array(snapshotRows().prefix(2)), more: true)))
        try await coordinator.download(scope: f.scope, maximumPages: 1)
        let requests = await f.transport.requests().count
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let saved = try await reopened.rebaseline(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(saved?.cursor, "opaque.server.cursor+_")
        let observed4 = await f.transport.requests().count
        XCTAssertEqual(observed4, requests)
        let resumed = snapshotCoordinator(f, database: reopened)
        await f.transport.set(
            try queueHTTP(snapshotPageObject(f.scope, rows: Array(snapshotRows().suffix(1)))))
        try await resumed.download(scope: f.scope, maximumPages: 1)
        try await resumed.prepare(scope: f.scope)
        let again = try MutationQueueSQLiteStore(url: f.url)
        let prepared = try await again.rebaseline(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(prepared?.state, .prepared)
        XCTAssertNotNil(prepared?.preparedId)
        XCTAssertNotNil(prepared?.terminalToken)
    }
    func testPageRollbackAndCapacityFailureKeepPriorProjection() async throws {
        for point in [
            MutationQueueFaultPoint.beforeSnapshotPageCommit, .beforeSnapshotPreparationCommit,
        ] {
            let fault = QueueFaultInjector(), f = try await queueFixture(self, fault: fault)
            let coordinator = snapshotCoordinator(f)
            try await snapshotStart(f, coordinator: coordinator)
            await f.transport.set(try queueHTTP(snapshotPageObject(f.scope, rows: snapshotRows())))
            if point == .beforeSnapshotPageCommit {
                fault.arm(point)
                await snapshotAssertFailure { try await coordinator.download(scope: f.scope) }
                let observed5 = try await coordinator.status(scope: f.scope)?.stagedNodes
                XCTAssertEqual(observed5, 0)
            } else {
                try await coordinator.download(scope: f.scope)
                fault.arm(point)
                await snapshotAssertFailure { try await coordinator.prepare(scope: f.scope) }
                let observed6 = try await coordinator.status(scope: f.scope)?.state
                XCTAssertEqual(observed6, .terminalReceived)
            }
            let observed7 = try await f.database.cachedProjectionState(
                scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator()
            ).locallyApplied
            XCTAssertNil(observed7)
        }
        let f = try await queueFixture(self, maximumBytes: 1000)
        await snapshotAssertFailure(.capacity) { try await snapshotStart(f) }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_sessions"), "0")
    }
    func testExactPageReplayIdempotentAndConflictingReplayRejected() async throws {
        let f = try await queueFixture(self)
        try await snapshotStart(f)
        let owner = UUID()
        try await f.database.claimRebaselineRun(
            scope: f.scope, credentialId: queueUUID(92), owner: owner)
        let observed8 = try await f.database.rebaseline(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        let saved = try XCTUnwrap(observed8)
        let page = try await RebaselineResponseDecoder(bridge: QueueValidator()).page(
            queueHTTP(snapshotPageObject(f.scope, rows: snapshotRows())), expected: saved.bootstrap)
        try await f.database.stageRebaseline(
            page, cursor: nil, credentialId: queueUUID(92), owner: owner, bridge: QueueValidator())
        try await f.database.stageRebaseline(
            page, cursor: nil, credentialId: queueUUID(92), owner: owner, bridge: QueueValidator())
        var rows = snapshotRows()
        rows[2]["name"] = "different"
        let conflicting = try await RebaselineResponseDecoder(bridge: QueueValidator()).page(
            queueHTTP(snapshotPageObject(f.scope, rows: rows)), expected: saved.bootstrap)
        await snapshotAssertFailure(.protocolFailure) {
            try await f.database.stageRebaseline(
                conflicting, cursor: nil, credentialId: queueUUID(92), owner: owner,
                bridge: QueueValidator())
        }
        await f.database.finishRebaselineRun(owner: owner)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_nodes"), "3")
    }
    func testCountMismatchRejectsTerminalCommitAndBlocksCompletion() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotStart(f, coordinator: coordinator)
        await f.transport.set(
            try queueHTTP(snapshotPageObject(f.scope, rows: Array(snapshotRows().prefix(2)))))
        await snapshotAssertFailure(.countMismatch) {
            try await coordinator.download(scope: f.scope)
        }
        await snapshotAssertFailure(.unverifiedManifest) {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_nodes"), "0")
    }
    func testPersistedGraphValidationRejectsWrongRootMissingParentCycleAndFileParent() async throws
    {
        for variant in 0..<5 {
            let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
            try await snapshotStart(f, coordinator: coordinator, root: variant == 0 ? 9 : 1)
            var rows = snapshotRows()
            switch variant {
            case 1: rows[2]["parent_node_id"] = queueUUID(99)
            case 2:
                rows[1]["parent_node_id"] = queueUUID(3)
                rows[2]["kind"] = "DIRECTORY"
            case 3: rows[1]["kind"] = "FILE"
            case 4: rows[1].removeValue(forKey: "parent_node_id")
            default: break
            }
            await f.transport.set(try queueHTTP(snapshotPageObject(f.scope, rows: rows)))
            try await coordinator.download(scope: f.scope)
            await snapshotAssertFailure(.invalidGraph) {
                try await coordinator.prepare(scope: f.scope)
            }
            let observed9 = try await f.database.rebaseline(
                scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())?.preparedId
            XCTAssertNil(observed9)
        }
    }
    func testTrashedAncestryAndDuplicateNamesAreValidLogicalGraph() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotStart(f, coordinator: coordinator)
        var rows = snapshotRows()
        rows[1]["state"] = "TRASHED"
        rows[2]["name"] = rows[1]["name"]
        await f.transport.set(try queueHTTP(snapshotPageObject(f.scope, rows: rows)))
        try await coordinator.download(scope: f.scope)
        try await coordinator.prepare(scope: f.scope)
        let observed10 = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(observed10, .prepared)
    }
    func testSnapshotTablesRejectUnownedRawWrites() async throws {
        let f = try await queueFixture(self)
        try await snapshotPrepared(f)
        XCTAssertThrowsError(
            try queueRawSQL(f.url, "UPDATE rebaseline_sessions SET stage='ACTIVE_COMPLETE'"))
        XCTAssertThrowsError(try queueRawSQL(f.url, "DELETE FROM rebaseline_nodes"))
        XCTAssertThrowsError(
            try queueRawSQL(f.url, "UPDATE rebaseline_pages SET request_cursor='forged'"))
    }
    func testPersistedEvidenceCorruptionCannotPrepareOrDispatchCompletion() async throws {
        for variant in 0..<3 {
            let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
            try await snapshotStart(f, coordinator: coordinator)
            await f.transport.set(try queueHTTP(snapshotPageObject(f.scope, rows: snapshotRows())))
            try await coordinator.download(scope: f.scope)
            let originalBase = try await f.database.syncBase(scope: f.scope)
            switch variant {
            case 0:
                try queueRawSQL(f.url, "DROP TRIGGER rebaseline_nodes_update_gate")
                try queueRawSQL(
                    f.url,
                    "UPDATE rebaseline_nodes SET metadata=? WHERE node_id='" + queueUUID(3) + "'",
                    bindings: [Data("{}".utf8)])
            case 1:
                try queueRawSQL(f.url, "DROP TRIGGER rebaseline_pages_update_gate")
                try queueRawSQL(
                    f.url, "UPDATE rebaseline_pages SET canonical=?", bindings: [Data("{}".utf8)])
            default:
                try queueRawSQL(f.url, "DROP TRIGGER rebaseline_sessions_update_gate")
                try queueRawSQL(f.url, "UPDATE rebaseline_sessions SET terminal_token=NULL")
            }
            let requests = await f.transport.requests().count
            await snapshotAssertFailure { try await coordinator.prepare(scope: f.scope) }
            await snapshotAssertFailure {
                try await coordinator.complete(
                    scope: f.scope, recovering: false, confirmedByUser: true)
            }
            let after = await f.transport.requests().count
            XCTAssertEqual(after, requests)
            let base = try await f.database.syncBase(scope: f.scope)
            XCTAssertEqual(base?.sequence, originalBase?.sequence)
            XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url))
        }
    }

}

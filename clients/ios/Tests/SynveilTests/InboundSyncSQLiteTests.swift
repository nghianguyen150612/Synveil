import Foundation
import SQLite3
import XCTest

@testable import Synveil

@MainActor
final class InboundSyncSQLiteTests: XCTestCase {
    func testFileBackedStageAndReopenPreservesOriginalEvidenceUnapplied() async throws {
        let f = try await queueFixture(self)
        guard case .staged(let staged, existing: false) = try await feedStage(f) else {
            return XCTFail()
        }
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let row = try await feedRead(reopened, scope: f.scope)
        XCTAssertEqual(row, staged)
        XCTAssertEqual(row?.state, .receivedUnapplied)
        XCTAssertEqual(row?.page.events.map(\.sequence.rawValue), ["1", "2"])
        XCTAssertEqual(row?.page.evidence?.token, "v1.sync-ack.original-evidence_123")
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "4")
    }
    func testDuplicatePageIdempotentAcrossRequestIds() async throws {
        let f = try await queueFixture(self)
        guard case .staged(let first, existing: false) = try await feedStage(f) else {
            return XCTFail()
        }
        var object = feedObject(scope: f.scope)
        object["meta"] = ["request_id": "feed-request-02"]
        guard case .staged(let second, existing: true) = try await feedStage(f, object: object)
        else { return XCTFail() }
        XCTAssertEqual(first, second)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "1")
    }
    func testConflictingTokenDoesNotOverwriteStagedEvidence() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        let before = try await feedRead(f.database, scope: f.scope)
        var object = feedObject(scope: f.scope)
        var data = object["data"] as! [String: Any]
        data["ack_token"] = "different-token"
        object["data"] = data
        let result = try await feedStage(f, object: object)
        XCTAssertEqual(result, .failed(.checkpointConflict))
        let after = try await feedRead(f.database, scope: f.scope)
        XCTAssertEqual(after, before)
    }
    func testConflictingEventsDoesNotOverwritePage() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        var object = feedObject(scope: f.scope)
        var data = object["data"] as! [String: Any]
        var events = data["changes"] as! [[String: Any]]
        events[0]["resource_revision"] = "7"
        data["changes"] = events
        object["data"] = data
        let result = try await feedStage(f, object: object)
        XCTAssertEqual(result, .failed(.checkpointConflict))
    }
    func testEmptyReadDoesNotStageOrAdvanceCheckpoint() async throws {
        let f = try await queueFixture(self)
        let result = try await feedStage(f, object: feedObject(scope: f.scope, count: 0))
        XCTAssertEqual(result, .noNewChanges(try feedPosition()))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "0")
    }
    func testRepeatedPageHasMoreNeverLoopsOrAdvancesBaseOrPosts() async throws {
        let f = try await queueFixture(self)
        let object = feedObject(scope: f.scope, high: 50)
        _ = try await feedStage(f, object: object)
        _ = try await feedStage(f, object: object)
        let requests = await f.transport.requests()
        // Explicit checkpoint preparation plus two explicit reads.
        XCTAssertEqual(requests.count, 3)
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "0")
        XCTAssertEqual(requests.last?.url.query, "limit=200")
        XCTAssertFalse(requests.last!.url.absoluteString.contains("cursor"))
    }
    func testPageSizeBoundsAndPreferredQuery() async throws {
        let f = try await queueFixture(self)
        for limit in [0, 501, -1] {
            let result = await feedService(f).readAndStage(scope: f.scope, limit: limit)
            XCTAssertEqual(result, .failed(.protocolFailure))
        }
        await f.transport.set(try queueHTTP(feedObject(scope: f.scope)))
        _ = await feedService(f).readAndStage(scope: f.scope, limit: 500)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(requests.last?.url.query, "limit=500")
    }
    func testCrossDeviceRejected() async throws {
        try await rejectScope(try await queueScope(device: 99))
    }
    func testCrossLibraryRejected() async throws {
        try await rejectScope(try await queueScope(library: 99))
    }
    func testCrossOriginRejected() async throws {
        try await rejectScope(try await queueScope(endpoint: "https://other.example"))
    }
    func testCrossOwnerRejected() async throws {
        try await rejectScope(try await queueScope(owner: 99))
    }
    func testStageWrongCredentialRejected() async throws {
        let f = try await queueFixture(self)
        let page = try await feedPage(scope: f.scope)
        await queueAssertFailure(.scopeMismatch) {
            try await f.database.stageFeed(
                page, credentialId: queueUUID(99), bridge: QueueValidator())
        }
    }
    func testStageAfterLogoutIsQuarantined() async throws {
        let f = try await queueFixture(self)
        let page = try await feedPage(scope: f.scope)
        try await f.database.quarantineSessions()
        await queueAssertFailure(.scopeMismatch) {
            try await f.database.stageFeed(
                page, credentialId: queueUUID(92), bridge: QueueValidator())
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "0")
    }
    func testFailedInsertRollback() async throws { try await faultStage(.beforeInsert) }
    func testFailureAfterInsertRollback() async throws { try await faultStage(.afterInsert) }
    func testFailedCommitDoesNotReportSuccess() async throws { try await faultStage(.beforeCommit) }
    func testLostCommitAcknowledgementUsesScopedReadback() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        fault.arm(.afterCommit)
        guard case .staged = try await feedStage(f) else { return XCTFail() }
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let readback = try await feedRead(reopened, scope: f.scope)
        XCTAssertNotNil(readback)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "1")
    }
    func testStorageLimitPreservesExistingOutboundRecord() async throws {
        let f = try await queueFixture(self, maximumBytes: 2000)
        let original = try await queueEnqueued(f)
        let result = try await feedStage(f)
        XCTAssertEqual(result, .failed(.storage(.capacity)))
        let row = try await queueRecord(f)
        XCTAssertEqual(row, original)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "0")
    }
    func testConcurrentStagingSerializedIdempotently() async throws {
        let f = try await queueFixture(self)
        let page = try await feedPage(scope: f.scope)
        async let a = f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        async let b = f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        let rows = try await [a, b]
        XCTAssertEqual(rows.filter(\.1).count, 1)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "1")
    }
    func testMigrationV2PreservesQueueBytesHistoryBaseAndQuarantine() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let first = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: original.mutation.id)
        try await f.queue.finish(first, result: .outcomeUnknown(.timeout))
        let retry = try await f.queue.acquireRecoveryAttempt(
            scope: f.scope, mutationId: original.mutation.id)
        try await f.queue.finish(retry, result: .outcomeUnknown(.timeout))
        let before = try await queueRecord(f)
        let history = try await f.database.attemptHistory(
            scope: f.scope, id: original.mutation.id.rawValue)
        try await f.database.quarantineSessions()
        try downgradeV2(f.url)
        let migrated = try MutationQueueSQLiteStore(url: f.url)
        let raw = try await migrated.record(scope: f.scope, id: original.mutation.id.rawValue)
        let after = try await MutationPersistenceCodec(bridge: QueueValidator()).rehydrate(
            XCTUnwrap(raw))
        let afterHistory = try await migrated.attemptHistory(
            scope: f.scope, id: original.mutation.id.rawValue)
        XCTAssertEqual(before, after)
        XCTAssertEqual(history, afterHistory)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "4")
    }
    func testV2MigrationFaultRollsBackWithoutLosingRecords() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try downgradeV2(f.url)
        let fault = QueueFaultInjector()
        fault.arm(.migration)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url, fault: { try fault.hit($0) }))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "2")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
        XCTAssertEqual(
            try queueRawScalar(
                f.url, "SELECT count(*) FROM sqlite_master WHERE name='inbound_pages'"), "0")
    }
    func testRebaselinePreservesStagingAndMutations() async throws {
        let f = try await queueFixture(self)
        let mutation = try await queueEnqueued(f)
        _ = try await feedStage(f)
        await f.transport.set(try syncError("sync_rebaseline_required", status: 409))
        let result = await feedService(f).readAndStage(scope: f.scope)
        XCTAssertEqual(result, .failed(.rebaselineRequired))
        let record = try await feedRead(f.database, scope: f.scope)
        XCTAssertEqual(record?.state, .blockedRebaseline)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT status FROM sync_bases"), "RECONCILIATION_REQUIRED")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT mutation_id FROM mutations"),
            mutation.mutation.id.rawValue)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "1")
    }
    func testEpochChangeBlocksDurablyWithoutReset() async throws {
        let f = try await queueFixture(self)
        var object = feedObject(scope: f.scope)
        var data = object["data"] as! [String: Any]
        data["epoch"] = "2"
        object["data"] = data
        let result = try await feedStage(f, object: object)
        XCTAssertEqual(result, .failed(.rebaselineRequired))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT epoch FROM sync_bases"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT status FROM sync_bases"), "RECONCILIATION_REQUIRED")
    }
    func testStagingCannotTransitionToAppliedOrAckStates() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        for state in ["APPLIED_ACK_PENDING", "ACK_IN_FLIGHT", "ACK_CONFIRMED"] {
            XCTAssertThrowsError(
                try queueRawSQL(f.url, "UPDATE inbound_pages SET state='\(state)'"))
        }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
    }
    func testReadbackDetectsCorruptEvidence() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        try queueRawSQL(f.url, "DROP TRIGGER inbound_immutable")
        try queueRawSQL(f.url, "UPDATE inbound_pages SET response=?", bindings: [Data("{}".utf8)])
        await feedAssertFailure(.protocolFailure) { try await feedRead(f.database, scope: f.scope) }
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url))
    }
    func testCrashReopenHasNoNetworkAndNoBearerStored() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        let requests = await f.transport.requests()
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        _ = try await reopened.recoverInterruptedOperations()
        _ = try await feedRead(reopened, scope: f.scope)
        let after = await f.transport.requests()
        XCTAssertEqual(requests, after)
        let body = try queueRawScalar(f.url, "SELECT CAST(response AS TEXT) FROM inbound_pages")
        XCTAssertFalse(body.contains(queueBearer))
        XCTAssertFalse(body.contains("Authorization"))
    }
    func testLateFeedAfterLogoutCannotStageOrPublish() async throws {
        let f = try await queueFixture(self)
        await f.transport.set(try queueHTTP(feedObject(scope: f.scope)))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await feedService(f).readAndStage(scope: f.scope) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        guard case .failed = await task.value else { return XCTFail() }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "0")
    }
    func testCredentialReplacementPreventsStaleFeed() async throws {
        let f = try await queueFixture(self)
        await f.transport.set(try queueHTTP(feedObject(scope: f.scope)))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await feedService(f).readAndStage(scope: f.scope) }
        await gate.wait()
        await f.credentials.replace(try queueSession(f.scope, credential: 99))
        await gate.release()
        guard case .failed = await task.value else { return XCTFail() }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "0")
    }
    func testV2MigrationRetainsAppliedAndConflictResponseEvidence() async throws {
        let f = try await queueFixture(self)
        let applied = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: applied.mutation.id)
        try await f.queue.finish(lease, result: queueApplied(applied.mutation))
        let conflict = try await queueEnqueued(
            f, mutation: queuePrepared(id: 11, node: 1, scope: f.scope))
        let second = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: conflict.mutation.id)
        try await f.queue.finish(second, result: queueConflict(conflict.mutation))
        let originals = try await f.database.activityRecords(scope: f.scope, limit: 100)
        let base = try await f.database.syncBase(scope: f.scope)
        try downgradeV2(f.url)
        let migrated = try MutationQueueSQLiteStore(url: f.url)
        let rows = try await migrated.activityRecords(scope: f.scope, limit: 100)
        let preservedBase = try await migrated.syncBase(scope: f.scope)
        XCTAssertEqual(rows.map(\.request), originals.map(\.request))
        XCTAssertEqual(rows.map(\.payload), originals.map(\.payload))
        XCTAssertEqual(rows.map(\.evidence), originals.map(\.evidence))
        XCTAssertEqual(rows.map(\.state), [.applied, .conflict])
        XCTAssertEqual(preservedBase?.responseBody, base?.responseBody)
    }
    func testUncommittedSQLiteInterruptionRetainsLastCommittedPage() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        var handle: OpaquePointer?
        XCTAssertEqual(sqlite3_open(f.url.path, &handle), SQLITE_OK)
        let db = try XCTUnwrap(handle)
        XCTAssertEqual(
            sqlite3_create_function_v2(
                db, "synveil_projection_authorized", 0, SQLITE_UTF8, nil,
                { context, _, _ in sqlite3_result_int(context, 0) }, nil, nil, nil), SQLITE_OK)
        XCTAssertEqual(sqlite3_exec(db, "BEGIN IMMEDIATE", nil, nil, nil), SQLITE_OK)
        XCTAssertEqual(
            sqlite3_exec(db, "UPDATE inbound_pages SET state='BLOCKED_REBASELINE'", nil, nil, nil),
            SQLITE_OK)
        // Closing a connection with an uncommitted transaction simulates interruption, not power loss.
        XCTAssertEqual(sqlite3_close(db), SQLITE_OK)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let row = try await feedRead(reopened, scope: f.scope)
        XCTAssertEqual(row?.state, .receivedUnapplied)
    }
    func testFeedUnavailablePreservesCredentialAndOriginalInbox() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        let original = try await feedRead(f.database, scope: f.scope)
        await f.transport.set(try syncError("dependency_unavailable", status: 503))
        let result = await feedService(f).readAndStage(scope: f.scope)
        XCTAssertEqual(result, .failed(.transport(.serverUnavailable)))
        let current = try await feedRead(f.database, scope: f.scope)
        XCTAssertEqual(current, original)
        let session = try await f.credentials.load(expectedServerEndpoint: f.scope.serverEndpoint)
        XCTAssertEqual(session.record.credential.rawValue, queueBearer)
    }
    func testConcurrentRebaselineDuringReadbackCannotReturnUnappliedSnapshot() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        let gate = QueueGate()
        let bridge = FeedReadBlockingValidator(gate)
        let task = Task {
            try await f.database.inboundPage(
                scope: f.scope, position: feedPosition(),
                credentialId: queueUUID(92), bridge: bridge)
        }
        await gate.wait()
        try await f.database.blockInbound(scope: f.scope, credentialId: queueUUID(92))
        await gate.release()
        await queueAssertFailure(.reconciliationRequired) { try await task.value }
        let current = try await feedRead(f.database, scope: f.scope)
        XCTAssertEqual(current?.state, .blockedRebaseline)
    }
    private func rejectScope(_ other: ClientMutationScope) async throws {
        let f = try await queueFixture(self)
        let page = try await feedPage(scope: other)
        do {
            _ = try await f.database.stageFeed(
                page, credentialId: queueUUID(92), bridge: QueueValidator())
            XCTFail()
        } catch {}
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "0")
    }
    private func faultStage(_ point: MutationQueueFaultPoint) async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let original = try await queueEnqueued(f)
        fault.arm(point)
        let result = try await feedStage(f)
        XCTAssertEqual(result, .failed(.storage(.diskFull)))
        let row = try await queueRecord(f)
        XCTAssertEqual(row, original)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let page = try await feedRead(reopened, scope: f.scope)
        XCTAssertNil(page)
    }
    private func downgradeV2(_ url: URL) throws {
        try projectionDowngradeV3(url)
        for sql in [
            "DROP TRIGGER inbound_immutable", "DROP TRIGGER inbound_application_gate",
            "DROP TABLE inbound_pages", "PRAGMA user_version=2",
        ] { try queueRawSQL(url, sql) }
    }
}

import Foundation
import XCTest

@testable import Synveil

@MainActor
final class InboundSyncRecoveryTests: XCTestCase {
    private func unknown(_ f: InboundFixture) async throws {
        let page = try await f.stage()
        guard case .applied = await f.application.apply(scope: f.scope, position: page.start) else {
            return XCTFail("Expected committed production projection")
        }
        let receipt = try await f.ack.receipt(scope: f.scope, position: page.start)
        await f.ack.discardUndispatchedReceipt(receipt)
    }
    func testReceivedUnappliedIsAppliedBeforeAnyGet() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        await f.wire.configure([
            try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")),
            try queueHTTP(feedObject(scope: f.scope, count: 0, from: 1)),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.map(\.method), [.post, .get])
        let row = try await f.record()
        XCTAssertEqual(row?.page.evidence?.token, page.evidence?.token)
    }
    func testAppliedPendingAckDoesNotRematerialize() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        f.nodes.calls.removeAll()
        f.nodes.values.removeAll()
        await f.wire.configure([
            try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")),
            try queueHTTP(feedObject(scope: f.scope, count: 0, from: 1)),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
        XCTAssertEqual(result.progress.eventsApplied, 0)
        XCTAssertTrue(f.nodes.calls.isEmpty)
    }
    func testAckInFlightStopsNormalSyncWithZeroRequests() async throws {
        let f = try await inboundFixture(self)
        try await unknown(f)
        let result = await f.run()
        XCTAssertEqual(result.reason, .awaitingAckRecovery)
        XCTAssertEqual(result.recovery?.attempts, 1)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testAckConfirmedContinuesWithoutReapplyingOrReacking() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        _ = await f.run()
        let before = await f.wire.requests().count
        f.nodes.calls.removeAll()
        await f.wire.configure([try queueHTTP(feedObject(scope: f.scope, count: 0, from: 1))])
        let result = await f.run()
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count - before, 1)
        XCTAssertEqual(requests.last?.method, .get)
        XCTAssertTrue(f.nodes.calls.isEmpty)
        XCTAssertEqual(result.progress.eventsApplied, 0)
    }
    func testBlockedRebaselinePreservesEvidenceAndCache() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        try await f.database.blockInbound(scope: f.scope, credentialId: queueUUID(92))
        let result = await f.run()
        XCTAssertEqual(result.reason, .rebaselineRequired)
        let row = try await f.record()
        XCTAssertEqual(row?.page.evidence?.token, page.evidence?.token)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM cached_nodes"), "1")
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testReopenResumesUnappliedOriginalPage() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let resumed = try await inboundFixture(self, base: f.base, database: reopened)
        await resumed.wire.configure([
            try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")),
            try queueHTTP(feedObject(scope: f.scope, count: 0, from: 1)),
        ])
        let result = await resumed.run()
        XCTAssertEqual(result.reason, .upToDate)
        let row = try await reopened.inboundPage(
            scope: f.scope, position: page.start, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(row?.page.evidence?.token, page.evidence?.token)
        let requests = await resumed.wire.requests()
        XCTAssertEqual(requests.first?.method, .post)
    }
    func testReopenRetainsAppliedProjectionAndPendingAck() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let resumed = try await inboundFixture(self, base: f.base, database: reopened)
        let status = await resumed.coordinator.status(scope: f.scope)
        XCTAssertEqual(status.pendingState, .appliedAckPending)
        XCTAssertEqual(status.progress.locallyApplied?.sequence.rawValue, "1")
        let requests = await resumed.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testReopenRetainsUnknownTokenWithoutAutomaticRecovery() async throws {
        let f = try await inboundFixture(self)
        try await unknown(f)
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let resumed = try await inboundFixture(self, base: f.base, database: reopened)
        _ = await resumed.queue.recoverInterruptedOperations()
        let status = await resumed.coordinator.status(scope: f.scope)
        XCTAssertEqual(status.pendingState, .ackInFlight)
        XCTAssertEqual(status.stopReason, .awaitingAckRecovery)
        let requests = await resumed.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testReopenRetainsConfirmedPositionWithoutNetwork() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        _ = await f.run()
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let resumed = try await inboundFixture(self, base: f.base, database: reopened)
        let status = await resumed.coordinator.status(scope: f.scope)
        XCTAssertEqual(status.progress.serverConfirmed?.sequence.rawValue, "1")
        let requests = await resumed.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testDuplicateStagingAndApplicationStayIdempotent() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        _ = await f.application.apply(scope: f.scope, position: page.start)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM inbound_pages"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM projection_commits"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM projection_events"), "1")
    }
    func testConflictingStageCannotOverwriteSignedEvidence() async throws {
        let f = try await inboundFixture(self)
        let original = try await f.stage()
        var object = inboundObject(f.scope)
        var data = object["data"] as! [String: Any]
        data["ack_token"] = "different.signed.evidence"
        object["data"] = data
        let conflict = try await feedPage(scope: f.scope, object: object)
        await feedAssertFailure(.checkpointConflict) {
            try await f.database.stageFeed(
                conflict, credentialId: queueUUID(92), bridge: QueueValidator())
        }
        let row = try await f.record()
        XCTAssertEqual(row?.page, original)
    }
    func testProjectionCommitFailurePreventsAck() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        fault.arm(.beforeProjectionCommit)
        let result = await f.run()
        XCTAssertEqual(result.reason, .storageFailure)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .receivedUnapplied)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM cached_nodes"), "0")
    }
    func testLostProjectionCommitReturnUsesValidatedReadback() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        fault.arm(.afterProjectionCommit)
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
        XCTAssertEqual(result.progress.eventsApplied, 1)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .ackConfirmed)
    }
    func testAckConfirmationCommitFailureRemainsUnknown() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        fault.arm(.beforeAckConfirmationCommit)
        let result = await f.run()
        XCTAssertEqual(result.reason, .awaitingAckRecovery)
        XCTAssertEqual(result.progress.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "0")
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
    }
    func testLostAckConfirmationReturnDoesNotFabricateFailureOrNextGet() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        fault.arm(.afterAckConfirmationCommit)
        let result = await f.run()
        XCTAssertEqual(result.reason, .progressed)
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "1")
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
    }
    func testResponseLossRequiresExplicitOriginalTokenRecovery() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        await f.wire.fail(at: 1, .timeout)
        let first = await f.run()
        XCTAssertEqual(first.reason, .awaitingAckRecovery)
        await f.wire.configure([try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1"))]
        )
        let recovery = try await f.coordinator.recoverUnknownAcknowledgement(
            scope: f.scope, position: feedPosition(), confirmedByUser: true)
        XCTAssertEqual(recovery.reason, .progressed)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.map(\.method), [.get, .post, .post])
        XCTAssertEqual(requests[1].body, requests[2].body)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM sync_ack_attempts"), "2")
    }
    func testRecoveryWithoutUserConfirmationDoesNotDispatch() async throws {
        let f = try await inboundFixture(self)
        try await unknown(f)
        let result = try await f.coordinator.recoverUnknownAcknowledgement(
            scope: f.scope, position: feedPosition(), confirmedByUser: false)
        XCTAssertEqual(result.reason, .recoveryConfirmationRequired)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testRecoveryWrongPositionDoesNotDispatch() async throws {
        let f = try await inboundFixture(self)
        try await unknown(f)
        let result = try await f.coordinator.recoverUnknownAcknowledgement(
            scope: f.scope, position: feedPosition(from: "2"), confirmedByUser: true)
        XCTAssertEqual(result.reason, .reconciliationRequired)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testRecoveryAttemptLimitDisablesFurtherDispatch() async throws {
        let f = try await inboundFixture(self)
        try await unknown(f)
        for _ in 1..<8 {
            let receipt = try await f.ack.recoveryReceipt(scope: f.scope, position: feedPosition())
            await f.ack.discardUndispatchedReceipt(receipt)
        }
        let result = try await f.coordinator.recoverUnknownAcknowledgement(
            scope: f.scope, position: feedPosition(), confirmedByUser: true)
        XCTAssertEqual(result.reason, .recoveryLimitReached)
        XCTAssertEqual(result.recovery?.attempts, 8)
        XCTAssertEqual(result.recovery?.canRecover, false)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testCheckpointRegressionBlocksFurtherPages() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([
            try queueHTTP(inboundObject(f.scope)),
            try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "0")),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .reconciliationRequired)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .blockedRebaseline)
    }
    func testCheckpointAheadOfLocalProjectionIsBlocked() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([
            try queueHTTP(inboundObject(f.scope)),
            try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "99")),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .reconciliationRequired)
        XCTAssertEqual(result.progress.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "0")
    }
    func testMalformedAckResponseNeverConfirms() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([
            try queueHTTP(inboundObject(f.scope)),
            try queueHTTP(["data": ["acknowledged_sequence": "1"]]),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .awaitingAckRecovery)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .ackInFlight)
    }
    func testServerCheckpointConflictPreservesOriginalToken() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([
            try queueHTTP(inboundObject(f.scope)),
            try inboundError("checkpoint_conflict", status: 409),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .reconciliationRequired)
        let row = try await f.record()
        XCTAssertEqual(row?.page.evidence?.token, "v1.original-page-0")
    }
    func testServerRebaselinePreservesAppliedNodesAndMutations() async throws {
        let f = try await inboundFixture(self)
        let mutation = try await queuePrepared(scope: f.scope)
        _ = await f.queue.enqueue(mutation)
        await f.wire.configure([
            try queueHTTP(inboundObject(f.scope)),
            try inboundError("sync_rebaseline_required", status: 409),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .rebaselineRequired)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM cached_nodes"), "1")
        let stored = try await f.database.record(scope: f.scope, id: mutation.id.rawValue)
        XCTAssertEqual(stored?.request, mutation.requestBody)
    }
    func testInboundConfirmationDoesNotRebaseQueuedMutation() async throws {
        let f = try await inboundFixture(self)
        let mutation = try await queuePrepared(scope: f.scope)
        _ = await f.queue.enqueue(mutation)
        try await f.configure()
        let result = await f.run()
        XCTAssertEqual(result.reason, .rebaselineRequired)
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "1")
        let stored = try await f.database.record(scope: f.scope, id: mutation.id.rawValue)
        XCTAssertEqual(stored?.sequence, "0")
        XCTAssertEqual(stored?.request, mutation.requestBody)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
    }
    func testOversizedDurablePageDefersWithoutDiscard() async throws {
        let f = try await inboundFixture(self)
        _ = try await f.stage(count: 2)
        let result = await f.run(.init(pageSize: 200, maximumPages: 8, maximumEvents: 1))
        XCTAssertEqual(result.reason, .moreWork)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .receivedUnapplied)
    }
    func testUnavailableSQLiteStopsBeforeNetwork() async throws {
        let f = try await inboundFixture(self)
        try queueRawSQL(f.base.url, "DROP TABLE inbound_pages")
        let result = await f.run()
        XCTAssertEqual(result.reason, .storageFailure)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
}

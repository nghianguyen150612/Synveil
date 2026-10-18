import Foundation
import XCTest

@testable import Synveil

@MainActor
final class InboundSyncRaceTests: XCTestCase {
    func testDuplicateRunRejectedWhileFeedSuspends() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 0)
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let first = Task { await f.run() }
        await gate.wait()
        let second = await f.run()
        XCTAssertEqual(second.reason, .alreadyRunning)
        await gate.release()
        let result = await first.value
        XCTAssertEqual(result.reason, .upToDate)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testSeparateCoordinatorsAndConnectionsShareDatabaseOwnership() async throws {
        let f = try await inboundFixture(self)
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let second = try await inboundFixture(self, base: f.base, database: reopened)
        try await f.configure(pages: 0)
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let first = Task { await f.run() }
        await gate.wait()
        let result = await second.run()
        XCTAssertEqual(result.reason, .alreadyRunning)
        let requests = await second.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        await gate.release()
        _ = await first.value
    }
    func testRunOwnershipReleasedAfterFailure() async throws {
        let f = try await inboundFixture(self)
        let first = await f.run()
        XCTAssertEqual(first.reason, .offline)
        try await f.configure(pages: 0)
        let second = await f.run()
        XCTAssertEqual(second.reason, .upToDate)
    }
    func testCancellationBeforeGetMakesZeroRequests() async throws {
        let f = try await inboundFixture(self)
        let task = Task { await f.run() }
        task.cancel()
        let result = await task.value
        XCTAssertEqual(result.reason, .cancelled)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testCancellationDuringFeedDoesNotStage() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let task = Task { await f.run() }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .cancelled)
        let record = try await f.record()
        XCTAssertNil(record)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testCancellationAfterStagingPreservesReceivedPage() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        var task: Task<InboundSyncRunResult, Never>?
        task = Task { await f.run { if $0.phase == .staged { task?.cancel() } } }
        let result = await task!.value
        XCTAssertEqual(result.reason, .cancelled)
        let record = try await f.record()
        XCTAssertEqual(record?.state, .receivedUnapplied)
        XCTAssertTrue(f.nodes.calls.isEmpty)
    }
    func testCancellationDuringMaterializationLeavesNoPartialNodes() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        f.nodes.gate = gate
        let task = Task { await f.run() }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .cancelled)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM cached_nodes"), "0")
        let record = try await f.record()
        XCTAssertEqual(record?.state, .receivedUnapplied)
    }
    func testCancellationAfterProjectionCommitPreservesPendingAck() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        var task: Task<InboundSyncRunResult, Never>?
        task = Task { await f.run { if $0.phase == .applied { task?.cancel() } } }
        let result = await task!.value
        XCTAssertEqual(result.reason, .cancelled)
        let record = try await f.record()
        XCTAssertEqual(record?.state, .appliedAckPending)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT applied_sequence FROM node_projection_state"),
            "1")
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testAckLeaseCommitFailureNeverDispatches() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        fault.arm(.beforeAckLeaseCommit)
        _ = await f.run()
        let record = try await f.record()
        XCTAssertEqual(record?.state, .appliedAckPending)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testAckLeaseReturnLossLeavesUnknownWithoutDispatch() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        fault.arm(.afterAckLeaseCommit)
        _ = await f.run()
        let status = await f.coordinator.status(scope: f.scope)
        XCTAssertEqual(status.stopReason, .awaitingAckRecovery)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testCancellationAfterAckDispatchRequiresRecovery() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 1, gate)
        let task = Task { await f.run() }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .awaitingAckRecovery)
        let record = try await f.record()
        XCTAssertEqual(record?.state, .ackInFlight)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT confirmed_sequence FROM node_projection_state"),
            "0")
    }
    func testLogoutDuringFeedFencesLateResponse() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let task = Task { await f.run() }
        await gate.wait()
        await f.base.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .committedButSessionChanged)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM inbound_pages"), "0")
    }
    func testLogoutDuringMaterializationFencesProjection() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        f.nodes.gate = gate
        let task = Task { await f.run() }
        await gate.wait()
        await f.base.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .committedButSessionChanged)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM cached_nodes"), "0")
    }
    func testLogoutDuringAckPreservesUncertainCommit() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 1, gate)
        let task = Task { await f.run() }
        await gate.wait()
        await f.base.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .committedButSessionChanged)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT applied_sequence FROM node_projection_state"),
            "1")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT confirmed_sequence FROM node_projection_state"),
            "0")
    }
    func testCredentialReplacementCannotBorrowReplacementToContinue() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let task = Task { await f.run() }
        await gate.wait()
        await f.base.credentials.replace(try queueSession(f.scope, credential: 99))
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .committedButSessionChanged)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testConcurrentRecoveryOperationsDispatchOnlyOnce() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        let receipt = try await f.ack.receipt(scope: f.scope, position: page.start)
        await f.ack.discardUndispatchedReceipt(receipt)
        await f.wire.configure([try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1"))]
        )
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let first = Task {
            await f.coordinator.recoverUnknownAcknowledgement(
                scope: f.scope, position: page.start, confirmedByUser: true)
        }
        await gate.wait()
        let second = await f.coordinator.recoverUnknownAcknowledgement(
            scope: f.scope, position: page.start, confirmedByUser: true)
        XCTAssertEqual(second.reason, .alreadyRunning)
        await gate.release()
        let result = await first.value
        XCTAssertEqual(result.reason, .progressed)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testDifferentLibraryScopesNeverShareProgress() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        _ = await f.run()
        let other = try await queueScope(library: 81)
        let result = await f.coordinator.synchronize(scope: other, configuration: .foreground)
        XCTAssertEqual(result.reason, .checkpointRequired)
        XCTAssertNil(result.progress.locallyApplied)
        let status = await f.coordinator.status(scope: f.scope)
        XCTAssertEqual(status.progress.serverConfirmed?.sequence.rawValue, "1")
    }
    func testQuarantineBlocksStatusAndRunWithoutNetwork() async throws {
        let f = try await inboundFixture(self)
        try await f.database.quarantineSessions()
        let result = await f.run()
        XCTAssertEqual(result.reason, .scopeMismatch)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testRedactedEvidenceAndPublicProgressContainNoSecrets() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        let status = await f.coordinator.status(scope: f.scope)
        let text = String(describing: status) + String(reflecting: page)
        XCTAssertFalse(text.contains(queueBearer))
        XCTAssertFalse(text.contains("v1.original-page-0"))
    }
    func testLogoutDuringProjectionReadbackCannotPublishStalePosition() async throws {
        let f = try await inboundFixture(self)
        let gate = QueueGate()
        let gated = InboundProjectionReadGate(base: f.projection, gate: gate)
        let coordinator = InboundSyncCoordinator(
            feed: SyncFeedService(
                provider: f.provider, queue: f.queue, store: f.database, bridge: QueueValidator()),
            application: f.application, ack: f.ack, projection: gated, queue: f.queue,
            database: f.database, provider: f.provider, bridge: QueueValidator())
        let task = Task {
            await coordinator.synchronize(scope: f.scope, configuration: .foreground)
        }
        await gate.wait()
        await f.base.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .committedButSessionChanged)
        XCTAssertNil(result.progress.serverConfirmed)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testCancellationBeforeAckLeaseCommitPreservesPendingPage() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        let handle = InboundCancellationHandle()
        fault.arm(.beforeAckLeaseCommit, failure: .cancelled) { handle.cancel() }
        let task = Task { await f.run() }
        handle.install(task)
        let result = await task.value
        XCTAssertEqual(result.reason, .cancelled)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .appliedAckPending)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testCancellationDuringAckConfirmationKeepsUnknownOutcome() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        let handle = InboundCancellationHandle()
        fault.arm(.beforeAckConfirmationCommit, failure: .cancelled) { handle.cancel() }
        let task = Task { await f.run() }
        handle.install(task)
        let result = await task.value
        XCTAssertEqual(result.reason, .awaitingAckRecovery)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .ackInFlight)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT confirmed_sequence FROM node_projection_state"),
            "0")
    }

}

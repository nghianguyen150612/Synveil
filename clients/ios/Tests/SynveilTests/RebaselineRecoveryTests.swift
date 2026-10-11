import Foundation
import XCTest

@testable import Synveil

@MainActor
final class RebaselineRecoveryTests: XCTestCase {
    func testResponseLossLeavesPreparedUnknownAndSameTokenRecoveryActivatesAfterReopen()
        async throws
    {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.fail(.timeout)
        await snapshotAssertFailure {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        let status = try await coordinator.status(scope: f.scope)
        XCTAssertEqual(status?.state, .outcomeUnknown)
        let observed11 = await f.transport.requests().last
        let original = try XCTUnwrap(observed11)
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.sequence, "0")
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let recovery = snapshotCoordinator(f, database: reopened)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope, replayed: true)))
        try await recovery.complete(scope: f.scope, recovering: true, confirmedByUser: true)
        let observed12 = await f.transport.requests().last
        let repeated = try XCTUnwrap(observed12)
        XCTAssertEqual(original.url, repeated.url)
        XCTAssertEqual(original.body, repeated.body)
        let observed13 = try await recovery.status(scope: f.scope)?.state
        XCTAssertEqual(observed13, .activeComplete)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_sessions"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_completion_attempts"), "2")
    }
    func testActivationFailurePreservesConfirmationAndExplicitRecoveryDoesNotPostAgain()
        async throws
    {
        let fault = QueueFaultInjector(), f = try await queueFixture(self, fault: fault),
            coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        fault.arm(.beforeSnapshotActivationCommit)
        await snapshotAssertFailure {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        let observed14 = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(observed14, .completionConfirmed)
        let observed15 = try await f.database.syncBase(scope: f.scope)?.sequence
        XCTAssertEqual(observed15, "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_active"), "0")
        let requests = await f.transport.requests().count
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        try await snapshotCoordinator(f, database: reopened).complete(
            scope: f.scope, recovering: true, confirmedByUser: true)
        let observed16 = await f.transport.requests().count
        XCTAssertEqual(observed16, requests)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT stage FROM rebaseline_sessions"), "ACTIVE_COMPLETE")
    }
    func testConfirmationPersistenceFailureRemainsUnknown() async throws {
        let fault = QueueFaultInjector(), f = try await queueFixture(self, fault: fault),
            coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        fault.arm(.beforeSnapshotConfirmationCommit)
        await snapshotAssertFailure {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        let observed17 = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(observed17, .outcomeUnknown)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_active"), "0")
    }
    func testCrashBeforeDispatchHasDurableUnknownAttempt() async throws {
        let f = try await queueFixture(self)
        try await snapshotPrepared(f)
        let owner = UUID()
        try await f.database.claimRebaselineRun(
            scope: f.scope, credentialId: queueUUID(92), owner: owner)
        _ = try await f.database.claimRebaselineCompletion(
            scope: f.scope, credentialId: queueUUID(92), owner: owner, recovering: false,
            bridge: QueueValidator())
        await f.database.finishRebaselineRun(owner: owner)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let status = try await reopened.rebaselineProgress(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(status?.state, .outcomeUnknown)
        XCTAssertEqual(status?.recoveryAction, .recoverOriginalCompletion)
    }
    func testIncompleteManifestCannotDispatchCompletion() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotStart(f, coordinator: coordinator)
        let count = await f.transport.requests().count
        await snapshotAssertFailure(.unverifiedManifest) {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        let observed18 = await f.transport.requests().count
        XCTAssertEqual(observed18, count)
    }
    func testCompletionAndRecoveryRequireExplicitConfirmation() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        let count = await f.transport.requests().count
        for recovery in [false, true] {
            await snapshotAssertFailure(.confirmationRequired) {
                try await coordinator.complete(
                    scope: f.scope, recovering: recovery, confirmedByUser: false)
            }
        }
        let observed19 = await f.transport.requests().count
        XCTAssertEqual(observed19, count)
    }
    func testGenerationMismatchCompletionRemainsUnknown() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        var object = snapshotCompletionObject(f.scope), data = object["data"] as! [String: Any],
            bootstrap = data["bootstrap"] as! [String: Any]
        bootstrap["generation"] = "2"
        data["bootstrap"] = bootstrap
        object["data"] = data
        await f.transport.set(try queueHTTP(object))
        await snapshotAssertFailure(.generationMismatch) {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        let observed20 = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(observed20, .outcomeUnknown)
    }
    func testRetentionExpiryDoesNotCreateNewBootstrapOrActivate() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.fail(.timeout)
        await snapshotAssertFailure {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "bootstrap_expired", "message": "Expired",
                        "request_id": "snapshot-request-01", "retryable": false,
                    ]
                ], status: 410))
        await snapshotAssertFailure(.expired) {
            try await coordinator.complete(scope: f.scope, recovering: true, confirmedByUser: true)
        }
        let observed21 = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(observed21, .reconciliationRequired)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_sessions"), "1")
        await snapshotAssertFailure(.reconciliationRequired) {
            try await snapshotStart(f, coordinator: coordinator)
        }
    }
    func testPendingMutationsBlockStartWithoutChangingRequestOrBase() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let count = await f.transport.requests().count
        await snapshotAssertFailure(.incompatibleMutations) { try await snapshotStart(f) }
        let observed22 = try await queueRecord(f)
        XCTAssertEqual(observed22, original)
        let observed23 = await f.transport.requests().count
        XCTAssertEqual(observed23, count)
    }
    func testUnknownSignedAckBlocksStart() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        await snapshotAssertFailure(.incompatibleSync) { try await snapshotStart(f) }
    }
    func testPreparedHandoffBlocksIndependentConnectionSyncCheckpointAndEnqueue() async throws {
        let f = try await queueFixture(self)
        try await snapshotPrepared(f)
        let other = try MutationQueueSQLiteStore(url: f.url)
        await queueAssertFailure(.reconciliationRequired) {
            try await other.claimInboundRun(
                scope: f.scope, credentialId: queueUUID(92), owner: UUID())
        }
        await queueAssertFailure(.reconciliationRequired) {
            try await other.persistCheckpoint(queueCheckpoint(scope: f.scope))
        }
        let mutation = try await queuePrepared(scope: f.scope)
        await queueAssertFailure(.reconciliationRequired) {
            try await other.enqueue(
                mutation,
                payload: MutationPersistenceCodec(bridge: QueueValidator()).payloadBytes(mutation))
        }
    }
    func testCrossConnectionAndConcurrentCompletionExclusion() async throws {
        let f = try await queueFixture(self)
        try await snapshotPrepared(f)
        let other = try MutationQueueSQLiteStore(url: f.url), owner = UUID()
        try await f.database.claimRebaselineRun(
            scope: f.scope, credentialId: queueUUID(92), owner: owner)
        await snapshotAssertFailure(.alreadyRunning) {
            try await other.claimRebaselineRun(
                scope: f.scope, credentialId: queueUUID(92), owner: UUID())
        }
        await f.database.finishRebaselineRun(owner: owner)
        let coordinator = snapshotCoordinator(f), gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        let first = Task {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        await gate.wait()
        await snapshotAssertFailure(.alreadyRunning) {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        await gate.release()
        try await first.value
    }
    func testLogoutFencesLateCompletionAndQuarantinesPreparedEvidence() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        let gate = QueueGate()
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        await f.transport.suspendNextRequest(gate)
        let task = Task {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        await snapshotAssertFailure { try await task.value }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_active"), "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_nodes"), "3")
        await snapshotAssertFailure { try await coordinator.status(scope: f.scope) }
    }
    func testWrongCredentialAndCrossLibraryCannotRecover() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.fail(.timeout)
        await snapshotAssertFailure {
            try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        }
        try await f.credentials.replace(queueSession(f.scope, credential: 93))
        await snapshotAssertFailure {
            try await coordinator.complete(scope: f.scope, recovering: true, confirmedByUser: true)
        }
        let wrong = try await queueScope(library: 99)
        await snapshotAssertFailure {
            try await coordinator.complete(scope: wrong, recovering: true, confirmedByUser: true)
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_active"), "0")
    }
    func testRecoveryAttemptBudgetNeverAutomaticallyLoops() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.fail(.timeout)
        for attempt in 0..<8 {
            await snapshotAssertFailure {
                try await coordinator.complete(
                    scope: f.scope, recovering: attempt > 0, confirmedByUser: true)
            }
        }
        let count = await f.transport.requests().count
        await snapshotAssertFailure(.reconciliationRequired) {
            try await coordinator.complete(scope: f.scope, recovering: true, confirmedByUser: true)
        }
        let after = await f.transport.requests().count
        XCTAssertEqual(after, count)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_completion_attempts"), "8")
    }
    func testCancelledDownloadKeepsPreviouslyCommittedPageOnly() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotStart(f, coordinator: coordinator)
        await f.transport.set(
            try queueHTTP(
                snapshotPageObject(f.scope, rows: Array(snapshotRows().prefix(2)), more: true)))
        try await coordinator.download(scope: f.scope, maximumPages: 1)
        let gate = QueueGate()
        await f.transport.set(
            try queueHTTP(snapshotPageObject(f.scope, rows: Array(snapshotRows().suffix(1)))))
        await f.transport.suspendNextRequest(gate)
        let task = Task { try await coordinator.download(scope: f.scope, maximumPages: 1) }
        await gate.wait()
        task.cancel()
        await gate.release()
        await snapshotAssertFailure { try await task.value }
        let status = try await coordinator.status(scope: f.scope)
        XCTAssertEqual(status?.pages, 1)
        XCTAssertEqual(status?.stagedNodes, 2)
        XCTAssertEqual(status?.state, .downloading)
    }
    func testExpiredDownloadAndExplicitNewGenerationNeverMixRows() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotStart(f, coordinator: coordinator)
        await f.transport.set(
            try queueHTTP(
                snapshotPageObject(f.scope, rows: Array(snapshotRows().prefix(2)), more: true)))
        try await coordinator.download(scope: f.scope, maximumPages: 1)
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "bootstrap_expired", "message": "Expired",
                        "request_id": "snapshot-request-01", "retryable": false,
                    ]
                ], status: 410))
        await snapshotAssertFailure(.expired) { try await coordinator.download(scope: f.scope) }
        let old = try await coordinator.status(scope: f.scope)
        XCTAssertEqual(old?.state, .expired)
        let bootstrap = snapshotBootstrapObject(f.scope, generation: "2", id: 701)
        await f.transport.set(try queueHTTP(snapshotEnvelope(bootstrap)))
        try await coordinator.start(
            scope: f.scope, library: snapshotLibrary(f.scope), confirmedByUser: true)
        await f.transport.set(
            try queueHTTP(snapshotPageObject(f.scope, rows: snapshotRows(), bootstrap: bootstrap)))
        try await coordinator.download(scope: f.scope)
        try await coordinator.prepare(scope: f.scope)
        let status = try await coordinator.status(scope: f.scope)
        XCTAssertEqual(status?.stagedNodes, 3)
        XCTAssertEqual(status?.pages, 1)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_nodes"), "5")
    }
}

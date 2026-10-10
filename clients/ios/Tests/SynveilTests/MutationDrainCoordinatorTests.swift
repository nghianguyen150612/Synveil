import Foundation
import XCTest

@testable import Synveil

@MainActor
func drainCoordinator(
    _ f: QueueFixture, queue: DurableMutationQueue? = nil,
    provider: (any AuthenticatedClientMutationRequestProviderProtocol)? = nil
) -> MutationDrainCoordinator {
    MutationDrainCoordinator(
        queue: queue ?? f.queue, provider: provider ?? f.provider, bridge: QueueValidator())
}

func drainAppliedHTTP(_ mutation: PreparedClientMutation, replayed: Bool = false) throws
    -> HTTPTransportResponse
{
    let resource = mutation.payload.intent.resourceIds[0]
    return try queueHTTP([
        "data": [
            "outcome": "APPLIED", "mutation_id": mutation.id.rawValue,
            "kind": mutation.kind.rawValue, "replayed": replayed, "journal_event_id": queueUUID(12),
            "journal_sequence": "9007199254740993",
            "node": [
                "id": resource.rawValue, "library_id": mutation.base.scope.libraryId.rawValue,
                "parent_node_id": queueUUID(99), "kind": "FILE", "state": "ACTIVE",
                "name": "Verified logical name", "revision": "4",
                "created_at": "2026-10-09T12:00:00Z", "updated_at": "2026-10-09T12:00:01Z",
            ],
        ],
        "meta": ["request_id": "mutation-request-01"],
    ])
}

func drainErrorHTTP(_ code: String, status: Int = 409) throws -> HTTPTransportResponse {
    try queueHTTP(
        [
            "error": [
                "code": code, "retryable": status == 503,
                "request_id": "mutation-request-01", "message": "Private diagnostic never stored",
            ]
        ], status: status)
}

@MainActor
final class MutationDrainCoordinatorTests: XCTestCase {
    func testEmptyQueueDoesNotPOST() async throws {
        let f = try await queueFixture(self)
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .completed(MutationDrainSummary()))
        await posts(f, 0)
    }
    func testOnePendingUsesOriginalImmutableRequest() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .completed(MutationDrainSummary(attempted: 1, applied: 1)))
        let sent = await f.transport.requests().filter { $0.method == .post }
        XCTAssertEqual(sent.count, 1)
        XCTAssertEqual(sent.first?.body, row.mutation.requestBody)
        let persisted = try await queueRecord(f)
        XCTAssertEqual(persisted.mutation, row.mutation)
        XCTAssertEqual(persisted.state, .applied)
    }
    func testDurableOrderAndUniqueLeasesAcrossSequentialBatch() async throws {
        let f = try await queueFixture(self)
        let first = try await queueEnqueued(f, mutation: queuePrepared(id: 19))
        let second = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        let gate = QueueGate()
        await f.transport.setSequence([
            try drainAppliedHTTP(first.mutation), try drainAppliedHTTP(second.mutation),
        ])
        await f.transport.suspendNextRequest(gate)
        let coordinator = drainCoordinator(f)
        let task = Task { await coordinator.drain(scope: f.scope, maximumOperations: 2) }
        await gate.wait()
        let firstSubmitting = try await queueRecord(f, id: 19)
        XCTAssertEqual(firstSubmitting.state, .submitting)
        XCTAssertEqual(firstSubmitting.attempt?.dispatchRecorded, true)
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .completed(MutationDrainSummary(attempted: 2, applied: 2)))
        let requests = await f.transport.requests().filter { $0.method == .post }
        XCTAssertEqual(
            requests.map(\.body), [first.mutation.requestBody, second.mutation.requestBody])
        let a = try await queueRecord(f, id: 19)
        let b = try await queueRecord(f, id: 11)
        XCTAssertNotEqual(a.attempt?.id, b.attempt?.id)
    }
    func testMaximumLimitLeavesLaterIndependentPending() async throws {
        let f = try await queueFixture(self)
        let first = try await queueEnqueued(f)
        _ = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        await f.transport.set(try drainAppliedHTTP(first.mutation))
        _ = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        let next = try await queueRecord(f, id: 11)
        XCTAssertEqual(next.state, .pending)
        await posts(f, 1)
    }
    func testInvalidLimitsFailClosed() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        for limit in [0, -1, 101, Int.max] {
            let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: limit)
            XCTAssertEqual(result, .stopped(.queue(.invalidLimit), MutationDrainSummary()))
        }
        await posts(f, 0)
    }
    func testWrongDeviceLibraryOwnerAndOriginCannotDrain() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        let scopes = try await [
            queueScope(device: 20), queueScope(library: 20),
            queueScope(owner: 20), queueScope(endpoint: "https://other.example"),
        ]
        for scope in scopes {
            let result = await drainCoordinator(f).drain(scope: scope, maximumOperations: 1)
            guard case .stopped = result else { return XCTFail("Wrong scope executed") }
        }
        await posts(f, 0)
    }
    func testMissingCheckpointBlocksEmptyAndPendingScopes() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .stopped(.queue(.syncBaseUnavailable), MutationDrainSummary()))
        await posts(f, 0)
    }
    func testInvalidCheckpointBlocksDrain() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try await f.database.invalidateBase(scope: f.scope)
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .stopped(.queue(.invalidCheckpoint), MutationDrainSummary()))
        await posts(f, 0)
    }
    func testReconciliationRequiredBaseBlocksDrain() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: f.scope, epoch: "2"))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .stopped(.queue(.reconciliationRequired), MutationDrainSummary()))
        await posts(f, 0)
    }
    func testMalformedCheckpointProvenanceCannotDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try queueRawSQL(f.url, "UPDATE sync_bases SET response=?", bindings: [Data("{}".utf8)])
        guard case .stopped = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        else { return XCTFail() }
        await posts(f, 0)
    }
    func testUnauthenticatedCannotDrain() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        await f.controller.requestLogout()
        guard case .stopped = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        else { return XCTFail() }
        await posts(f, 0)
    }
    func testUnavailableDatabaseFailsClosed() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try queueRawSQL(f.url, "DROP TABLE sync_bases")
        guard case .stopped = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        else { return XCTFail() }
        await posts(f, 0)
    }
    func testSameCoordinatorRejectsConcurrentDrainAndRecovery() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let gate = QueueGate()
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        await f.transport.suspendNextRequest(gate)
        let c = drainCoordinator(f)
        let first = Task { await c.drain(scope: f.scope, maximumOperations: 1) }
        await gate.wait()
        let duplicate = await c.drain(scope: f.scope, maximumOperations: 1)
        let recovery = await c.reconcileUnknown(scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(duplicate, .stopped(.queue(.concurrentExecution), MutationDrainSummary()))
        XCTAssertEqual(recovery, duplicate)
        await gate.release()
        _ = await first.value
        await posts(f, 1)
    }
    func testSeparateCoordinatorsCannotOverlapSameScope() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        _ = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        let gate = QueueGate()
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        await f.transport.suspendNextRequest(gate)
        let first = Task { await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1) }
        await gate.wait()
        let database = try MutationQueueSQLiteStore(url: f.url)
        let queue = DurableMutationQueue(
            store: database, provider: f.provider, bridge: QueueValidator())
        let duplicate = await drainCoordinator(f, queue: queue).drain(
            scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(duplicate, .stopped(.queue(.concurrentExecution), MutationDrainSummary()))
        await gate.release()
        _ = await first.value
        let later = try await queueRecord(f, id: 11)
        XCTAssertEqual(later.state, .pending)
        await posts(f, 1)
    }
    func testInitializationAndLocalCrashRecoveryDoNotPOST() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        _ = drainCoordinator(f)
        _ = await f.queue.recoverInterruptedOperations()
        let state = try await queueRecord(f)
        XCTAssertEqual(state.state, .outcomeUnknown)
        await posts(f, 0)
    }
    func testLeaseRollbackPreventsPOST() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await queueEnqueued(f)
        fault.arm(.afterSubmitting)
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .stopped(.queue(.diskFull), MutationDrainSummary()))
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .pending)
        await posts(f, 0)
    }
    func testLostLeaseCommitAcknowledgementPreventsPOST() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await queueEnqueued(f)
        fault.arm(.afterCommit)
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .stopped(.queue(.commitAcknowledgementLost), MutationDrainSummary()))
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .outcomeUnknown)
        XCTAssertEqual(row.evidence?.uncertainty, "LEASE_COMMIT_ACKNOWLEDGEMENT_LOST")
        await posts(f, 0)
    }
    func testSingleUseAuthorizerAndMissingAuthorizer() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        let global = AuthenticatedClientMutationRepository(
            provider: f.provider, bridge: QueueValidator())
        let denied = await global.submit(row.mutation)
        XCTAssertEqual(denied, .failed(.preparationRequired))
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let authorized = AuthenticatedClientMutationRepository(
            provider: f.provider, bridge: QueueValidator(),
            authorizer: DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease))
        guard case .applied = await authorized.submit(row.mutation) else { return XCTFail() }
        guard case .failed = await authorized.submit(row.mutation) else { return XCTFail() }
        await posts(f, 1)
    }
    func testWrongMutationCannotBorrowLease() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let wrong = try await queuePrepared(id: 11, node: 2)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let repo = AuthenticatedClientMutationRepository(
            provider: f.provider, bridge: QueueValidator(),
            authorizer: DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease))
        guard case .failed = await repo.submit(wrong) else { return XCTFail() }
        await posts(f, 0)
    }
    func testDeviceBearerRouteHasNoCookieCSRFOrBrowserMutation() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        _ = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        let request = await f.transport.requests().last!
        XCTAssertEqual(
            request.url.path,
            "/synveil/api/v1/devices/\(f.scope.deviceId.rawValue)/libraries/\(f.scope.libraryId.rawValue)/mutations"
        )
        XCTAssertEqual(request.url.scheme, "https")
        XCTAssertEqual(request.headers["Authorization"], "Bearer " + queueBearer)
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertNil(request.headers["X-CSRF-Token"])
        XCTAssertNil(request.url.query)
    }
    func testAppliedReplayJournalAndNoACK() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await f.transport.set(try drainAppliedHTTP(row.mutation, replayed: true))
        _ = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        let persisted = try await queueRecord(f)
        let evidence = try XCTUnwrap(persisted.evidence)
        let verified = try await ClientMutationResponseDecoder(bridge: QueueValidator()).decode(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: XCTUnwrap(evidence.responseBody)), for: row.mutation)
        guard case .applied(let value) = verified else { return XCTFail() }
        XCTAssertTrue(value.replayed)
        XCTAssertEqual(value.journalSequence.rawValue, "9007199254740993")
        XCTAssertEqual(value.journalEventId.rawValue, queueUUID(12))
        let base = try await f.queue.persistedBase(scope: f.scope)
        XCTAssertEqual(base.sequence.rawValue, "0")
        await posts(f, 1)
    }
    func testConflictPersistsAndStopsWithoutResolution() async throws {
        try await terminal(
            code: "mutation_conflict", state: .conflict, reason: .conflictRequiresReview)
    }
    func testIdentityConflictRemainsDistinct() async throws {
        try await terminal(
            code: "mutation_id_conflict", state: .failedPermanent, reason: .mutationIdConflict)
    }
    func testRebaselineAtomicallyBlocksScope() async throws {
        try await terminal(
            code: "sync_rebaseline_required", state: .blockedRebaseline, reason: .rebaselineRequired
        )
    }
    func testPermanentNotFoundDoesNotRevokeCredential() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        await f.transport.set(try drainErrorHTTP("not_found", status: 404))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(
            result,
            .stopped(
                .submission(.permanentRejection(.notFound)),
                MutationDrainSummary(attempted: 1, permanentRejections: 1)))
        XCTAssertEqual(f.controller.state, .authenticated)
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .failedPermanent)
        await posts(f, 1)
    }
    func test503PreservesQueueAndCredentialWithoutRetry() async throws {
        try await unknownResponse(drainErrorHTTP("dependency_unavailable", status: 503))
    }
    func testMalformedJSONIsUnknown() async throws {
        try await unknownResponse(
            HTTPTransportResponse(
                statusCode: 200,
                headers: ["Content-Type": "application/json"], body: Data("{".utf8)))
    }
    func testWrongMutationResponseIsUnknown() async throws {
        try await unknownResponse(drainAppliedHTTP(queuePrepared(id: 11)))
    }
    func testCrossLibraryResponseIsUnknown() async throws {
        try await unknownResponse(drainAppliedHTTP(queuePrepared(scope: queueScope(library: 81))))
    }
    func testWrongNodeResponseIsUnknown() async throws {
        try await unknownResponse(drainAppliedHTTP(queuePrepared(node: 2)))
    }
    func testTimeoutIsUnknownAndNotRetried() async throws { try await networkUnknown(.timeout) }
    func testConnectionResetIsUnknownAndNotRetried() async throws {
        try await networkUnknown(.malformedResponse)
    }
    func testTLSFailureIsUnknownAndNotRetried() async throws { try await networkUnknown(.tlsError) }
    func testOfflineIsUnknownAndNotRetried() async throws { try await networkUnknown(.offline) }
    func testUnknownExcludedFromNormalDrain() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: .outcomeUnknown(.timeout))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .stopped(.unknownRequiresReconciliation, MutationDrainSummary()))
        await posts(f, 0)
    }
    func testExplicitRecoveryUsesSameIDBytesBaseAndFreshLease() async throws {
        let f = try await uncertain()
        let old = try await queueRecord(f)
        await f.transport.set(try drainAppliedHTTP(old.mutation, replayed: true))
        let result = await drainCoordinator(f).reconcileUnknown(
            scope: f.scope, mutationId: old.mutation.id)
        XCTAssertEqual(result, .completed(MutationDrainSummary(attempted: 1, applied: 1)))
        let row = try await queueRecord(f)
        XCTAssertEqual(row.mutation, old.mutation)
        XCTAssertNotEqual(row.attempt?.id, old.attempt?.id)
        let requests = await f.transport.requests().filter { $0.method == .post }
        XCTAssertEqual(requests.map(\.body), [old.mutation.requestBody, old.mutation.requestBody])
        let history = try await f.database.attemptHistory(
            scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(history.first?.attempt, old.attempt)
        XCTAssertEqual(history.count, 1)
        let evidence = try JSONDecoder().decode(
            MutationOutcomeEvidence.self, from: XCTUnwrap(history.first?.evidence))
        XCTAssertEqual(evidence, old.evidence)
    }
    func testReplayedConflictRemainsConflict() async throws {
        let f = try await uncertain()
        let row = try await queueRecord(f)
        let conflict = try await queueConflict(row.mutation)
        let (_, bytes) = try await MutationPersistenceCodec(bridge: QueueValidator()).evidence(
            for: conflict, mutation: row.mutation)
        let evidence = try JSONDecoder().decode(MutationOutcomeEvidence.self, from: bytes)
        await f.transport.set(
            try HTTPTransportResponse(
                statusCode: 409, headers: ["Content-Type": "application/json"],
                body: XCTUnwrap(evidence.responseBody)))
        let result = await drainCoordinator(f).reconcileUnknown(
            scope: f.scope, mutationId: row.mutation.id)
        guard case .stopped(.conflictRequiresReview, let summary) = result else { return XCTFail() }
        XCTAssertEqual(summary.conflicts, 1)
        let persisted = try await queueRecord(f)
        XCTAssertEqual(persisted.evidence?.category, .conflict)
        XCTAssertEqual(persisted.state, .conflict)
    }
    func testAnotherAmbiguousRecoveryRemainsUnknown() async throws {
        let f = try await uncertain()
        let old = try await queueRecord(f)
        _ = await drainCoordinator(f).reconcileUnknown(scope: f.scope, mutationId: old.mutation.id)
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .outcomeUnknown)
        XCTAssertEqual(row.mutation, old.mutation)
        let history = try await f.database.attemptHistory(
            scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(history.count, 1)
        await posts(f, 2)
    }
    func testConcurrentExplicitRecoveriesCannotBothDispatch() async throws {
        let f = try await uncertain()
        let row = try await queueRecord(f)
        let gate = QueueGate()
        await f.transport.set(try drainAppliedHTTP(row.mutation, replayed: true))
        await f.transport.suspendNextRequest(gate)
        let first = Task {
            await drainCoordinator(f).reconcileUnknown(scope: f.scope, mutationId: row.mutation.id)
        }
        await gate.wait()
        let database = try MutationQueueSQLiteStore(url: f.url)
        let q = DurableMutationQueue(
            store: database, provider: f.provider, bridge: QueueValidator())
        let result = await drainCoordinator(f, queue: q).reconcileUnknown(
            scope: f.scope, mutationId: row.mutation.id)
        guard case .stopped(.queue(.invalidTransition), _) = result else { return XCTFail() }
        await gate.release()
        _ = await first.value
        await posts(f, 2)
    }
    func testRecoveryWrongScopeAndReplacementCredentialFailClosed() async throws {
        let f = try await uncertain()
        let old = try await queueRecord(f)
        let wrong = try await queueScope(library: 81)
        guard
            case .stopped = await drainCoordinator(f).reconcileUnknown(
                scope: wrong, mutationId: old.mutation.id)
        else { return XCTFail() }
        await f.credentials.replace(try queueSession(f.scope, credential: 93))
        guard
            case .stopped = await drainCoordinator(f).reconcileUnknown(
                scope: f.scope, mutationId: old.mutation.id)
        else { return XCTFail() }
        await posts(f, 1)
    }
    func testRecoveryRebaselineFromServerStopsWithoutChangingBase() async throws {
        let f = try await uncertain()
        let row = try await queueRecord(f)
        await f.transport.set(try drainErrorHTTP("sync_rebaseline_required"))
        _ = await drainCoordinator(f).reconcileUnknown(scope: f.scope, mutationId: row.mutation.id)
        let persisted = try await queueRecord(f)
        XCTAssertEqual(persisted.state, .blockedRebaseline)
        XCTAssertEqual(persisted.mutation.base, row.mutation.base)
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.status, .reconciliationRequired)
        guard
            case .stopped = await drainCoordinator(f).reconcileUnknown(
                scope: f.scope, mutationId: row.mutation.id)
        else { return XCTFail() }
        await posts(f, 2)
    }
    func testValidAppliedThenSQLiteFullNeverReportsSuccessOrResends() async throws {
        try await failedPersistence(.diskFull)
    }
    func testValidAppliedThenSQLiteBusyNeverReportsSuccessOrResends() async throws {
        try await failedPersistence(.busy)
    }
    func testValidAppliedThenCommitLostNeverReportsFalseRollback() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        let gate = QueueGate()
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        await f.transport.suspendNextRequest(gate)
        let task = Task { await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1) }
        await gate.wait()
        fault.arm(.afterCommit)
        await gate.release()
        let result = await task.value
        guard
            case .stopped(.resultPersistenceFailed(.commitAcknowledgementLost), let summary) =
                result
        else {
            return XCTFail()
        }
        XCTAssertEqual(summary.applied, 0)
        let persisted = try await queueRecord(f)
        XCTAssertEqual(persisted.state, .applied)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let restored = try await reopened.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(restored?.state, .applied)
        await posts(f, 1)
    }
    func testSQLiteAuthorizationFailurePreservesTypedCategoryAndNoPOST() async throws {
        for failure in [MutationQueueFailure.busy, .diskFull, .io] {
            let fault = QueueFaultInjector()
            let f = try await queueFixture(self, fault: fault)
            _ = try await queueEnqueued(f)
            fault.arm(.afterDispatchOwnership, failure: failure)
            let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
            XCTAssertEqual(
                result,
                .stopped(
                    .queue(failure), MutationDrainSummary(attempted: 1, localPreDispatchFailures: 1)
                ))
            let row = try await queueRecord(f)
            XCTAssertEqual(row.state, .outcomeUnknown)
            XCTAssertEqual(row.attempt?.dispatchRecorded, false)
            await posts(f, 0)
        }
    }
    func testDispatchCommitAcknowledgementLossIsDistinctFromNetworkUncertainty() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await queueEnqueued(f)
        let provider = DrainAuthorizationCommitLossProvider(base: f.provider, fault: fault)
        let result = await drainCoordinator(f, provider: provider).drain(
            scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(
            result,
            .stopped(
                .queue(.commitAcknowledgementLost),
                MutationDrainSummary(attempted: 1, localPreDispatchFailures: 1)))
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .outcomeUnknown)
        XCTAssertEqual(row.attempt?.dispatchRecorded, true)
        XCTAssertEqual(row.evidence?.uncertainty, "LOCAL_PRE_DISPATCH")
        await posts(f, 0)
    }

    func testCancellationBeforeLeaseDispatchesNothing() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        let gate = QueueGate()
        await f.credentials.suspendNextLoad(gate)
        let task = Task { await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1) }
        await gate.wait()
        task.cancel()
        await gate.release()
        guard case .stopped = await task.value else { return XCTFail() }
        await posts(f, 0)
    }
    func testCancellationAfterDispatchPersistsUnknown() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let gate = QueueGate()
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        await f.transport.suspendNextRequest(gate)
        let task = Task { await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1) }
        await gate.wait()
        task.cancel()
        await gate.release()
        guard case .stopped = await task.value else { return XCTFail() }
        let persisted = try await queueRecord(f)
        XCTAssertEqual(persisted.state, .outcomeUnknown)
        XCTAssertEqual(persisted.evidence?.uncertainty, "CANCELLED")
        await posts(f, 1)
    }
    func testLogoutDuringLeaseCapturePreventsPOST() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        let gate = QueueGate()
        await f.credentials.suspendNextLoad(gate)
        let task = Task { await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        guard case .stopped = await task.value else { return XCTFail() }
        await posts(f, 0)
    }
    func testLogoutDuringHTTPQuarantinesAndSuppressesAppliedPublication() async throws {
        try await changedSessionDuringHTTP(logout: true)
    }
    func testCredentialReplacementAfterPOSTCannotBorrowNewSession() async throws {
        try await changedSessionDuringHTTP(logout: false)
    }
    func testMalformedPersistedOperationBlocksPOST() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try queueRawSQL(f.url, "DROP TRIGGER mutation_immutable")
        try queueRawSQL(f.url, "UPDATE mutations SET request=?", bindings: [Data("{}".utf8)])
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .stopped(.queue(.malformedRecord), MutationDrainSummary()))
        await posts(f, 0)
    }
    func testPreDispatchFailureIsDistinctAndConservative() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        let provider = DrainRejectingProvider(base: f.provider)
        let result = await drainCoordinator(f, provider: provider).drain(
            scope: f.scope, maximumOperations: 1)
        guard case .stopped(.submission(.offline), let summary) = result else { return XCTFail() }
        XCTAssertEqual(summary.localPreDispatchFailures, 1)
        XCTAssertEqual(summary.unknown, 0)
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .outcomeUnknown)
        XCTAssertEqual(row.evidence?.uncertainty, "LOCAL_PRE_DISPATCH")
        await posts(f, 0)
    }

    func testCancellationAfterResultCommitKeepsAppliedButSuppressesPublication() async throws {
        try await changedSessionAfterCommit(cancel: true)
    }
    func testLogoutAfterResultCommitKeepsAppliedButSuppressesPublication() async throws {
        try await changedSessionAfterCommit(cancel: false)
    }
    func testAuthenticationRejectionStopsAndQuarantines() async throws {
        try await authenticationStop(
            code: "authentication_failed", status: 401, expected: .authenticationRejected)
    }
    func testDeviceRevocationStopsAndQuarantines() async throws {
        try await authenticationStop(code: "device_revoked", status: 403, expected: .deviceRevoked)
    }
    func testPermissionDeniedPersistsRejectionAndStopsBatch() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        _ = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        await f.transport.set(try drainErrorHTTP("permission_denied", status: 403))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 2)
        guard
            case .stopped(.submission(.permanentRejection(.permissionDenied)), let summary) = result
        else { return XCTFail() }
        XCTAssertEqual(summary.permanentRejections, 1)
        let later = try await queueRecord(f, id: 11)
        XCTAssertEqual(later.state, .pending)
        await posts(f, 1)
    }
    func testLostDispatchMarkerAcknowledgementDoesNotPOST() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        fault.arm(.afterCommit)
        let repo = AuthenticatedClientMutationRepository(
            provider: f.provider, bridge: QueueValidator(),
            authorizer: DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease))
        guard case .failed = await repo.submit(row.mutation) else { return XCTFail() }
        let committed = try await queueRecord(f)
        XCTAssertEqual(committed.attempt?.dispatchRecorded, true)
        await posts(f, 0)
    }
    func testCredentialReplacementBeforePOSTInvalidatesLease() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        await f.credentials.replace(try queueSession(f.scope, credential: 93))
        let repo = AuthenticatedClientMutationRepository(
            provider: f.provider, bridge: QueueValidator(),
            authorizer: DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease))
        guard case .failed = await repo.submit(row.mutation) else { return XCTFail() }
        await posts(f, 0)
    }
    func testExplicitRecoveryRejectsMissingRecord() async throws {
        let f = try await queueFixture(self)
        let id = try await ClientMutationId.validated(queueUUID(999), using: QueueValidator())
        let result = await drainCoordinator(f).reconcileUnknown(scope: f.scope, mutationId: id)
        XCTAssertEqual(result, .stopped(.queue(.notFound), MutationDrainSummary()))
        await posts(f, 0)
    }
    func testUnknownWithChangedLocalBaseCannotReconcile() async throws {
        let f = try await uncertain()
        let row = try await queueRecord(f)
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: f.scope, epoch: "2"))
        let result = await drainCoordinator(f).reconcileUnknown(
            scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .stopped(.queue(.reconciliationRequired), MutationDrainSummary()))
        await posts(f, 1)
    }
    func testConflictEvidenceKeepsReplayAndServerDetails() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let codec = MutationPersistenceCodec(bridge: QueueValidator())
        let expected = try await queueConflict(row.mutation)
        let (_, bytes) = try await codec.evidence(for: expected, mutation: row.mutation)
        let projection = try JSONDecoder().decode(MutationOutcomeEvidence.self, from: bytes)
        await f.transport.set(
            try HTTPTransportResponse(
                statusCode: 409,
                headers: ["Content-Type": "application/json"],
                body: XCTUnwrap(projection.responseBody)))
        _ = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        let stored = try await queueRecord(f)
        XCTAssertEqual(stored.evidence, projection)
        XCTAssertEqual(stored.mutation, row.mutation)
        await posts(f, 1)
    }

    func testLogoutAfterLeaseCommitBeforeAcknowledgementPreventsPOST() async throws {
        try await changedSessionAfterLeaseCommit(cancel: false)
    }
    func testCancellationAfterLeaseCommitBeforeAcknowledgementPreventsPOST() async throws {
        try await changedSessionAfterLeaseCommit(cancel: true)
    }
    func testCancellationDuringLocalResultPersistenceDoesNotUndoCommit() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let gate = QueueGate()
        let delayed = QueueDelayedStorage(
            base: f.database, gate: gate, delayResult: true, resultBeforeCommit: true)
        let queue = DurableMutationQueue(
            store: delayed, provider: f.provider, bridge: QueueValidator())
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        let task = Task {
            await drainCoordinator(f, queue: queue).drain(scope: f.scope, maximumOperations: 1)
        }
        await gate.wait()
        let before = try await f.database.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(before?.state, .submitting)
        task.cancel()
        await gate.release()
        guard case .stopped(_, let summary) = await task.value else { return XCTFail() }
        XCTAssertEqual(summary.applied, 0)
        let after = try await f.database.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(after?.state, .applied)
        await posts(f, 1)
    }

    private func changedSessionAfterLeaseCommit(cancel: Bool) async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let gate = QueueGate()
        let delayed = QueueDelayedStorage(base: f.database, gate: gate, delayLease: true)
        let queue = DurableMutationQueue(
            store: delayed, provider: f.provider, bridge: QueueValidator())
        let task = Task {
            await drainCoordinator(f, queue: queue).drain(scope: f.scope, maximumOperations: 1)
        }
        await gate.wait()
        let committed = try await f.database.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(committed?.state, .submitting)
        XCTAssertEqual(committed?.attempt?.dispatchRecorded, false)
        if cancel { task.cancel() } else { await f.controller.requestLogout() }
        await gate.release()
        guard case .stopped = await task.value else { return XCTFail() }
        let settled = try await f.database.record(scope: f.scope, id: row.mutation.id.rawValue)
        if cancel { XCTAssertEqual(settled?.state, .outcomeUnknown) }
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        _ = try await reopened.recoverInterruptedOperations()
        let recovered = try await reopened.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(recovered?.state, .outcomeUnknown)
        XCTAssertEqual(recovered?.request, row.mutation.requestBody)
        await posts(f, 0)
    }

    private func changedSessionAfterCommit(cancel: Bool) async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let gate = QueueGate()
        let delayed = QueueDelayedStorage(base: f.database, gate: gate, delayResult: true)
        let queue = DurableMutationQueue(
            store: delayed, provider: f.provider, bridge: QueueValidator())
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        let task = Task {
            await drainCoordinator(f, queue: queue).drain(scope: f.scope, maximumOperations: 1)
        }
        await gate.wait()
        let committed = try await f.database.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(committed?.state, .applied)
        if cancel { task.cancel() } else { await f.controller.requestLogout() }
        await gate.release()
        guard case .stopped(_, let summary) = await task.value else { return XCTFail() }
        XCTAssertEqual(summary.applied, 0)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        _ = try await reopened.recoverInterruptedOperations()
        let terminal = try await reopened.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(terminal?.state, .applied)
        await posts(f, 1)
    }
    private func authenticationStop(code: String, status: Int, expected: ClientMutationFailure)
        async throws
    {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        await f.transport.set(try drainErrorHTTP(code, status: status))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        guard case .stopped(.submission(let failure), _) = result else { return XCTFail() }
        XCTAssertEqual(failure, expected)
        XCTAssertNotEqual(f.controller.state, .authenticated)
        _ = await f.queue.pending(scope: f.scope, limit: 1)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
        await posts(f, 1)
    }

    private func posts(
        _ f: QueueFixture, _ count: Int, file: StaticString = #filePath, line: UInt = #line
    ) async {
        let requests = await f.transport.requests().filter { $0.method == .post }
        XCTAssertEqual(requests.count, count, file: file, line: line)
    }
    private func terminal(code: String, state: MutationQueueState, reason: MutationDrainStopReason)
        async throws
    {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        _ = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        await f.transport.set(try drainErrorHTTP(code))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 2)
        guard case .stopped(let actual, _) = result else { return XCTFail() }
        XCTAssertEqual(actual, reason)
        let stored = try await queueRecord(f)
        XCTAssertEqual(stored.state, state)
        XCTAssertEqual(stored.mutation, row.mutation)
        let later = try await queueRecord(f, id: 11)
        XCTAssertEqual(later.state, .pending)
        if state == .blockedRebaseline {
            let base = try await f.database.syncBase(scope: f.scope)
            XCTAssertEqual(base?.status, .reconciliationRequired)
        }
        await posts(f, 1)
    }
    private func unknownResponse(_ response: HTTPTransportResponse) async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        await f.transport.set(response)
        guard case .stopped = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        else { return XCTFail() }
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .outcomeUnknown)
        XCTAssertEqual(f.controller.state, .authenticated)
        await posts(f, 1)
    }
    private func networkUnknown(_ error: SynveilTransportError) async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        await f.transport.fail(error)
        guard case .stopped = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        else { return XCTFail() }
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .outcomeUnknown)
        await posts(f, 1)
    }
    private func uncertain() async throws -> QueueFixture {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        await f.transport.fail(.timeout)
        _ = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        return f
    }
    private func failedPersistence(_ failure: MutationQueueFailure) async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        fault.arm(.beforeResultPersistence, failure: failure)
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        guard case .stopped(.resultPersistenceFailed(let actual), let summary) = result else {
            return XCTFail()
        }
        XCTAssertEqual(actual, failure)
        XCTAssertEqual(summary.applied, 0)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let stored = try await reopened.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(stored?.state, .submitting)
        _ = try await reopened.recoverInterruptedOperations()
        let recovered = try await reopened.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(recovered?.state, .outcomeUnknown)
        await posts(f, 1)
    }
    private func changedSessionDuringHTTP(logout: Bool) async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let gate = QueueGate()
        await f.transport.set(try drainAppliedHTTP(row.mutation))
        await f.transport.suspendNextRequest(gate)
        let task = Task { await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1) }
        await gate.wait()
        if logout {
            await f.controller.requestLogout()
        } else {
            await f.credentials.replace(try queueSession(f.scope, credential: 93))
        }
        await gate.release()
        guard case .stopped(_, let summary) = await task.value else { return XCTFail() }
        XCTAssertEqual(summary.applied, 0)
        let stored = try await f.database.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(stored?.state, .submitting)
        await posts(f, 1)
    }
}

@MainActor
private final class DrainRejectingProvider: AuthenticatedClientMutationRequestProviderProtocol {
    let base: AuthenticatedLibraryRequestProvider
    init(base: AuthenticatedLibraryRequestProvider) { self.base = base }
    func begin() async throws -> LibraryRequestScope { try await base.begin() }
    func validate(_ scope: LibraryRequestScope) async throws { try await base.validate(scope) }
    func submitMutation(
        _ mutation: PreparedClientMutation, scope: LibraryRequestScope,
        onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse { throw SynveilTransportError.offline }
    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope) {
        base.handle(failure, scope: scope)
    }
}

@MainActor
private final class DrainAuthorizationCommitLossProvider:
    AuthenticatedClientMutationRequestProviderProtocol
{
    let base: AuthenticatedLibraryRequestProvider
    let fault: QueueFaultInjector
    var armed = false
    init(base: AuthenticatedLibraryRequestProvider, fault: QueueFaultInjector) {
        self.base = base
        self.fault = fault
    }
    func begin() async throws -> LibraryRequestScope { try await base.begin() }
    func validate(_ scope: LibraryRequestScope) async throws {
        try await base.validate(scope)
        if !armed {
            armed = true
            fault.arm(.afterCommit)
        }
    }
    func submitMutation(
        _ mutation: PreparedClientMutation, scope: LibraryRequestScope,
        onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse {
        try await base.submitMutation(mutation, scope: scope, onDispatch: onDispatch)
    }
    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope) {
        base.handle(failure, scope: scope)
    }
}

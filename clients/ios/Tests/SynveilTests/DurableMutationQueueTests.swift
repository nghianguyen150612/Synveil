import Foundation
import SQLite3
import XCTest

@testable import Synveil

@MainActor
final class DurableMutationQueueTests: XCTestCase {
    func testDurablePendingRecordPreservesAllImmutableFields() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        XCTAssertEqual(row.state, .pending)
        XCTAssertNil(row.attempt)
        XCTAssertNil(row.evidence)
        XCTAssertEqual(row.mutation.id.rawValue, queueUUID(10))
        XCTAssertEqual(row.mutation.base.epoch.rawValue, "1")
        XCTAssertEqual(row.mutation.base.sequence.rawValue, "0")
        let expected = try await queuePrepared()
        XCTAssertEqual(row.mutation.payload.intent, expected.payload.intent)
        let reread = try await queueRecord(f)
        XCTAssertEqual(reread, row)
    }
    func testRealRustIdentitySurvivesRehydration() async throws {
        let f = try await queueFixture(self)
        let bridge = try await RustBridgeAsyncAdapter()
        let mutation = try await queuePrepared(bridge: bridge)
        _ = try await queueEnqueued(f, mutation: mutation)
        let stored = try await f.database.record(scope: f.scope, id: mutation.id.rawValue)
        let raw = try XCTUnwrap(stored)
        let row = try await MutationPersistenceCodec(bridge: bridge).rehydrate(raw)
        XCTAssertEqual(row.mutation, mutation)
    }
    func testEveryKindRehydratesUsingOriginalEncoder() async throws {
        let f = try await queueFixture(self)
        let node = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let revision = try NodeRevision(validating: "9007199254740993")
        let intents: [ClientMutationIntent] = [
            .createDirectory(
                parentNodeId: parent, expectedParentRevision: revision, name: "Escaped \"/🚀"),
            .renameNode(nodeId: node, expectedRevision: revision, newName: "Escaped \"/🚀"),
            .moveNode(
                nodeId: node, expectedRevision: revision, newParentNodeId: parent,
                expectedNewParentRevision: revision),
            .trashNode(nodeId: node, expectedRevision: revision),
            .restoreNode(
                nodeId: node, expectedRevision: revision, expectedParentNodeId: parent,
                expectedParentRevision: revision),
        ]
        for (index, intent) in intents.enumerated() {
            let scope = try await queueScope(library: 80 + index)
            _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: scope))
            let mutation = try await queuePrepared(id: 10 + index, scope: scope, intent: intent)
            let (stored, _) = try await f.database.enqueue(
                mutation,
                payload: MutationPersistenceCodec(bridge: QueueValidator()).payloadBytes(mutation))
            let row = try await MutationPersistenceCodec(bridge: QueueValidator()).rehydrate(stored)
            XCTAssertEqual(row.mutation, mutation)
            XCTAssertEqual(row.mutation.requestBody, mutation.requestBody)
        }
    }
    func testExactLargeBaseSurvivesDatabaseTextStorage() async throws {
        let f = try await queueFixture(self)
        let large = "18446744073709551615"
        await f.transport.set(
            try queueHTTP(queueCheckpointObject(scope: f.scope, epoch: large, sequence: large)))
        _ = await f.service.prepare(scope: f.scope)
        let row = try await queueEnqueued(f, mutation: queuePrepared(epoch: large, sequence: large))
        let restored = try await queueRecord(f)
        XCTAssertEqual(restored.mutation, row.mutation)
        XCTAssertEqual(restored.mutation.base.sequence.rawValue, large)
    }
    func testCorruptPayloadRejectedWithoutRepair() async throws {
        try await corrupted(column: "payload", bytes: Data("{}".utf8))
    }
    func testTruncatedRequestRejectedWithoutRepair() async throws {
        try await corrupted(column: "request", bytes: Data("{".utf8))
    }
    func testNoncanonicalStoredRequestRejectedWithoutRepair() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        try queueRawSQL(f.url, "DROP TRIGGER mutation_immutable")
        var changed = row.mutation.requestBody
        changed.append(32)
        try queueRawSQL(f.url, "UPDATE mutations SET request=?", bindings: [changed])
        let result = await f.queue.get(scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .failed(.malformedRecord))
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT length(request) FROM mutations"),
            String(changed.count))
    }
    func testUnsupportedEncodingVersionFailsClosed() async throws {
        try await corruptedScalar("encoding_version=2")
    }
    func testUnsupportedMutationKindFailsClosed() async throws {
        try await corruptedScalar("kind='UPLOAD'")
    }
    func testInvalidStoredUUIDFailsClosed() async throws {
        try await corruptedScalar("mutation_id='invalid'")
    }
    func testStoredBaseDisagreementFailsClosed() async throws {
        try await corruptedScalar("epoch='2'")
    }
    func testDependencyMetadataTamperingRejected() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        try queueRawSQL(f.url, "DELETE FROM dependencies")
        let result = await f.queue.get(scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .failed(.malformedRecord))
    }
    private func corrupted(column: String, bytes: Data) async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        try queueRawSQL(f.url, "DROP TRIGGER mutation_immutable")
        // column comes from the two fixed test cases above, never user input.
        try queueRawSQL(f.url, "UPDATE mutations SET \(column)=?", bindings: [bytes])
        let result = await f.queue.get(scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .failed(.malformedRecord))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }
    private func corruptedScalar(_ assignment: String) async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        try queueRawSQL(f.url, "DROP TRIGGER mutation_immutable")
        // Test corruption bypasses constraints on a disposable database only.
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(f.url.path, &db), SQLITE_OK)
        let handle = try XCTUnwrap(db)
        defer { sqlite3_close(handle) }
        XCTAssertEqual(
            sqlite3_exec(handle, "PRAGMA ignore_check_constraints=ON", nil, nil, nil), SQLITE_OK)
        XCTAssertEqual(
            sqlite3_exec(handle, "UPDATE mutations SET \(assignment)", nil, nil, nil), SQLITE_OK)
        let result = await f.queue.pending(scope: f.scope, limit: 10)
        XCTAssertEqual(result, .failed(.malformedRecord))
        _ = row
    }
    func testSubmittingTransitionRecordsAttemptBeforeTransport() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .submitting)
        XCTAssertNotNil(current.attempt)
        XCTAssertEqual(current.attempt?.dispatchRecorded, false)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testAppliedTransitionPersistsAuthoritativeProjection() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let result = try await queueApplied(row.mutation)
        try await f.queue.finish(lease, result: result)
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .applied)
        XCTAssertEqual(current.evidence?.category, .applied)
        let evidence = try XCTUnwrap(current.evidence)
        let decoded = try await ClientMutationResponseDecoder(bridge: QueueValidator()).decode(
            HTTPTransportResponse(
                statusCode: evidence.responseStatus!, headers: ["Content-Type": "application/json"],
                body: evidence.responseBody!), for: row.mutation)
        XCTAssertEqual(decoded, result)
        let second = try MutationQueueSQLiteStore(url: f.url)
        let stored = try await second.record(scope: f.scope, id: row.mutation.id.rawValue)
        let raw = try XCTUnwrap(stored)
        let reread = try await MutationPersistenceCodec(bridge: QueueValidator()).rehydrate(raw)
        XCTAssertEqual(reread, current)
    }
    func testAppliedDoesNotAdvanceAcknowledgedCheckpoint() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: queueApplied(row.mutation))
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.sequence, "0")
        XCTAssertEqual(base?.epoch, "1")
    }
    func testConflictEvidencePreservesHistoricalObservation() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let result = try await queueConflict(row.mutation)
        try await f.queue.finish(lease, result: result)
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .conflict)
        let evidence = try XCTUnwrap(current.evidence)
        let decoded = try await ClientMutationResponseDecoder(bridge: QueueValidator()).decode(
            HTTPTransportResponse(
                statusCode: evidence.responseStatus!, headers: ["Content-Type": "application/json"],
                body: evidence.responseBody!), for: row.mutation)
        XCTAssertEqual(decoded, result)
        XCTAssertFalse(
            String(data: evidence.responseBody!, encoding: .utf8)!.contains(
                "Do not persist raw diagnostics"))
    }
    func testConflictWithoutOptionalDetailsRemainsDurable() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: queueConflict(row.mutation, details: false))
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .conflict)
    }
    func testOutcomeUnknownRetainsOriginalOperationAndAttempt() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: original.mutation.id)
        try await f.queue.finish(lease, result: .outcomeUnknown(.timeout))
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .outcomeUnknown)
        XCTAssertEqual(current.mutation, original.mutation)
        XCTAssertNotNil(current.attempt)
        XCTAssertEqual(current.evidence?.uncertainty, "UNVERIFIED_OUTCOME")
    }
    func testRebaselineBlocksAndPreservesOriginalBase() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: original.mutation.id)
        try await f.queue.finish(
            lease, result: .rebaselineRequired(requestId: "mutation-request-01"))
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .blockedRebaseline)
        XCTAssertEqual(current.mutation, original.mutation)
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.sequence, "0")
        XCTAssertEqual(base?.status, .reconciliationRequired)
    }
    func testPermanentRejectionIsTerminal() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: original.mutation.id)
        try await f.queue.finish(lease, result: .failed(.permanentRejection(.notFound)))
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .failedPermanent)
        XCTAssertEqual(current.evidence?.rejection, "not_found")
        await queueAssertFailure(.invalidTransition) {
            try await f.queue.acquireAttempt(scope: f.scope, mutationId: original.mutation.id)
        }
    }
    func testMutationIdConflictIsTerminalWithoutReplacementIdentity() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(
            scope: f.scope, mutationId: original.mutation.id)
        try await f.queue.finish(
            lease, result: .mutationIdConflict(requestId: "mutation-request-01"))
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .failedPermanent)
        XCTAssertEqual(current.mutation.id, original.mutation.id)
        XCTAssertEqual(current.evidence?.category, .identityConflict)
    }
    func testInvalidAndTerminalTransitionGraph() {
        for source in MutationQueueState.allCases {
            for target in MutationQueueState.allCases {
                let expected =
                    source == .pending && target == .submitting
                    || source == .submitting && ![.pending, .submitting].contains(target)
                XCTAssertEqual(source.permits(target), expected)
            }
        }
    }
    func testDuplicateCompletionCannotOverwriteAppliedEvidence() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: queueApplied(row.mutation))
        let before = try await queueRecord(f)
        await queueAssertFailure(.invalidTransition) {
            try await f.queue.finish(lease, result: .outcomeUnknown(.timeout))
        }
        let after = try await queueRecord(f)
        XCTAssertEqual(after, before)
    }
    func testInterruptedSubmittingRecoveryAcrossNewConnection() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let previous = try await queueRecord(f)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let recovered = try await reopened.recoverInterruptedOperations()
        XCTAssertEqual(recovered, 1)
        let stored = try await reopened.record(scope: f.scope, id: row.mutation.id.rawValue)
        let raw = try XCTUnwrap(stored)
        let current = try await MutationPersistenceCodec(bridge: QueueValidator()).rehydrate(raw)
        XCTAssertEqual(current.state, .outcomeUnknown)
        XCTAssertEqual(current.mutation, row.mutation)
        XCTAssertEqual(current.attempt, previous.attempt)
        XCTAssertEqual(current.evidence?.uncertainty, "INTERRUPTED")
    }
    func testRecoveryIsIdempotentAndNeverPosts() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let first = await f.queue.recoverInterruptedOperations()
        XCTAssertEqual(first, .recovered(1))
        let second = await f.queue.recoverInterruptedOperations()
        XCTAssertEqual(second, .recovered(0))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
        let current = try await queueRecord(f)
        XCTAssertEqual(current.mutation.id, row.mutation.id)
    }
    func testUnknownOperationCannotAcquireAutomaticReplayLease() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: .outcomeUnknown(.cancelled))
        await queueAssertFailure(.invalidTransition) {
            try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        }
    }
    func testCancelledAttemptDoesNotClaimRollback() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: .failed(.cancelled))
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .outcomeUnknown)
        XCTAssertEqual(current.evidence?.uncertainty, "CANCELLED")
    }
    func testFaultAfterSubmittingBeforeCommitLeavesPending() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        fault.arm(.afterSubmitting)
        await queueAssertFailure(.diskFull) {
            try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        }
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .pending)
        XCTAssertNil(current.attempt)
    }
    func testFaultAfterDispatchOwnershipRollsBackMarkerPreservingUncertainty() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        fault.arm(.afterDispatchOwnership)
        let authorizer = DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
        await queueAssertFailure(.diskFull) {
            try await authorizer.authorizePersistedSubmission(row.mutation)
        }
        let before = try await queueRecord(f)
        XCTAssertEqual(before.attempt?.dispatchRecorded, false)
        _ = await f.queue.recoverInterruptedOperations()
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .outcomeUnknown)
    }
    func testFailureBeforeAppliedPersistenceRetainsSubmittingThenRecoversUnknown() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let result = try await queueApplied(row.mutation)
        fault.arm(.beforeResultPersistence)
        await queueAssertFailure(.diskFull) { try await f.queue.finish(lease, result: result) }
        let before = try await queueRecord(f)
        XCTAssertEqual(before.state, .submitting)
        _ = await f.queue.recoverInterruptedOperations()
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .outcomeUnknown)
    }
    func testFailureDuringConflictPersistenceRollsBackResult() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let result = try await queueConflict(row.mutation)
        fault.arm(.duringConflictPersistence)
        await queueAssertFailure(.diskFull) { try await f.queue.finish(lease, result: result) }
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .submitting)
        XCTAssertNil(current.evidence)
    }
    func testSharedDestinationParentDependencyIsBlocked() async throws {
        let f = try await queueFixture(self)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let revision = try NodeRevision(validating: "3")
        let first = try await queuePrepared(
            intent: .createDirectory(
                parentNodeId: parent, expectedParentRevision: revision, name: "A"))
        _ = try await queueEnqueued(f, mutation: first)
        let node = try await NodeId.validated(queueUUID(3), using: QueueValidator())
        let second = try await queuePrepared(
            id: 11,
            intent: .moveNode(
                nodeId: node, expectedRevision: revision, newParentNodeId: parent,
                expectedNewParentRevision: revision))
        let result = await f.queue.enqueue(second)
        XCTAssertEqual(result, .failed(.dependencyConflict))
    }
    func testConflictAndUnknownContinueBlockingDependencies() async throws {
        for result in [
            ClientMutationSubmissionResult.outcomeUnknown(.timeout),
            .conflict(nil, requestId: "mutation-request-01"),
        ] {
            let f = try await queueFixture(self)
            let row = try await queueEnqueued(f)
            let lease = try await f.queue.acquireAttempt(
                scope: f.scope, mutationId: row.mutation.id)
            try await f.queue.finish(lease, result: result)
            let next = await f.queue.enqueue(try await queuePrepared(id: 11))
            XCTAssertEqual(next, .failed(.dependencyConflict))
        }
    }
    func testIndependentResourceOperationsKeepDistinctIdentity() async throws {
        let f = try await queueFixture(self)
        let first = try await queueEnqueued(f)
        let second = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        XCTAssertNotEqual(first.mutation.id, second.mutation.id)
        XCTAssertLessThan(first.enqueueOrder, second.enqueueOrder)
    }
    func testQueueReadsAreBoundedAndRejectInvalidLimits() async throws {
        let f = try await queueFixture(self)
        for id in 10..<14 {
            _ = try await queueEnqueued(f, mutation: queuePrepared(id: id, node: id))
        }
        let result = await f.queue.pending(scope: f.scope, limit: 2)
        guard case .records(let rows) = result else { return XCTFail() }
        XCTAssertEqual(rows.count, 2)
        for limit in [0, -1, 101] {
            let result = await f.queue.pending(scope: f.scope, limit: limit)
            XCTAssertEqual(result, .failed(.invalidLimit))
        }
    }
    func testWrongOwnerQueueAccessRejected() async throws {
        try await wrongScope(queueScope(owner: 99))
    }
    func testWrongDeviceQueueAccessRejected() async throws {
        try await wrongScope(queueScope(device: 99))
    }
    func testWrongOriginQueueAccessRejected() async throws {
        try await wrongScope(queueScope(endpoint: "https://other.example"))
    }
    func testWrongLibraryHasNoActionableOriginalRecord() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let scope = try await queueScope(library: 99)
        let result = await f.queue.get(scope: scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .missing)
        await queueAssertFailure(.syncBaseUnavailable) {
            try await f.queue.acquireAttempt(scope: scope, mutationId: row.mutation.id)
        }
    }
    private func wrongScope(_ scope: ClientMutationScope) async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let result = await f.queue.get(scope: scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .failed(.scopeMismatch))
    }
    func testLogoutInvalidatesLeaseAndQuarantinesWithoutDeletion() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        await f.controller.requestLogout()
        await queueAssertFailure(.ownershipRequired) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
        let result = await f.queue.get(scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .failed(.unauthenticated))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
    }
    func testLogoutQuarantinesPersistedScopesNotReadInCurrentQueueInstance() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        let coldQueue = DurableMutationQueue(
            store: f.database, provider: f.provider, bridge: QueueValidator())
        f.controller.installMutationSessionInvalidator { [weak coldQueue] in
            coldQueue?.invalidateSession()
        }
        await f.controller.requestLogout()
        let id = try await ClientMutationId.validated(queueUUID(10), using: QueueValidator())
        let result = await coldQueue.get(scope: f.scope, mutationId: id)
        XCTAssertEqual(result, .failed(.unauthenticated))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }

    func testReplacementCredentialCannotInheritQuarantinedQueue() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await f.controller.requestLogout()
        let session = try queueSession(
            f.scope, credential: 93, bearer: "svd1_" + String(repeating: "b", count: 64))
        await f.credentials.replace(session)
        f.controller.requireEnrollment()
        f.controller.markAuthenticated(after: SecureCredentialPersistenceReceipt(session: session))
        let result = await f.queue.get(scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .failed(.scopeMismatch))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }
    func testCredentialReplacementInvalidatesAuthorizationEvenWithSameCredentialId() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        await f.credentials.replace(
            try queueSession(f.scope, bearer: "svd1_" + String(repeating: "b", count: 64)))
        do {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
            XCTFail()
        } catch { XCTAssertEqual(error as? LibraryFailure, .staleSession) }
        let current = try await f.database.record(scope: f.scope, id: row.mutation.id.rawValue)
        XCTAssertEqual(current?.attempt?.dispatchRecorded, false)
    }
    func testCredentialReplacementBetweenCaptureChecksIsFenced() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await f.credentials.replaceAfterNextLoad(
            try queueSession(f.scope, bearer: "svd1_" + String(repeating: "b", count: 64)))
        let result = await f.queue.get(scope: f.scope, mutationId: row.mutation.id)
        XCTAssertEqual(result, .failed(.staleSession))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
    }

    func testStaleLifecycleRevisionRejectedAfterRecovery() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        f.controller.requireRecovery(.authentication)
        await queueAssertFailure(.ownershipRequired) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
    }
    func testRevocationInvalidatesSubmissionOwnership() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let session = try await f.provider.begin()
        f.provider.handle(.deviceRevoked, scope: session)
        await queueAssertFailure(.ownershipRequired) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testAuthorizerConsumesExactlyOnePersistedDispatchCapability() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let authorizer = DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
        try await authorizer.authorizePersistedSubmission(row.mutation)
        let current = try await queueRecord(f)
        XCTAssertEqual(current.attempt?.dispatchRecorded, true)
        await queueAssertFailure(.ownershipRequired) {
            try await authorizer.authorizePersistedSubmission(row.mutation)
        }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testAuthorizerRejectsMissingPersistedRow() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try queueRawSQL(f.url, "DELETE FROM dependencies")
        try queueRawSQL(f.url, "DELETE FROM mutations")
        await queueAssertFailure(.notFound) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
    }
    func testAuthorizerRejectsModifiedRequestBytes() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try queueRawSQL(f.url, "DROP TRIGGER mutation_immutable")
        try queueRawSQL(f.url, "UPDATE mutations SET request=?", bindings: [Data("{}".utf8)])
        await queueAssertFailure(.malformedRecord) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
    }
    func testAuthorizerRejectsChangedMutationIdentity() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let changed = try await queuePrepared(id: 99)
        await queueAssertFailure(.ownershipRequired) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(changed)
        }
    }
    func testAuthorizerRejectsChangedBaseProvenance() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try queueRawSQL(f.url, "UPDATE sync_bases SET sequence='2'")
        await queueAssertFailure(.invalidCheckpoint) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
    }
    func testAuthorizerRejectsTerminalRecord() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: queueApplied(row.mutation))
        await queueAssertFailure(.ownershipRequired) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
    }
    func testStorageAuthorizerRejectsMissingAttemptOwnership() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await queueAssertFailure(.ownershipRequired) {
            try await f.database.authorizeAttempt(
                row.mutation, owner: UUID().uuidString, attemptId: UUID().uuidString)
        }
    }
    func testConcurrentAttemptsCannotBothOwnRecord() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let results = await withTaskGroup(of: Bool.self) { group in
            for _ in 0..<8 {
                group.addTask {
                    do {
                        _ = try await f.queue.acquireAttempt(
                            scope: f.scope, mutationId: row.mutation.id)
                        return true
                    } catch { return false }
                }
            }
            var results: [Bool] = []
            for await result in group { results.append(result) }
            return results
        }
        XCTAssertEqual(results.filter { $0 }.count, 1)
    }
    func testRecoveryInvalidatesPreviouslyLiveLease() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        _ = await f.queue.recoverInterruptedOperations()
        await queueAssertFailure(.ownershipRequired) {
            try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
                .authorizePersistedSubmission(row.mutation)
        }
    }
    func testInvalidAppliedIdentityCannotBePersisted() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let other = try await queuePrepared(id: 99)
        do {
            try await f.queue.finish(lease, result: queueApplied(other))
            XCTFail()
        } catch {}
        let current = try await queueRecord(f)
        XCTAssertEqual(current.state, .submitting)
    }
    func testPreparationAndQueueNeverEnableProductionPostGate() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let repository = AuthenticatedClientMutationRepository(
            provider: f.provider, bridge: QueueValidator())
        let result = await repository.submit(row.mutation)
        XCTAssertEqual(result, .failed(.preparationRequired))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testCancellationAfterCommitReportsCommittedSessionChange() async throws {
        let f = try await queueFixture(self)
        let gate = QueueGate()
        let queue = DurableMutationQueue(
            store: QueueDelayedStorage(base: f.database, gate: gate), provider: f.provider,
            bridge: QueueValidator())
        let mutation = try await queuePrepared()
        let task = Task { await queue.enqueue(mutation) }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.committedButSessionChanged))
        let raw = try await f.database.record(scope: f.scope, id: mutation.id.rawValue)
        XCTAssertEqual(raw?.request, mutation.requestBody)
        XCTAssertEqual(raw?.state, .pending)
    }
    func testLogoutAfterCommitQuarantinesWithoutReportingRollback() async throws {
        let f = try await queueFixture(self)
        let gate = QueueGate()
        let queue = DurableMutationQueue(
            store: QueueDelayedStorage(base: f.database, gate: gate), provider: f.provider,
            bridge: QueueValidator())
        f.controller.installMutationSessionInvalidator { [weak queue] in queue?.invalidateSession()
        }
        let mutation = try await queuePrepared()
        let task = Task { await queue.enqueue(mutation) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.committedButSessionChanged))
        let lookup = await queue.get(scope: f.scope, mutationId: mutation.id)
        XCTAssertEqual(lookup, .failed(.unauthenticated))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
    }
    func testCommitAcknowledgementLossQueueReadsBackExactOperation() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        fault.arm(.afterCommit)
        let mutation = try await queuePrepared()
        let result = await f.queue.enqueue(mutation)
        guard case .enqueued(let record) = result else { return XCTFail() }
        XCTAssertEqual(record.mutation, mutation)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }

    func testRedactedDebugAndTypedErrorsDoNotExposeNamesOrSecrets() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        for value in [
            String(describing: row), String(reflecting: row), String(describing: row.mutation),
            String(describing: MutationQueueFailure.io),
        ] {
            XCTAssertFalse(value.contains("Private logical"))
            XCTAssertFalse(value.contains(queueBearer))
        }
    }
}

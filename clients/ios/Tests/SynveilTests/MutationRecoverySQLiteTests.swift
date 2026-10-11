import Foundation
import XCTest

@testable import Synveil

@MainActor
final class MutationRecoverySQLiteTests: XCTestCase {
    func testVersionOneMigrationPreservesPendingIdentityAndBytes() async throws {
        let f = try await queueFixture(self)
        let old = try await queueEnqueued(f)
        try downgrade(f)
        let migrated = try MutationQueueSQLiteStore(url: f.url)
        let raw = try await migrated.record(scope: f.scope, id: old.mutation.id.rawValue)
        let row = try await MutationPersistenceCodec(bridge: QueueValidator()).rehydrate(
            XCTUnwrap(raw))
        XCTAssertEqual(row, old)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "5")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT count(*) FROM mutation_attempt_history"), "0")
    }
    func testVersionOneMigrationPreservesInterruptedAttemptThenLocalRecovery() async throws {
        let f = try await queueFixture(self)
        let old = try await queueEnqueued(f)
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: old.mutation.id)
        let submitting = try await queueRecord(f)
        try downgrade(f)
        let migrated = try MutationQueueSQLiteStore(url: f.url)
        let count = try await migrated.recoverInterruptedOperations()
        XCTAssertEqual(count, 1)
        let recovered = try await migrated.record(scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(recovered?.request, old.mutation.requestBody)
        XCTAssertEqual(recovered?.attempt, submitting.attempt)
        XCTAssertEqual(recovered?.state, .outcomeUnknown)
    }
    func testMigrationFailureRollsBackVersionAndSchema() async throws {
        let f = try await queueFixture(self)
        let old = try await queueEnqueued(f)
        try downgrade(f)
        let fault = QueueFaultInjector()
        fault.arm(.migration)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url, fault: { try fault.hit($0) }))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "1")
        XCTAssertEqual(
            try queueRawScalar(
                f.url, "SELECT count(*) FROM sqlite_master WHERE name='mutation_attempt_history'"),
            "0")
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let row = try await reopened.record(scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(row?.request, old.mutation.requestBody)
    }
    func testDamagedVersionOneSchemaIsNeverRepaired() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try downgrade(f)
        try queueRawSQL(f.url, "DROP TRIGGER mutation_immutable")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url)) { error in
            XCTAssertEqual(error as? MutationQueueFailure, .corrupt)
        }
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }
    func testVersionOneTerminalOutcomeSurvivesMigration() async throws {
        let f = try await queueFixture(self)
        let old = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: old.mutation.id)
        try await f.queue.finish(lease, result: queueApplied(old.mutation))
        let terminal = try await queueRecord(f)
        try downgrade(f)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        _ = try await reopened.recoverInterruptedOperations()
        let raw = try await reopened.record(scope: f.scope, id: old.mutation.id.rawValue)
        let row = try await MutationPersistenceCodec(bridge: QueueValidator()).rehydrate(
            XCTUnwrap(raw))
        XCTAssertEqual(row, terminal)
    }
    func testRecoveryTransactionArchivesExactPreviousUncertainty() async throws {
        let f = try await unknown()
        let old = try await queueRecord(f)
        let lease = try await f.queue.acquireRecoveryAttempt(
            scope: f.scope, mutationId: old.mutation.id)
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .submitting)
        XCTAssertEqual(row.mutation, old.mutation)
        XCTAssertNotEqual(row.attempt?.id, old.attempt?.id)
        XCTAssertEqual(row.attempt?.dispatchRecorded, false)
        XCTAssertNil(row.evidence)
        let history = try await f.database.attemptHistory(
            scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(history.count, 1)
        XCTAssertEqual(history.first?.attempt, old.attempt)
        let decoded = try JSONDecoder().decode(
            MutationOutcomeEvidence.self, from: XCTUnwrap(history.first?.evidence))
        XCTAssertEqual(decoded, old.evidence)
        try await f.queue.finish(lease, result: .outcomeUnknown(.timeout))
    }
    func testRecoveryRollbackPreservesOldAttemptAndEvidence() async throws {
        let fault = QueueFaultInjector()
        let f = try await unknown(fault: fault)
        let old = try await queueRecord(f)
        fault.arm(.afterSubmitting)
        await queueAssertFailure(.diskFull) {
            try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: old.mutation.id)
        }
        let row = try await queueRecord(f)
        XCTAssertEqual(row, old)
        let history = try await f.database.attemptHistory(
            scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertTrue(history.isEmpty)
    }
    func testRecoveryCommitAcknowledgementLossPreservesCommittedHistory() async throws {
        let fault = QueueFaultInjector()
        let f = try await unknown(fault: fault)
        let old = try await queueRecord(f)
        fault.arm(.afterCommit)
        await queueAssertFailure(.commitAcknowledgementLost) {
            try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: old.mutation.id)
        }
        let settled = try await queueRecord(f)
        XCTAssertEqual(settled.state, .outcomeUnknown)
        XCTAssertEqual(settled.evidence?.uncertainty, "LEASE_COMMIT_ACKNOWLEDGEMENT_LOST")
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        _ = try await reopened.recoverInterruptedOperations()
        let raw = try await reopened.record(scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(raw?.state, .outcomeUnknown)
        XCTAssertEqual(raw?.request, old.mutation.requestBody)
        let history = try await reopened.attemptHistory(
            scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(history.count, 1)
        XCTAssertEqual(history.first?.attempt, old.attempt)
    }
    func testCrashDuringRecoveryReturnsUnknownAndRetainsHistory() async throws {
        let f = try await unknown()
        let old = try await queueRecord(f)
        let lease = try await f.queue.acquireRecoveryAttempt(
            scope: f.scope, mutationId: old.mutation.id)
        try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
            .authorizePersistedSubmission(old.mutation)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        _ = try await reopened.recoverInterruptedOperations()
        let row = try await reopened.record(scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(row?.state, .outcomeUnknown)
        XCTAssertEqual(row?.attempt?.dispatchRecorded, true)
        XCTAssertEqual(row?.request, old.mutation.requestBody)
        let history = try await reopened.attemptHistory(
            scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(history.first?.attempt, old.attempt)
    }
    func testHistoryIsImmutableAndCannotBeDeleted() async throws {
        let f = try await unknown()
        let old = try await queueRecord(f)
        _ = try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: old.mutation.id)
        XCTAssertThrowsError(
            try queueRawSQL(f.url, "UPDATE mutation_attempt_history SET attempt_owner='changed'"))
        XCTAssertThrowsError(try queueRawSQL(f.url, "DELETE FROM mutation_attempt_history"))
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT count(*) FROM mutation_attempt_history"), "1")
    }
    func testUnknownCannotTransitionWithoutArchivedEvidence() async throws {
        let f = try await unknown()
        XCTAssertThrowsError(
            try queueRawSQL(
                f.url,
                "UPDATE mutations SET state='SUBMITTING',attempt_id='\(UUID().uuidString)',dispatch_recorded=0,evidence=NULL"
            ))
        let row = try await queueRecord(f)
        XCTAssertEqual(row.state, .outcomeUnknown)
    }
    func testBoundedHistoryBlocksNinthRecoveryWithoutDeletingEvidence() async throws {
        let f = try await unknown()
        let old = try await queueRecord(f)
        for _ in 0..<MutationQueuePolicy.maximumRecoveryAttempts {
            let lease = try await f.queue.acquireRecoveryAttempt(
                scope: f.scope, mutationId: old.mutation.id)
            try await f.queue.finish(lease, result: .outcomeUnknown(.timeout))
        }
        let before = try await queueRecord(f)
        await queueAssertFailure(.recoveryLimit) {
            try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: old.mutation.id)
        }
        let after = try await queueRecord(f)
        XCTAssertEqual(after, before)
        let history = try await f.database.attemptHistory(
            scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(history.count, 8)
        XCTAssertEqual(history.first?.attempt, old.attempt)
    }
    func testHistoricalEvidenceCountsTowardCapacity() async throws {
        let f = try await unknown()
        let old = try await queueRecord(f)
        let bytes = Int(
            try queueRawScalar(
                f.url,
                "SELECT length(request)+length(payload)+length(evidence)+(SELECT length(response) FROM sync_bases) FROM mutations"
            ))!
        let limited = try MutationQueueSQLiteStore(url: f.url, maximumBytes: bytes)
        let queue = DurableMutationQueue(
            store: limited, provider: f.provider, bridge: QueueValidator())
        let lease = try await queue.acquireRecoveryAttempt(
            scope: f.scope, mutationId: old.mutation.id)
        await queueAssertFailure(.capacity) {
            try await queue.finish(lease, result: .outcomeUnknown(.timeout))
        }
        let row = try await limited.record(scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(row?.state, .submitting)
    }
    func testRecoveryRequiresExactlyUnknownSourceState() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        await queueAssertFailure(.invalidTransition) {
            try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: row.mutation.id)
        }
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        await queueAssertFailure(.invalidTransition) {
            try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: row.mutation.id)
        }
        try await f.queue.finish(lease, result: queueApplied(row.mutation))
        await queueAssertFailure(.invalidTransition) {
            try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: row.mutation.id)
        }
    }
    func testConflictIdentityConflictAndRebaselineCannotBlindlyReplay() async throws {
        for result in [
            ClientMutationSubmissionResult.conflict(nil, requestId: "mutation-request-01"),
            .mutationIdConflict(requestId: "mutation-request-01"),
            .rebaselineRequired(requestId: "mutation-request-01"),
        ] {
            let f = try await queueFixture(self)
            let row = try await queueEnqueued(f)
            let lease = try await f.queue.acquireAttempt(
                scope: f.scope, mutationId: row.mutation.id)
            try await f.queue.finish(lease, result: result)
            let retry = await drainCoordinator(f).reconcileUnknown(
                scope: f.scope, mutationId: row.mutation.id)
            guard case .stopped = retry else { return XCTFail() }
            let sent = await f.transport.requests().filter { $0.method == .post }
            XCTAssertTrue(sent.isEmpty)
        }
    }
    func testTwoSQLiteWritersCannotAcquireSameRecoveryAttempt() async throws {
        let f = try await unknown()
        let old = try await queueRecord(f)
        let second = try MutationQueueSQLiteStore(url: f.url)
        let results = await withTaskGroup(of: Bool.self, returning: [Bool].self) { group in
            for db in [f.database, second] {
                group.addTask {
                    do {
                        _ = try await db.beginRecoveryAttempt(
                            scope: f.scope, id: old.mutation.id.rawValue,
                            owner: UUID().uuidString, attemptId: UUID().uuidString)
                        return true
                    } catch { return false }
                }
            }
            var outcomes: [Bool] = []
            for await value in group { outcomes.append(value) }
            return outcomes
        }
        XCTAssertEqual(results.filter { $0 }.count, 1)
        let history = try await second.attemptHistory(scope: f.scope, id: old.mutation.id.rawValue)
        XCTAssertEqual(history.count, 1)
    }
    func testDifferentRecordCannotAcquireWhileScopeIsSubmitting() async throws {
        let f = try await queueFixture(self)
        let first = try await queueEnqueued(f)
        let later = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: first.mutation.id)
        await queueAssertFailure(.concurrentExecution) {
            try await f.queue.acquireAttempt(scope: f.scope, mutationId: later.mutation.id)
        }
    }
    func testLaterPendingCannotOvertakeEarlierUnknown() async throws {
        let f = try await unknown()
        let later = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        await queueAssertFailure(.dependencyConflict) {
            try await f.queue.acquireAttempt(scope: f.scope, mutationId: later.mutation.id)
        }
    }
    func testLaterUnknownCannotOvertakeEarlierConflict() async throws {
        let f = try await queueFixture(self)
        let first = try await queueEnqueued(f)
        let later = try await queueEnqueued(f, mutation: queuePrepared(id: 11, node: 2))
        // Synthetic imported P035 uncertainty; exact requests and reservations are retained.
        try queueRawSQL(
            f.url,
            "UPDATE mutations SET state='SUBMITTING',attempt_id='\(UUID().uuidString)',attempt_owner='\(UUID().uuidString)',attempt_started=1 WHERE mutation_id='\(later.mutation.id.rawValue)'"
        )
        _ = try await f.database.recoverInterruptedOperations()
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: first.mutation.id)
        try await f.queue.finish(lease, result: .conflict(nil, requestId: "mutation-request-01"))
        await queueAssertFailure(.dependencyConflict) {
            try await f.queue.acquireRecoveryAttempt(scope: f.scope, mutationId: later.mutation.id)
        }
    }
    func testIndependentScopesCanHoldDistinctLeases() async throws {
        let f = try await queueFixture(self)
        let first = try await queueEnqueued(f)
        let scope = try await queueScope(library: 81)
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: scope))
        let other = try await queuePrepared(id: 11, scope: scope)
        guard case .enqueued = await f.queue.enqueue(other) else { return XCTFail() }
        _ = try await f.queue.acquireAttempt(scope: f.scope, mutationId: first.mutation.id)
        _ = try await f.queue.acquireAttempt(scope: scope, mutationId: other.id)
        let a = try await f.database.record(scope: f.scope, id: first.mutation.id.rawValue)
        let b = try await f.database.record(scope: scope, id: other.id.rawValue)
        XCTAssertNotEqual(a?.attempt?.id, b?.attempt?.id)
    }
    func testOlderTerminalRowsDoNotStarvePendingRead() async throws {
        let f = try await queueFixture(self)
        for id in 100..<205 {
            let row = try await queueEnqueued(f, mutation: queuePrepared(id: id))
            let lease = try await f.queue.acquireAttempt(
                scope: f.scope, mutationId: row.mutation.id)
            try await f.queue.finish(lease, result: queueApplied(row.mutation))
        }
        let pending = try await queueEnqueued(f, mutation: queuePrepared(id: 205))
        await f.transport.set(try drainAppliedHTTP(pending.mutation))
        let result = await drainCoordinator(f).drain(scope: f.scope, maximumOperations: 1)
        XCTAssertEqual(result, .completed(MutationDrainSummary(attempted: 1, applied: 1)))
    }
    func testLateOldLeaseCannotOverwriteRecoveryTerminalResult() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let old = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(old, result: .outcomeUnknown(.timeout))
        let recovery = try await f.queue.acquireRecoveryAttempt(
            scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(recovery, result: queueApplied(row.mutation))
        for result in [
            ClientMutationSubmissionResult.outcomeUnknown(.timeout),
            .conflict(nil, requestId: "mutation-request-01"), try await queueApplied(row.mutation),
        ] {
            await queueAssertFailure(.invalidTransition) {
                try await f.queue.finish(old, result: result)
            }
        }
        let terminal = try await queueRecord(f)
        XCTAssertEqual(terminal.state, .applied)
    }
    func testDuplicateResultCannotOverwriteTerminalOutcome() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        let applied = try await queueApplied(row.mutation)
        try await f.queue.finish(lease, result: applied)
        await queueAssertFailure(.invalidTransition) {
            try await f.queue.finish(lease, result: applied)
        }
        let persisted = try await queueRecord(f)
        XCTAssertEqual(persisted.state, .applied)
    }
    func testHistoricalCorruptionBlocksFurtherRecovery() async throws {
        let f = try await unknown()
        let row = try await queueRecord(f)
        let lease = try await f.queue.acquireRecoveryAttempt(
            scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: .outcomeUnknown(.timeout))
        try queueRawSQL(f.url, "DROP TRIGGER attempt_history_immutable")
        try queueRawSQL(
            f.url, "UPDATE mutation_attempt_history SET evidence=?", bindings: [Data("{}".utf8)])
        guard
            case .stopped = await drainCoordinator(f).reconcileUnknown(
                scope: f.scope, mutationId: row.mutation.id)
        else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT count(*) FROM mutation_attempt_history"), "1")
    }

    private func downgrade(_ f: QueueFixture) throws {
        try projectionDowngradeV3(f.url)
        try queueRawSQL(f.url, "DROP TABLE mutation_attempt_history")
        try queueRawSQL(f.url, "DROP TRIGGER mutation_transition")
        try queueRawSQL(f.url, XCTUnwrap(MutationQueueSQLiteStore.version1Schema.last))
        for sql in [
            "DROP TRIGGER inbound_immutable", "DROP TRIGGER inbound_application_gate",
            "DROP TABLE inbound_pages",
        ] {
            try queueRawSQL(f.url, sql)
        }
        try queueRawSQL(f.url, "PRAGMA user_version=1")
    }
    private func unknown(fault: QueueFaultInjector? = nil) async throws -> QueueFixture {
        let f = try await queueFixture(self, fault: fault)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await DurableMutationAttemptAuthorizer(queue: f.queue, lease: lease)
            .authorizePersistedSubmission(row.mutation)
        try await f.queue.finish(lease, result: .outcomeUnknown(.timeout))
        return f
    }
}

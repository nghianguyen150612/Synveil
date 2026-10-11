import Foundation
import SQLite3
import XCTest

@testable import Synveil

@MainActor
final class MutationQueueSQLiteTests: XCTestCase {
    func testDatabaseInitializationRegistersSchema() async throws {
        let f = try await queueFixture(self)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "5")
        XCTAssertEqual(
            try queueRawScalar(
                f.url,
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
            ), "18")
    }
    func testWALAndFullDurabilityConfiguration() async throws {
        let f = try await queueFixture(self)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA journal_mode"), "wal")
        // synchronous/foreign_keys are connection-local; validate the store via a test-only inspection.
        let configuration = try await f.database.durabilityConfiguration()
        XCTAssertEqual(configuration, [1, 2, 64])
    }
    func testReopenPreservesExactMutation() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let raw = try await reopened.record(scope: f.scope, id: original.mutation.id.rawValue)
        let restored = try await MutationPersistenceCodec(bridge: QueueValidator()).rehydrate(
            XCTUnwrap(raw))
        XCTAssertEqual(restored, original)
    }
    func testCommittedDataVisibleAcrossConnections() async throws {
        let f = try await queueFixture(self)
        let second = try MutationQueueSQLiteStore(url: f.url)
        let original = try await queueEnqueued(f)
        let row = try await second.record(scope: f.scope, id: original.mutation.id.rawValue)
        XCTAssertEqual(row?.request, original.mutation.requestBody)
    }
    func testAtomicEnqueuePersistsDependencyRows() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM dependencies"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }
    func testFailureBeforeInsertLeavesEmptyQueue() async throws {
        try await failedInsert(at: .beforeInsert)
    }
    func testFailureAfterInsertRollsBackEntireOperation() async throws {
        try await failedInsert(at: .afterInsert)
    }
    func testFailedCommitDoesNotReportSuccess() async throws {
        try await failedInsert(at: .beforeCommit)
    }
    private func failedInsert(at point: MutationQueueFaultPoint) async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        fault.arm(point)
        let result = await f.queue.enqueue(try await queuePrepared())
        XCTAssertEqual(result, .failed(.diskFull))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM dependencies"), "0")
    }
    func testInjectedIOFailureRollsBackWithoutDiagnostics() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        fault.arm(.afterInsert, failure: .io)
        let result = await f.queue.enqueue(try await queuePrepared())
        XCTAssertEqual(result, .failed(.io))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "0")
    }

    func testRollbackFailurePoisonsConnectionAndHidesUncommittedRows() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        fault.arm(.afterInsert)
        fault.armRollbackFailure()
        let mutation = try await queuePrepared()
        let result = await f.queue.enqueue(mutation)
        XCTAssertEqual(result, .failed(.diskFull))
        let lookup = await f.queue.get(scope: f.scope, mutationId: mutation.id)
        XCTAssertEqual(lookup, .failed(.io))
        // An independent WAL reader sees the last committed state while the poisoned writer is
        // retained only for preservation/closure. It cannot publish its uncommitted INSERT.
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "0")
    }

    func testCommitAcknowledgementLossReadsBackCommittedIdentity() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        fault.arm(.afterCommit)
        // Exercise the storage boundary directly so session-binding commits do not consume the fault.
        let op = try await queuePrepared()
        await queueAssertFailure(.commitAcknowledgementLost) {
            try await f.database.enqueue(
                op, payload: MutationPersistenceCodec(bridge: QueueValidator()).payloadBytes(op))
        }
        let row = try await queueRecord(f)
        XCTAssertEqual(row.mutation, op)
        XCTAssertEqual(row.state, .pending)
    }
    func testDuplicateSameSemanticsRetainsOneOrder() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let second = await f.queue.enqueue(row.mutation)
        XCTAssertEqual(second, .existing(row))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }
    func testDuplicateChangedSemanticsFailsWithoutOverwrite() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let result = await f.queue.enqueue(try await queuePrepared(name: "Different"))
        XCTAssertEqual(result, .failed(.duplicateIdentity))
        let restored = try await queueRecord(f)
        XCTAssertEqual(restored, original)
    }
    func testConcurrentIdenticalEnqueuesProduceOneRow() async throws {
        let f = try await queueFixture(self)
        let op = try await queuePrepared()
        let results = await withTaskGroup(of: MutationEnqueueResult.self) { group in
            for _ in 0..<16 { group.addTask { await f.queue.enqueue(op) } }
            var results: [MutationEnqueueResult] = []
            for await result in group { results.append(result) }
            return results
        }
        XCTAssertEqual(
            results.filter {
                if case .enqueued = $0 { return true }
                return false
            }.count, 1)
        XCTAssertEqual(
            results.filter {
                if case .existing = $0 { return true }
                return false
            }.count, 15)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }
    func testConcurrentOverlappingDependenciesAreSerialized() async throws {
        let f = try await queueFixture(self)
        let a = try await queuePrepared(id: 10)
        let b = try await queuePrepared(id: 11)
        async let first = f.queue.enqueue(a)
        async let second = f.queue.enqueue(b)
        let results = await [first, second]
        XCTAssertEqual(results.filter { $0 == .failed(.dependencyConflict) }.count, 1)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
    }
    func testOutstandingCapacityPreservesExistingRows() async throws {
        let f = try await queueFixture(self, maximumOutstanding: 1)
        let original = try await queueEnqueued(f)
        let result = await f.queue.enqueue(try await queuePrepared(id: 11, node: 2))
        XCTAssertEqual(result, .failed(.capacity))
        let restored = try await queueRecord(f)
        XCTAssertEqual(restored, original)
    }
    func testTotalRecordCapacityIncludesTerminalEvidence() async throws {
        let f = try await queueFixture(self, maximumRecords: 1)
        let row = try await queueEnqueued(f)
        let lease = try await f.queue.acquireAttempt(scope: f.scope, mutationId: row.mutation.id)
        try await f.queue.finish(lease, result: .failed(.permanentRejection(.notFound)))
        let result = await f.queue.enqueue(try await queuePrepared(id: 11, node: 2))
        XCTAssertEqual(result, .failed(.capacity))
    }
    func testSerializedByteCapacityFailsClosed() async throws {
        let f = try await queueFixture(self, maximumBytes: 600)
        let result = await f.queue.enqueue(
            try await queuePrepared(name: String(repeating: "a", count: 500)))
        XCTAssertEqual(result, .failed(.capacity))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "0")
    }
    func testDeterministicDatabaseOrdering() async throws {
        let f = try await queueFixture(self)
        for value in [19, 11, 15] {
            _ = try await queueEnqueued(f, mutation: queuePrepared(id: value, node: value))
        }
        let result = await f.queue.pending(scope: f.scope, limit: 10)
        guard case .records(let rows) = result else { return XCTFail() }
        XCTAssertEqual(rows.map { $0.mutation.id.rawValue }, [19, 11, 15].map(queueUUID))
        XCTAssertEqual(rows.map(\.enqueueOrder), [1, 2, 3])
    }
    func testCrossDeviceStorageIsolation() async throws {
        try await storageIsolation(scope: queueScope(device: 99))
    }
    func testCrossLibraryStorageIsolation() async throws {
        try await storageIsolation(scope: queueScope(library: 99))
    }
    func testCrossOriginStorageIsolation() async throws {
        try await storageIsolation(scope: queueScope(endpoint: "https://other.example"))
    }
    func testCrossOwnerStorageIsolation() async throws {
        try await storageIsolation(scope: queueScope(owner: 99))
    }
    private func storageIsolation(scope: ClientMutationScope) async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let rows = try await f.database.records(scope: scope, limit: 10)
        let lookup = try await f.database.record(scope: scope, id: original.mutation.id.rawValue)
        XCTAssertTrue(rows.isEmpty)
        XCTAssertNil(lookup)
    }
    func testSameMutationIdHasIndependentScopedIdentity() async throws {
        let f = try await queueFixture(self)
        let first = try await queueEnqueued(f)
        let scope = try await queueScope(library: 99)
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: scope))
        let second = try await queuePrepared(scope: scope)
        _ = try await f.database.enqueue(
            second, payload: MutationPersistenceCodec(bridge: QueueValidator()).payloadBytes(second)
        )
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "2")
        XCTAssertEqual(first.mutation.id, second.id)
    }
    func testUnknownFutureSchemaDoesNotResetDatabase() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try queueRawSQL(f.url, "PRAGMA user_version=6")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url)) {
            XCTAssertEqual($0 as? MutationQueueFailure, .unsupportedSchema)
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "6")
    }
    func testUnversionedNonemptyDatabaseRejected() async throws {
        let url = queueTemporaryURL(self)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try queueRawSQL(url, "CREATE TABLE evidence(value TEXT)")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: url))
        XCTAssertEqual(
            try queueRawScalar(url, "SELECT count(*) FROM sqlite_master WHERE name='evidence'"), "1"
        )
    }
    func testInterruptedMigrationRollsBackSchemaAndVersion() async throws {
        let url = queueTemporaryURL(self)
        let fault = QueueFaultInjector()
        fault.arm(.migration)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: url, fault: { try fault.hit($0) }))
        XCTAssertEqual(try queueRawScalar(url, "PRAGMA user_version"), "0")
        XCTAssertEqual(
            try queueRawScalar(
                url, "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'"), "0")
        _ = try MutationQueueSQLiteStore(url: url)
        XCTAssertEqual(try queueRawScalar(url, "PRAGMA user_version"), "5")
    }
    func testMalformedCurrentSchemaRejectedWithoutRecreation() async throws {
        let f = try await queueFixture(self)
        try queueRawSQL(f.url, "DROP INDEX dependency_node")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_bases"), "1")
    }
    func testCorruptDatabaseIsPreserved() async throws {
        let url = queueTemporaryURL(self)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let original = Data("damaged sqlite evidence".utf8)
        try original.write(to: url)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: url))
        XCTAssertEqual(try Data(contentsOf: url), original)
    }
    func testBoundedBusyHandlingFromCompetingWriter() async throws {
        let f = try await queueFixture(self)
        var db: OpaquePointer?
        XCTAssertEqual(sqlite3_open(f.url.path, &db), SQLITE_OK)
        let locked = try XCTUnwrap(db)
        defer {
            sqlite3_exec(locked, "ROLLBACK", nil, nil, nil)
            sqlite3_close(locked)
        }
        XCTAssertEqual(sqlite3_exec(locked, "BEGIN IMMEDIATE", nil, nil, nil), SQLITE_OK)
        let op = try await queuePrepared()
        await queueAssertFailure(.busy) {
            try await f.database.enqueue(
                op, payload: MutationPersistenceCodec(bridge: QueueValidator()).payloadBytes(op))
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "0")
    }
    func testImmutableFieldsProtectedByDatabaseTrigger() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        XCTAssertThrowsError(try queueRawSQL(f.url, "UPDATE mutations SET epoch='2'"))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT epoch FROM mutations"), "1")
    }
    func testInvalidStateProtectedByDatabaseConstraint() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        XCTAssertThrowsError(try queueRawSQL(f.url, "UPDATE mutations SET state='UNRECOGNIZED'"))
        XCTAssertThrowsError(try queueRawSQL(f.url, "UPDATE mutations SET state='APPLIED'"))
    }
    func testFileAndSidecarPermissionsArePrivate() async throws {
        let f = try await queueFixture(self)
        for path in [f.url.path, f.url.path + "-wal", f.url.path + "-shm"] {
            let attributes = try FileManager.default.attributesOfItem(atPath: path)
            XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o600)
        }
        let attributes = try FileManager.default.attributesOfItem(
            atPath: f.url.deletingLastPathComponent().path)
        XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o700)
    }
    func testNativeDataProtectionAttributes() async throws {
        let f = try await queueFixture(self)
        #if os(iOS)
            let attributes = try [
                f.url.deletingLastPathComponent().path, f.url.path, f.url.path + "-wal",
                f.url.path + "-shm",
            ].map {
                try FileManager.default.attributesOfItem(atPath: $0)
            }
            let values = attributes.map(MutationQueueFilePolicy.protectionName)
            #if targetEnvironment(simulator)
                if attributes.allSatisfy({ $0[.protectionKey] == nil }) {
                    throw XCTSkip(
                        "The Simulator filesystem does not expose Data Protection attributes; requested policy is verified separately. Physical-device round-trip remains required."
                    )
                }
            #endif
            for actual in values {
                XCTAssertEqual(
                    actual, FileProtectionType.completeUntilFirstUserAuthentication.rawValue)
            }
        #else
            XCTAssertTrue(FileManager.default.fileExists(atPath: f.url.path))
        #endif
    }
    func testRequestedProtectionPolicyAndPermissions() throws {
        for directory in [false, true] {
            let attributes = MutationQueueFilePolicy.requestedAttributes(directory: directory)
            XCTAssertEqual(attributes[.posixPermissions] as? Int, directory ? 0o700 : 0o600)
            #if os(iOS)
                XCTAssertEqual(
                    MutationQueueFilePolicy.protectionName(attributes),
                    FileProtectionType.completeUntilFirstUserAuthentication.rawValue)
            #endif
        }
    }

    func testBackupExclusionForDeviceBoundQueue() async throws {
        let f = try await queueFixture(self)
        #if canImport(Darwin)
            for url in [
                f.url.deletingLastPathComponent(), f.url, URL(fileURLWithPath: f.url.path + "-wal"),
                URL(fileURLWithPath: f.url.path + "-shm"),
            ] {
                XCTAssertEqual(
                    try url.resourceValues(forKeys: [.isExcludedFromBackupKey])
                        .isExcludedFromBackup, true)
            }
        #else
            XCTAssertTrue(FileManager.default.fileExists(atPath: f.url.path))
        #endif
    }
    func testDatabaseSymlinkRejected() async throws {
        let f = try await queueFixture(self)
        let link = queueTemporaryURL(self)
        try FileManager.default.createDirectory(
            at: link.deletingLastPathComponent(), withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: f.url)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: link))
    }
    func testProductionLocationUsesApplicationSupport() throws {
        let url = try MutationQueueSQLiteStore.productionURL()
        let root = try XCTUnwrap(
            FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first)
        XCTAssertTrue(url.path.hasPrefix(root.path))
        XCTAssertTrue(url.path.hasSuffix("Synveil/MutationQueue/mutations.sqlite"))
    }
    func testOpenFailureIsTypedAndDoesNotBypassGate() async throws {
        let f = try await queueFixture(self)
        XCTAssertThrowsError(
            try MutationQueueSQLiteStore(url: URL(string: "https://invalid.example")!))
        let repository = AuthenticatedClientMutationRepository(
            provider: f.provider, bridge: QueueValidator())
        let result = await repository.submit(try await queuePrepared())
        XCTAssertEqual(result, .failed(.preparationRequired))
        let sent = await f.transport.requests()
        XCTAssertEqual(sent.count, 1)
    }
    func testCredentialsAndRawHeadersAbsentFromSQLiteAndSidecars() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        for path in [f.url.path, f.url.path + "-wal", f.url.path + "-shm"] {
            let bytes = try Data(contentsOf: URL(fileURLWithPath: path))
            for secret in [
                queueBearer, "sve1_" + String(repeating: "b", count: 64), "Authorization", "Cookie",
                "csrf", "Keychain",
            ] {
                XCTAssertNil(bytes.range(of: Data(secret.utf8)), "Secret category present")
            }
        }
    }
}

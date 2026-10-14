import Foundation
import XCTest

@testable import Synveil

@MainActor
final class FileDownloadStorageTests: XCTestCase {
    func testFreshSchemaV7() async throws {
        let f = try await downloadFixture(self)
        XCTAssertEqual(try queueRawScalar(f.queue.url, "PRAGMA user_version"), "9")
        XCTAssertEqual(try queueRawScalar(f.queue.url, "SELECT count(*) FROM file_downloads"), "0")
    }
    private func downgradeV5(_ url: URL) throws {
        try queueRawSQL(url, "DROP TABLE session_authorization_checks")
        try queueRawSQL(url, "DROP TABLE background_upload_orphans")
        try queueRawSQL(url, "DROP TABLE background_upload_jobs")
        try queueRawSQL(url, "DROP TRIGGER file_upload_identity")
        try queueRawSQL(url, "DROP TRIGGER file_upload_offset")
        try queueRawSQL(url, "DROP TABLE file_uploads")
        for name in ["file_download_identity", "file_download_verified"] {
            try queueRawSQL(url, "DROP TRIGGER \(name)")
        }
        try queueRawSQL(url, "DROP TABLE file_downloads")
        try queueRawSQL(url, "PRAGMA user_version=5")
    }
    func testV5MigrationRetainsMutationAndSnapshotEvidence() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        try downgradeV5(f.url)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let rows = try await reopened.records(scope: f.scope, limit: 10)
        XCTAssertEqual(rows.first?.mutationId, original.mutation.id.rawValue)
        XCTAssertEqual(rows.first?.request, original.mutation.requestBody)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "9")
        for table in [
            "rebaseline_sessions", "rebaseline_nodes", "projection_commits", "inbound_pages",
            "mutation_attempt_history", "scopes",
        ] {
            XCTAssertEqual(
                try queueRawScalar(
                    f.url, "SELECT count(*) FROM sqlite_master WHERE name='\(table)'"), "1")
        }
    }
    func testV5MigrationRollback() async throws {
        let f = try await queueFixture(self)
        _ = try await queueEnqueued(f)
        try downgradeV5(f.url)
        let fault = QueueFaultInjector()
        fault.arm(.migration)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url, fault: { try fault.hit($0) }))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "5")
        XCTAssertEqual(
            try queueRawScalar(
                f.url, "SELECT count(*) FROM sqlite_master WHERE name='file_downloads'"), "0")
    }
    func testCorruptV5FailsClosed() async throws {
        let f = try await queueFixture(self)
        try downgradeV5(f.url)
        try queueRawSQL(f.url, "ALTER TABLE scopes ADD COLUMN untrusted TEXT")
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "5")
    }
    func testImmutableIdentitySQLTrigger() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        for column in [
            "version_id", "node_id", "expected_length", "expected_sha256", "credential_id",
            "session_id",
        ] {
            XCTAssertThrowsError(
                try queueRawSQL(f.queue.url, "UPDATE file_downloads SET \(column)='changed'"))
        }
    }
    func testSQLCannotDeclareUnverifiedDataVerified() async throws {
        let f = try await downloadFixture(self, body: Data(repeating: 97, count: 100_000))
        await f.content.configure(failAfterChunks: 1)
        _ = await downloadRun(f)
        XCTAssertThrowsError(
            try queueRawSQL(f.queue.url, "UPDATE file_downloads SET state='VERIFIED'"))
    }
    func test512MiBQuotaBeforeContentGET() async throws {
        let f = try await downloadFixture(self)
        var object = downloadVersionObject()
        var resource = object["data"] as! [String: Any]
        var attributes = resource["attributes"] as! [String: Any]
        attributes["byte_length"] = String(DownloadPolicy.quota + 1)
        resource["attributes"] = attributes
        object["data"] = resource
        await f.metadata.setVersion(try queueHTTP(object))
        guard case .stopped(_, .stagingQuotaExceeded) = await downloadRun(f) else {
            return XCTFail()
        }
        let requests = await f.content.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func test64MiBReserveBeforeContentGET() async throws {
        let f = try await downloadFixture(self, free: DownloadPolicy.freeReserve + 4)
        guard case .stopped(_, .insufficientStorage) = await downloadRun(f) else {
            return XCTFail()
        }
        let requests = await f.content.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testOverflowReservationRejected() async throws {
        let f = try await downloadFixture(self)
        do {
            try await f.staging.reserve(
                DownloadStorageReservation(operationId: DownloadOperationId(), bytes: UInt64.max),
                durable: [DownloadStorageReservation(operationId: DownloadOperationId(), bytes: 1)])
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .stagingQuotaExceeded) }
    }
    func testConcurrentReservationsSerialized() async throws {
        let f = try await downloadFixture(self)
        let a = DownloadStorageReservation(
            operationId: DownloadOperationId(), bytes: DownloadPolicy.quota)
        let b = DownloadStorageReservation(operationId: DownloadOperationId(), bytes: 1)
        try await f.staging.reserve(a, durable: [])
        do {
            try await f.staging.reserve(b, durable: [])
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .stagingQuotaExceeded) }
    }
    func testUnknownFutureUploadStagingAccounted() async throws {
        let f = try await downloadFixture(self)
        let file = f.root.appendingPathComponent("future-upload")
        try Data("x".utf8).write(to: file)
        let reservation = DownloadStorageReservation(
            operationId: DownloadOperationId(), bytes: DownloadPolicy.quota)
        do {
            try await f.staging.reserve(reservation, durable: [])
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .stagingQuotaExceeded) }
    }
    func testPrivatePermissionsAndBackupExclusion() async throws {
        let f = try await downloadFixture(self)
        let file = try downloadCapability(await downloadRun(f))
        let path = f.root.appendingPathComponent(file.operationId.rawValue + ".verified")
        let directory = try FileManager.default.attributesOfItem(atPath: f.root.path)
        let attributes = try FileManager.default.attributesOfItem(atPath: path.path)
        XCTAssertEqual((directory[.posixPermissions] as? NSNumber)?.intValue, 0o700)
        XCTAssertEqual((attributes[.posixPermissions] as? NSNumber)?.intValue, 0o600)
        #if canImport(Darwin)
            XCTAssertEqual(
                try path.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup,
                true)
        #endif
    }
    func testOpaquePathTraversalRejected() {
        for value in ["../escape", "/tmp/escape", "name.part", UUID().uuidString] {
            XCTAssertThrowsError(try DownloadOperationId(validating: value))
        }
    }
    func testSymlinkRootRejected() async throws {
        let f = try await downloadFixture(self)
        let link = f.root.deletingLastPathComponent().appendingPathComponent("link")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: f.root)
        XCTAssertThrowsError(try DownloadStagingStore(root: link))
    }
    func testSymlinkPartRejectedBeforeAppend() async throws {
        let f = try await downloadFixture(self, body: Data(repeating: 97, count: 100_000))
        await f.content.configure(failAfterChunks: 1)
        _ = await downloadRun(f)
        let record = try await downloadRecord(f)
        let path = f.root.appendingPathComponent(record.id.rawValue + ".part")
        try FileManager.default.removeItem(at: path)
        let external = f.root.deletingLastPathComponent().appendingPathComponent("sensitive")
        try Data("secret".utf8).write(to: external)
        try FileManager.default.createSymbolicLink(at: path, withDestinationURL: external)
        guard case .stopped(_, .filesystemFailure) = await downloadRun(f, resume: record.id) else {
            return XCTFail()
        }
        XCTAssertEqual(try Data(contentsOf: external), Data("secret".utf8))
    }
    func testCheckpointTransactionRollbackDoesNotInventOffset() async throws {
        let fault = QueueFaultInjector()
        let f = try await downloadFixture(
            self, body: Data(repeating: 97, count: 100_000), databaseFault: fault)
        let gate = QueueGate()
        await f.content.configure(gate: gate)
        let task = Task { await downloadRun(f) }
        await gate.wait()
        fault.arm(.beforeCommit)
        await gate.release()
        guard case .stopped = await task.value else { return XCTFail() }
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.checkpoint.offset, 0)
        let bytes = try Data(
            contentsOf: f.root.appendingPathComponent(record.id.rawValue + ".part"))
        XCTAssertEqual(bytes.count, DownloadPolicy.bufferSize)
        let recovery = try await f.staging.recover(record)
        XCTAssertEqual(recovery, .resume(record.checkpoint))
    }
    func testFinalSQLiteCommitFailureAfterRenameReconciles() async throws {
        let fault = QueueFaultInjector()
        let f = try await downloadFixture(self, databaseFault: fault)
        var armed = false
        let result = await f.coordinator.run(library: f.queue.scope.libraryId, node: f.node.id) {
            progress in
            if progress.state == .verifying, !armed {
                armed = true
                fault.arm(.beforeCommit)
            }
        }
        guard case .stopped(let saved, _) = result else { return XCTFail() }
        let record = try XCTUnwrap(saved)
        XCTAssertNotEqual(record.state, .verified)
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".verified").path))
        _ = try downloadCapability(await downloadRun(f, resume: record.id))
    }
    func testQuarantinePreventsAnyOtherScopeAccess() async throws {
        let f = try await downloadFixture(self)
        let capability = try downloadCapability(await downloadRun(f))
        let scope = try await queueScope(library: 99)
        try await f.queue.database.bindSession(scope: scope, credentialId: queueUUID(92))
        let other = try await f.queue.database.download(
            scope: scope, credentialId: queueUUID(92),
            id: capability.operationId, bridge: QueueValidator())
        XCTAssertNil(other)
        try await f.queue.database.quarantineSessions()
        do {
            _ = try await f.coordinator.read(capability, offset: 0, count: 5)
            XCTFail()
        } catch {}
    }
    func testIndependentConnectionsCannotOwnDuplicateDownloads() async throws {
        let f = try await downloadFixture(self)
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let first = UUID()
        let second = UUID()
        try await f.queue.database.claimDownloadRun(owner: first)
        do {
            try await reopened.claimDownloadRun(owner: second)
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .duplicateOperation) }
        await f.queue.database.finishDownloadRun(owner: first)
        try await reopened.claimDownloadRun(owner: second)
        await reopened.finishDownloadRun(owner: second)
    }
}

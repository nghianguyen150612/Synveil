import Foundation
import XCTest

@testable import Synveil

@MainActor
final class FileDownloadRecoveryTests: XCTestCase {
    private func partial(_ f: DownloadFixture) async throws -> DownloadRecord {
        await f.content.configure(failAfterChunks: 1)
        guard case .stopped(_, .timeout) = await downloadRun(f) else {
            XCTFail()
            throw DownloadFailure.unexpectedStatus
        }
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.checkpoint.offset, UInt64(DownloadPolicy.bufferSize))
        return record
    }
    private var largeBody: Data { Data(repeating: 97, count: DownloadPolicy.bufferSize * 2 + 19) }

    func testFullDownloadHashesPublishesAndCommits() async throws {
        let f = try await downloadFixture(self)
        let file = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.state, .verified)
        XCTAssertEqual(record.checkpoint.prefixSHA256, downloadDigest(f.body))
        XCTAssertEqual(record.checkpoint.offset, 5)
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".part").path))
        let data = try await f.coordinator.read(file, offset: 0, count: 5)
        XCTAssertEqual(data, f.body)
        XCTAssertEqual(
            try queueRawScalar(f.queue.url, "SELECT state FROM file_downloads"), "VERIFIED")
    }
    func testExactDeviceBearerAndImmutableURL() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let requests = await f.content.requests()
        let request = try XCTUnwrap(requests.first)
        let expectedPath = [
            "api", "v1", "versions", f.version.rawValue, "content",
        ].joined(separator: "/")
        XCTAssertEqual(
            request.url.absoluteString,
            f.queue.scope.serverEndpoint.urlString + expectedPath
        )
        XCTAssertEqual(request.headers["Authorization"], "Bearer " + queueBearer)
        XCTAssertEqual(request.headers["Accept"], "application/octet-stream")
        XCTAssertEqual(request.headers["Accept-Encoding"], "identity")
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertNil(request.headers["X-CSRF-Token"])
        XCTAssertNil(request.headers["If-None-Match"])
        XCTAssertNil(request.headers["If-Range"])
        let metadata = await f.metadata.requests()
        XCTAssertEqual(metadata.count, 2)
    }
    func testEmptyContentStillHashesAndCommits() async throws {
        let f = try await downloadFixture(self, body: Data())
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.checkpoint.prefixSHA256, DownloadPolicy.emptyDigest)
        XCTAssertEqual(record.state, .verified)
    }
    func testSameLengthCorruptionNeverVerified() async throws {
        let f = try await downloadFixture(self)
        await f.content.configure(body: Data("HELLO".utf8))
        guard case .stopped(_, .sha256Mismatch) = await downloadRun(f) else { return XCTFail() }
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.state, .integrityFailure)
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".verified").path))
    }
    func testTruncatedBodyNeverVerified() async throws {
        let f = try await downloadFixture(self)
        await f.content.configure(body: Data("hel".utf8))
        guard case .stopped(_, .contentLengthMismatch) = await downloadRun(f) else {
            return XCTFail()
        }
        let durable = try await downloadRecord(f)
        XCTAssertNotEqual(durable.state, .verified)
    }
    func testResumePreservesOriginalOperationAndContentIdentity() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        await f.content.configure()
        _ = try downloadCapability(await downloadRun(f, resume: original.id))
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.id, original.id)
        XCTAssertEqual(record.identity, original.identity)
        XCTAssertEqual(record.attemptCount, 2)
        let requests = await f.content.requests()
        XCTAssertEqual(requests.last?.headers["Range"], "bytes=65536-131090")
        XCTAssertTrue(requests.allSatisfy { $0.url.path.contains("/versions/") })
    }
    func testCurrentVersionChangesDoNotSwitchPinnedVersion() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        var object =
            try JSONSerialization.jsonObject(with: projectionNodeHTTP(f.node).body)
            as! [String: Any]
        var resource = object["data"] as! [String: Any]
        var attributes = resource["attributes"] as! [String: Any]
        attributes["current_version_id"] = queueUUID(502)
        resource["attributes"] = attributes
        object["data"] = resource
        await f.metadata.setNode(try queueHTTP(object))
        await f.content.configure()
        let file = try downloadCapability(await downloadRun(f, resume: original.id))
        XCTAssertEqual(file.identity.versionId, original.identity.versionId)
        let requests = await f.content.requests()
        XCTAssertTrue(
            requests.allSatisfy { $0.url.path.contains(original.identity.versionId.rawValue) })
    }
    func testRange200DoesNotAppend() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        await f.content.configure(status: 200)
        guard case .stopped(_, .invalidRange) = await downloadRun(f, resume: original.id) else {
            return XCTFail()
        }
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.checkpoint, original.checkpoint)
        let bytes = try Data(
            contentsOf: f.root.appendingPathComponent(original.id.rawValue + ".part"))
        XCTAssertEqual(bytes.count, DownloadPolicy.bufferSize)
    }
    func testExtraUncommittedTailIsTruncatedOnResume() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        let path = f.root.appendingPathComponent(original.id.rawValue + ".part")
        let writer = try FileHandle(forWritingTo: path)
        try writer.seekToEnd()
        try writer.write(contentsOf: Data("uncommitted".utf8))
        try writer.synchronize()
        try writer.close()
        await f.content.configure()
        _ = try downloadCapability(await downloadRun(f, resume: original.id))
        let durable = try await downloadRecord(f)
        XCTAssertEqual(durable.checkpoint.offset, UInt64(largeBody.count))
    }
    func testMissingCommittedPrefixRejected() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        let writer = try FileHandle(
            forWritingTo: f.root.appendingPathComponent(original.id.rawValue + ".part"))
        try writer.truncate(atOffset: 10)
        try writer.close()
        let before = await f.content.requests().count
        guard case .stopped(_, .integrityFailure) = await downloadRun(f, resume: original.id) else {
            return XCTFail()
        }
        let after = await f.content.requests().count
        XCTAssertEqual(before, after)
    }
    func testCorruptPrefixRejectedBeforeGET() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        let writer = try FileHandle(
            forWritingTo: f.root.appendingPathComponent(original.id.rawValue + ".part"))
        try writer.write(contentsOf: Data("corrupt".utf8))
        try writer.close()
        guard case .stopped(_, .integrityFailure) = await downloadRun(f, resume: original.id) else {
            return XCTFail()
        }
        let requests = await f.content.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testMissingStagingRejected() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        try FileManager.default.removeItem(
            at: f.root.appendingPathComponent(original.id.rawValue + ".part"))
        guard case .stopped(_, .stagingMissing) = await downloadRun(f, resume: original.id) else {
            return XCTFail()
        }
    }
    func testPartialSurvivesRealDatabaseAndFilesystemReopen() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        let original = try await partial(f)
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let row = try await reopened.download(
            scope: f.queue.scope, credentialId: queueUUID(92),
            id: original.id, bridge: QueueValidator())
        XCTAssertEqual(row, original)
        let staging = try DownloadStagingStore(root: f.root, freeBytes: { _ in UInt64.max })
        let recovery = try await staging.recover(original)
        XCTAssertEqual(recovery, .resume(original.checkpoint))
        let requests = await f.content.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testVerifiedFileSurvivesReopen() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let record = try await downloadRecord(f)
        let reopened = try DownloadStagingStore(root: f.root)
        try await reopened.verify(record, published: true)
        let database = try MutationQueueSQLiteStore(url: f.queue.url)
        let row = try await database.download(
            scope: f.queue.scope, credentialId: queueUUID(92),
            id: record.id, bridge: QueueValidator())
        XCTAssertEqual(row, record)
    }
    func testLostRenameAcknowledgmentReconcilesWithoutNetwork() async throws {
        let fault = DownloadFaultInjector()
        let f = try await downloadFixture(self, filesystemFault: fault)
        fault.arm(.afterRename)
        guard case .stopped(let row, .filesystemFailure) = await downloadRun(f) else {
            return XCTFail()
        }
        let record = try XCTUnwrap(row)
        XCTAssertNotEqual(record.state, .verified)
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".verified").path))
        _ = try downloadCapability(await downloadRun(f, resume: record.id))
        let requests = await f.content.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testLostSQLiteCommitAcknowledgmentReadback() async throws {
        let fault = QueueFaultInjector()
        let f = try await downloadFixture(self, databaseFault: fault)
        fault.arm(.afterCommit)
        _ = try downloadCapability(await downloadRun(f))
        let durable = try await downloadRecord(f)
        XCTAssertEqual(durable.state, .verified)
    }
    func testRenameFailureLeavesPrivatePartial() async throws {
        let fault = DownloadFaultInjector()
        let f = try await downloadFixture(self, filesystemFault: fault)
        fault.arm(.rename)
        guard case .stopped(_, .filesystemFailure) = await downloadRun(f) else { return XCTFail() }
        let record = try await downloadRecord(f)
        XCTAssertNotEqual(record.state, .verified)
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: f.root.appendingPathComponent(record.id.rawValue + ".part").path))
    }
    func testWriteFailureNeverAdvancesOffset() async throws {
        let fault = DownloadFaultInjector()
        let f = try await downloadFixture(self, filesystemFault: fault)
        fault.arm(.write)
        guard case .stopped(_, .filesystemFailure) = await downloadRun(f) else { return XCTFail() }
        let durable = try await downloadRecord(f)
        XCTAssertEqual(durable.checkpoint.offset, 0)
    }
    func testSynchronizationFailureDoesNotAdvanceCheckpoint() async throws {
        let fault = DownloadFaultInjector()
        let f = try await downloadFixture(self, body: largeBody, filesystemFault: fault)
        let original = try await partial(f)
        await f.content.configure()
        fault.arm(.synchronize)
        guard case .stopped(_, .filesystemFailure) = await downloadRun(f, resume: original.id)
        else { return XCTFail() }
        let durable = try await downloadRecord(f)
        XCTAssertEqual(durable.checkpoint, original.checkpoint)
    }
    func testEvictedVerifiedCapabilityUnavailable() async throws {
        let f = try await downloadFixture(self)
        let file = try downloadCapability(await downloadRun(f))
        try FileManager.default.removeItem(
            at: f.root.appendingPathComponent(file.operationId.rawValue + ".verified"))
        do {
            _ = try await f.coordinator.read(file, offset: 0, count: 5)
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .stagingMissing) }
        let durable = try await downloadRecord(f)
        XCTAssertNotEqual(durable.state, .verified)
    }
    func testPreviouslyVerifiedCorruptionUnavailable() async throws {
        let f = try await downloadFixture(self)
        let file = try downloadCapability(await downloadRun(f))
        try Data("HELLO".utf8).write(
            to: f.root.appendingPathComponent(file.operationId.rawValue + ".verified"))
        do {
            _ = try await f.coordinator.read(file, offset: 0, count: 5)
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .sha256Mismatch) }
    }
    func testNoFilenameFromServerUsed() async throws {
        let f = try await downloadFixture(self)
        let expected = try DownloadExpectedContent(length: "5", sha256: downloadDigest(f.body))
        var headers = downloadHeaders(expected)
        headers["Content-Disposition"] = "attachment; filename=\"../../escaped\""
        await f.content.configure(headers: headers)
        let file = try downloadCapability(await downloadRun(f))
        let names = try FileManager.default.contentsOfDirectory(atPath: f.root.path)
        XCTAssertEqual(names, [file.operationId.rawValue + ".verified"])
    }
    func testNoContentBytesOrBearerInSQLite() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let columns = try queueRawScalar(
            f.queue.url, "SELECT sql FROM sqlite_master WHERE name='file_downloads'")
        XCTAssertFalse(columns.contains("BLOB"))
        for url in [f.queue.url, URL(fileURLWithPath: f.queue.url.path + "-wal")] {
            if let data = try? Data(contentsOf: url) {
                XCTAssertNil(data.range(of: Data(queueBearer.utf8)))
            }
        }
    }
    func testDurableProgressAndVerifyingSeparateFromCompletion() async throws {
        let f = try await downloadFixture(self, body: largeBody)
        var progress: [DownloadProgress] = []
        _ = try downloadCapability(
            await f.coordinator.run(library: f.queue.scope.libraryId, node: f.node.id) {
                progress.append($0)
            })
        XCTAssertEqual(progress.last?.state, .verified)
        XCTAssertTrue(
            progress.contains { $0.state == .verifying && $0.durableBytes == $0.totalBytes })
        XCTAssertEqual(progress.map(\.durableBytes), progress.map(\.durableBytes).sorted())
    }
    func testDuplicateActiveRequestSerialized() async throws {
        let f = try await downloadFixture(self)
        let gate = QueueGate()
        await f.content.configure(gate: gate)
        let first = Task { await downloadRun(f) }
        await gate.wait()
        guard case .stopped(nil, .duplicateOperation) = await downloadRun(f) else {
            return XCTFail()
        }
        await gate.release()
        _ = try downloadCapability(await first.value)
        let requests = await f.content.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testVerifiedDuplicateReusesValidatedFile() async throws {
        let f = try await downloadFixture(self)
        let first = try downloadCapability(await downloadRun(f))
        let second = try downloadCapability(await downloadRun(f))
        XCTAssertEqual(first.operationId, second.operationId)
        let requests = await f.content.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testCancelDuringContentPreservesDurableEvidence() async throws {
        let f = try await downloadFixture(self)
        let gate = QueueGate()
        await f.content.configure(gate: gate)
        let task = Task { await downloadRun(f) }
        await gate.wait()
        f.coordinator.cancel()
        await gate.release()
        guard case .stopped(_, .cancelled) = await task.value else { return XCTFail() }
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.state, .cancelled)
        XCTAssertEqual(record.checkpoint.offset, 0)
    }
    func testLogoutFencesLateContent() async throws {
        let f = try await downloadFixture(self)
        let gate = QueueGate()
        await f.content.configure(gate: gate)
        let task = Task { await downloadRun(f) }
        await gate.wait()
        await f.queue.controller.requestLogout()
        await gate.release()
        guard case .stopped = await task.value else { return XCTFail() }
        XCTAssertNotEqual(
            try queueRawScalar(f.queue.url, "SELECT state FROM file_downloads"), "VERIFIED")
    }
    func testCredentialReplacementFencedBeforePublication() async throws {
        let f = try await downloadFixture(self)
        let gate = QueueGate()
        await f.content.configure(gate: gate)
        let task = Task { await downloadRun(f) }
        await gate.wait()
        _ = try await f.queue.credentials.update(
            queueSession(f.queue.scope, credential: 93).record,
            for: f.queue.scope.serverEndpoint)
        await gate.release()
        guard case .stopped(_, .staleSession) = await task.value else { return XCTFail() }
        XCTAssertNotEqual(
            try queueRawScalar(f.queue.url, "SELECT state FROM file_downloads"), "VERIFIED")
    }
    func testCapabilityRevokedAfterLogout() async throws {
        let f = try await downloadFixture(self)
        let file = try downloadCapability(await downloadRun(f))
        await f.queue.controller.requestLogout()
        do {
            _ = try await f.coordinator.read(file, offset: 0, count: 5)
            XCTFail()
        } catch {}
    }
    func testHistorical404StopsWithoutContentGET() async throws {
        let f = try await downloadFixture(self)
        let error = [
            "error": [
                "code": "not_found", "message": "Unavailable", "request_id": "download-request-01",
                "retryable": false,
            ]
        ]
        await f.metadata.setVersion(try queueHTTP(error, status: 404))
        guard case .stopped(_, .sourceUnavailable) = await downloadRun(f) else { return XCTFail() }
        let requests = await f.content.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testDirectoryIneligibleBeforeContentGET() async throws {
        try await ineligible(key: "kind", value: "DIRECTORY")
    }
    func testTrashedFileIneligibleBeforeContentGET() async throws {
        try await ineligible(key: "state", value: "TRASHED")
    }
    func testWrongLibraryCannotDownload() async throws {
        try await ineligible(key: "library_id", value: queueUUID(99))
    }
    private func ineligible(key: String, value: String) async throws {
        let f = try await downloadFixture(self)
        var object =
            try JSONSerialization.jsonObject(with: projectionNodeHTTP(f.node).body)
            as! [String: Any]
        var resource = object["data"] as! [String: Any]
        var attributes = resource["attributes"] as! [String: Any]
        attributes[key] = value
        resource["attributes"] = attributes
        object["data"] = resource
        await f.metadata.setNode(try queueHTTP(object))
        guard case .stopped = await downloadRun(f) else { return XCTFail() }
        let requests = await f.content.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testConditionalRevalidationRequiresValidLocalFile() async throws {
        let f = try await downloadFixture(self)
        let file = try downloadCapability(await downloadRun(f))
        try await f.coordinator.revalidate(file)
        let requests = await f.content.requests()
        XCTAssertEqual(requests.last?.headers["If-None-Match"], file.identity.expected.etag)
        XCTAssertNil(requests.last?.headers["Range"])
    }
    func testViewModelNoAutomaticNetworkOnLoad() async throws {
        let f = try await downloadFixture(self)
        let model = FileDownloadViewModel(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
        await model.loadSaved()
        let metadata = await f.metadata.requests()
        let content = await f.content.requests()
        XCTAssertTrue(metadata.isEmpty)
        XCTAssertTrue(content.isEmpty)
        XCTAssertTrue(model.canStart)
    }
    func testViewModelSessionInvalidatesStatus() async throws {
        let f = try await downloadFixture(self)
        let model = FileDownloadViewModel(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
        await model.download()
        XCTAssertEqual(model.progress?.state, .verified)
        await f.queue.controller.requestLogout()
        XCTAssertFalse(model.isCurrent)
        XCTAssertFalse(model.canResume)
        XCTAssertFalse(model.canStart)
        model.invalidate()
        XCTAssertNil(model.progress)
    }
    func testExplicitRestartPreservesIdentityAfterIntegrityFailure() async throws {
        let f = try await downloadFixture(self)
        await f.content.configure(body: Data("HELLO".utf8))
        _ = await downloadRun(f)
        let original = try await downloadRecord(f)
        await f.content.configure(body: f.body)
        let file = try downloadCapability(
            await f.coordinator.run(
                library: f.queue.scope.libraryId,
                node: f.node.id, resume: original.id, restart: true
            ) { _ in })
        XCTAssertEqual(file.operationId, original.id)
        XCTAssertEqual(file.identity, original.identity)
    }
}

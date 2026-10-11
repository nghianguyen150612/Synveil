import CryptoKit
import Foundation
import XCTest

@testable import Synveil

@MainActor
final class FileUploadStorageTests: XCTestCase {
    func testUploadRecordPreservesCanonicalIntentSessionAndOffsetAcrossReopen() async throws {
        let fixture = try await queueFixture(self)
        let requestSession = try await fixture.provider.begin()
        let target = UploadTarget.createFile(
            library: fixture.scope.libraryId,
            parent: try await NodeId.validated(queueUUID(2), using: QueueValidator()),
            name: "durable.txt")
        let expected = try UploadExpectedContent(
            byteLength: 16, sha256: digest(Data((0..<16).map(UInt8.init))))
        let intent = try await UploadIntent.prepare(
            target: target, expected: expected,
            generator: FixedUploadIDGenerator(value: queueUUID(701)), bridge: QueueValidator())
        let stagingId = UploadOperationId()
        let id = UploadOperationId()
        let record = UploadRecord(
            id: id, stagingId: stagingId, stagingReleased: false,
            intent: intent, scope: fixture.scope,
            credentialId: requestSession.credentialIdentifier,
            originatingSessionId: UUID().uuidString.lowercased(),
            serverSessionId: nil, serverTargetNodeId: nil,
            authoritativeOffset: 0, serverState: nil, state: .sourceStaged,
            uncertain: false, attemptCount: 0, completion: nil, failure: nil)
        try await fixture.database.prepareUpload(record)

        try await fixture.database.updateUpload(
            record, state: .sessionCreating, uncertain: true, beginAttempt: true)
        var stored = try await fixture.database.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        var saved = try XCTUnwrap(stored)
        let node = try await NodeId.validated(queueUUID(3), using: QueueValidator())
        try await fixture.database.updateUpload(
            saved, state: .sessionOpen, uncertain: false,
            serverSessionId: intent.idempotencyKey.rawValue,
            serverTargetNodeId: node, serverState: .open)
        stored = try await fixture.database.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        saved = try XCTUnwrap(stored)
        try await fixture.database.updateUpload(
            saved, state: .uploading, uncertain: true, beginAttempt: true)
        stored = try await fixture.database.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        saved = try XCTUnwrap(stored)
        try await fixture.database.updateUpload(
            saved, state: .uploading, uncertain: false,
            authoritativeOffset: 8, serverState: .open)
        stored = try await fixture.database.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        let advanced = try XCTUnwrap(stored)

        do {
            try await fixture.database.updateUpload(
                advanced, state: .uploading, uncertain: false,
                authoritativeOffset: 4, serverState: .open)
            XCTFail("A saved server offset must never rewind")
        } catch { XCTAssertEqual(error as? UploadFailure, .invalidMetadata) }

        let reopened = try MutationQueueSQLiteStore(url: fixture.url)
        stored = try await reopened.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        let restored = try XCTUnwrap(stored)
        XCTAssertEqual(restored.intent.requestBytes, intent.requestBytes)
        XCTAssertEqual(restored.intent.idempotencyKey.rawValue, queueUUID(701))
        XCTAssertEqual(restored.serverSessionId, queueUUID(701))
        XCTAssertEqual(restored.authoritativeOffset, 8)
        XCTAssertEqual(restored.state, .uploading)

        try await reopened.updateUpload(
            restored, state: .readyToComplete, uncertain: false,
            authoritativeOffset: 16, serverState: .open)
        stored = try await reopened.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        let ready = try XCTUnwrap(stored)
        try await reopened.updateUpload(
            ready, state: .completing, uncertain: true, beginAttempt: true)
        stored = try await reopened.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        let completing = try XCTUnwrap(stored)
        let completion = UploadCompletion(
            sessionId: queueUUID(701), nodeId: node,
            fileVersionId: try await FileVersionId.validated(
                queueUUID(702), using: QueueValidator()),
            revision: try NodeRevision(validating: "4"), expected: expected,
            committedAt: Date(timeIntervalSince1970: 1_791_720_000), requestId: "upload-commit-01")
        try await reopened.updateUpload(
            completing, state: .committed, uncertain: false,
            authoritativeOffset: 16, serverState: .committed, completion: completion)
        stored = try await reopened.upload(
            scope: fixture.scope, credentialId: requestSession.credentialIdentifier,
            id: id, bridge: QueueValidator())
        let committed = try XCTUnwrap(stored)
        XCTAssertEqual(committed.state, .committed)
        XCTAssertEqual(committed.completion?.fileVersionId, completion.fileVersionId)
        XCTAssertEqual(committed.completion?.expected, expected)
    }

    func testV6ToV7MigrationPreservesExistingMutationEvidence() async throws {
        let fixture = try await queueFixture(self)
        let original = try await queueEnqueued(fixture)
        try queueRawSQL(fixture.url, "DROP TRIGGER file_upload_identity")
        try queueRawSQL(fixture.url, "DROP TRIGGER file_upload_offset")
        try queueRawSQL(fixture.url, "DROP TABLE file_uploads")
        try queueRawSQL(fixture.url, "PRAGMA user_version=6")

        let migrated = try MutationQueueSQLiteStore(url: fixture.url)
        XCTAssertEqual(try queueRawScalar(fixture.url, "PRAGMA user_version"), "7")
        let retained = try await migrated.records(scope: fixture.scope, limit: 10)
        XCTAssertEqual(retained.first?.mutationId, original.mutation.id.rawValue)
        XCTAssertEqual(retained.first?.request, original.mutation.requestBody)
        XCTAssertEqual(try queueRawScalar(fixture.url, "SELECT count(*) FROM file_uploads"), "0")
    }

    func testStagedSourceIsPrivateRehashedAndReadInFourMiBChunks() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "SynveilUploadStorage-" + UUID().uuidString)
        try FileManager.default.createDirectory(
            at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("provider-source.bin")
        let root = directory.appendingPathComponent("private-staging", isDirectory: true)
        let size = 16 * 1024 * 1024 + 123
        let bytes = Data((0..<size).map { UInt8(truncatingIfNeeded: $0 &* 31) })
        try bytes.write(to: source)
        let staging = try DownloadStagingStore(root: root, freeBytes: { _ in 2_000_000_000 })
        let id = UploadOperationId()

        let expected = try await staging.stageUpload(
            from: source, stagingId: id, durable: [])
        XCTAssertEqual(expected.byteLength, UInt64(size))
        XCTAssertEqual(expected.sha256, Self.digest(bytes))
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: root.appendingPathComponent(id.rawValue + ".upload").path))
        try await staging.verifyUploadSource(stagingId: id, expected: expected)

        var offset: UInt64 = 0
        var reconstructed = Data()
        while offset < expected.byteLength {
            let count = Int(
                min(
                    UInt64(UploadPolicy.chunkBytes), expected.byteLength - offset))
            let chunk = try await staging.uploadChunk(
                stagingId: id, expected: expected, offset: offset, count: count)
            reconstructed.append(chunk)
            offset += UInt64(count)
        }
        XCTAssertEqual(reconstructed, bytes)
        XCTAssertEqual(reconstructed.count, size)

        try await staging.reconcileOrphanUploads(keeping: [id.rawValue])
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: root.appendingPathComponent(id.rawValue + ".upload").path))
        try await staging.reconcileOrphanUploads(keeping: [])
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: root.appendingPathComponent(id.rawValue + ".upload").path))
    }

    func testInsufficientReserveDoesNotPublishUploadSource() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "SynveilUploadReserve-" + UUID().uuidString)
        try FileManager.default.createDirectory(
            at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("small.bin")
        try Data([1, 2, 3]).write(to: source)
        let root = directory.appendingPathComponent("private-staging", isDirectory: true)
        let staging = try DownloadStagingStore(
            root: root, freeBytes: { _ in DownloadPolicy.freeReserve - 1 })
        let id = UploadOperationId()

        do {
            _ = try await staging.stageUpload(from: source, stagingId: id, durable: [])
            XCTFail("Expected the free-space reserve to reject the import")
        } catch { XCTAssertEqual(UploadPolicy.classify(error), .insufficientStorage) }
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: root.appendingPathComponent(id.rawValue + ".upload").path))
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: root.appendingPathComponent(id.rawValue + ".upload.incomplete").path))
    }

    private func digest(_ data: Data) -> String { Self.digest(data) }

    private static func digest(_ data: Data) -> String {
        "sha256:" + SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }
}

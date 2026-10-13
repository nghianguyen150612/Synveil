import CryptoKit
import Foundation
import XCTest

@testable import Synveil

@MainActor
final class FileUploadCoordinatorTests: XCTestCase {
    func testLostCreatePartialPatchAndLostCompletionRecoverOnOriginalIntent() async throws {
        let fixture = try await queueFixture(self, initializeBase: false)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let sourceURL = FileManager.default.temporaryDirectory.appendingPathComponent(
            "SynveilUploadRecovery-" + UUID().uuidString + ".bin")
        let size = 16 * 1024 * 1024 + 123
        let source = Data((0..<size).map { UInt8(truncatingIfNeeded: $0 &* 37) })
        try source.write(to: sourceURL)
        addTeardownBlock { try? FileManager.default.removeItem(at: sourceURL) }

        let scripted = UploadCoordinatorProvider(
            base: fixture.provider, library: fixture.scope.libraryId,
            parent: parent, source: source, loseFirstCreate: true,
            partiallyAcceptFirstPatch: 65_537, loseCompletion: true)
        let provider: any AuthenticatedUploadRequestProviderProtocol = scripted
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "SynveilUploadRecoveryStage-" + UUID().uuidString, isDirectory: true)
        let staging = try DownloadStagingStore(
            root: directory, freeBytes: { _ in 2_000_000_000 })
        let coordinator = FileUploadCoordinator(
            repository: AuthenticatedUploadRepository(provider: provider, bridge: QueueValidator()),
            database: fixture.database, staging: UploadStagingStore(shared: staging),
            identityGenerator: FixedUploadIDGenerator(value: queueUUID(703)))
        let result = await coordinator.start(
            selectedURL: sourceURL,
            target: .createFile(
                library: fixture.scope.libraryId, parent: parent, name: "archive.bin"),
            library: fixture.scope.libraryId, libraryStatus: .active,
            progress: { _ in })
        guard case .stopped(let uncertain?, let createFailure) = result else {
            return XCTFail("A lost create response must remain recoverable")
        }
        XCTAssertEqual(createFailure, .timeout)
        XCTAssertEqual(uncertain.state, .outcomeUnknown)
        XCTAssertNil(uncertain.serverSessionId)

        let resumed = await coordinator.resume(
            id: uncertain.id, library: fixture.scope.libraryId,
            libraryStatus: .active, progress: { _ in })
        guard case .completed(let completed) = resumed else {
            return XCTFail("Status reconciliation should prove the original commit")
        }
        XCTAssertEqual(completed.state, .committed)
        XCTAssertTrue(completed.stagingReleased)
        XCTAssertEqual(completed.authoritativeOffset, UInt64(size))
        XCTAssertEqual(completed.completion?.nodeId.rawValue, queueUUID(3))
        XCTAssertEqual(completed.completion?.fileVersionId.rawValue, queueUUID(704))
        XCTAssertEqual(completed.intent.idempotencyKey.rawValue, queueUUID(703))

        XCTAssertEqual(scripted.createBodies.count, 2)
        XCTAssertEqual(scripted.createBodies[0], scripted.createBodies[1])
        XCTAssertEqual(scripted.statusCount, 2)
        XCTAssertEqual(scripted.patchRequests.count, 5)
        XCTAssertEqual(scripted.patchRequests[0].offset, 0)
        XCTAssertEqual(
            scripted.patchRequests[0].bytes, source.subdata(in: 0..<UploadPolicy.chunkBytes))
        XCTAssertEqual(scripted.patchRequests[1].offset, 65_537)
        for request in scripted.patchRequests.dropFirst() {
            let start = Int(request.offset)
            let end = start + request.bytes.count
            XCTAssertEqual(request.bytes, source.subdata(in: start..<end))
        }
        XCTAssertEqual(scripted.finalOffset, UInt64(size))
        XCTAssertEqual(scripted.completionCalls, 1)
        XCTAssertEqual(completed.intent.expected.sha256, Self.digest(source))

        let stored = try await fixture.database.upload(
            scope: fixture.scope, credentialId: queueUUID(92),
            id: completed.id, bridge: QueueValidator())
        XCTAssertEqual(stored?.completion?.fileVersionId.rawValue, queueUUID(704))
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: directory.appendingPathComponent(
                    completed.stagingId.rawValue + ".upload"
                ).path))
    }

    private static func digest(_ data: Data) -> String {
        "sha256:" + SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }
}

@MainActor
final class UploadCoordinatorProvider: AuthenticatedUploadRequestProviderProtocol {
    struct CapturedPatch {
        let offset: UInt64
        let bytes: Data
    }

    private let base: AuthenticatedLibraryRequestProvider
    private let library: LibraryId
    private let parent: NodeId
    private let source: Data
    private let loseFirstCreate: Bool
    private let partialAcceptance: Int
    private let loseCompletion: Bool
    private var intent: UploadIntent?
    private var serverOffset: UInt64 = 0
    private var serverState: UploadServerState = .open
    private var completion: UploadCompletion?
    private var didLoseCreate = false
    private var didPartiallyAccept = false
    private var didLoseCompletion = false

    private(set) var createBodies: [Data] = []
    private(set) var patchRequests: [CapturedPatch] = []
    private(set) var statusCount = 0
    private(set) var completionCalls = 0

    var finalOffset: UInt64 { serverOffset }
    var statusFailure: UploadFailure?
    var onStatusResponse: (() -> Void)?
    func acceptBackground(to offset: UInt64) { serverOffset = offset }
    func backgroundUploadRequest(
        record: UploadRecord, chunk: BackgroundChunkIntent,
        session: LibraryRequestScope
    ) async throws -> HTTPTransportRequest {
        try await base.backgroundUploadRequest(record: record, chunk: chunk, session: session)
    }

    init(
        base: AuthenticatedLibraryRequestProvider, library: LibraryId, parent: NodeId,
        source: Data, loseFirstCreate: Bool, partiallyAcceptFirstPatch: Int,
        loseCompletion: Bool
    ) {
        self.base = base
        self.library = library
        self.parent = parent
        self.source = source
        self.loseFirstCreate = loseFirstCreate
        partialAcceptance = partiallyAcceptFirstPatch
        self.loseCompletion = loseCompletion
    }

    func begin() async throws -> LibraryRequestScope { try await base.begin() }
    func validate(_ scope: LibraryRequestScope) async throws { try await base.validate(scope) }
    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope) {
        base.handle(failure, scope: scope)
    }

    func requestPage(cursor: String?, scope: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        let object: [String: Any] = [
            "data": [
                [
                    "id": library.rawValue, "type": "library", "revision": "1",
                    "attributes": [
                        "name": "Upload test", "root_node_id": parent.rawValue,
                        "status": "ACTIVE", "created_at": "2026-10-11T12:00:00Z",
                        "updated_at": "2026-10-11T12:00:00Z",
                    ],
                ]
            ],
            "page": ["has_more": false], "meta": ["request_id": "upload-library-01"],
        ]
        return try queueHTTP(object)
    }

    func requestNode(nodeId: NodeId, scope: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        let resource: [String: Any] = [
            "id": parent.rawValue, "type": "node", "revision": "1",
            "attributes": [
                "library_id": library.rawValue, "name": "Root", "kind": "DIRECTORY",
                "state": "ACTIVE", "created_at": "2026-10-11T12:00:00Z",
                "updated_at": "2026-10-11T12:00:00Z", "purge_eligible": false,
            ],
        ]
        return try queueHTTP([
            "data": resource, "meta": ["request_id": "upload-parent-01"],
        ])
    }

    func requestChildrenPage(
        libraryId: LibraryId, parent: NodeParentScope, cursor: String?,
        scope: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        HTTPTransportResponse(statusCode: 404)
    }

    func createUploadSession(intent: UploadIntent, session: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        createBodies.append(intent.requestBytes)
        if let previous = self.intent {
            guard previous == intent else { throw UploadFailure.conflict }
        } else {
            self.intent = intent
            if case .createFile = intent.target { serverState = .open }
        }
        if loseFirstCreate, !didLoseCreate {
            didLoseCreate = true
            throw UploadFailure.timeout
        }
        return try sessionResponse(intent: intent, status: 201, includeLocation: true)
    }

    func uploadStatus(sessionId: String, session: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        statusCount += 1
        if let statusFailure { throw statusFailure }
        guard sessionId == intent?.idempotencyKey.rawValue, let intent else {
            throw UploadFailure.invalidMetadata
        }
        let response = try sessionResponse(intent: intent, status: 200, includeLocation: false)
        onStatusResponse?()
        return response
    }

    func patchUploadChunk(
        sessionId: String, offset: UInt64, bytes: Data, session: LibraryRequestScope
    ) async throws -> HTTPTransportResponse {
        guard let intent, sessionId == intent.idempotencyKey.rawValue,
            serverState == .open, offset == serverOffset
        else { throw UploadFailure.offsetMismatch }
        let start = Int(offset)
        let end = start + bytes.count
        guard end <= source.count, bytes == source.subdata(in: start..<end) else {
            throw UploadFailure.integrityMismatch
        }
        patchRequests.append(CapturedPatch(offset: offset, bytes: bytes))
        if !didPartiallyAccept {
            didPartiallyAccept = true
            serverOffset += UInt64(min(partialAcceptance, bytes.count))
            throw UploadFailure.timeout
        }
        serverOffset += UInt64(bytes.count)
        return HTTPTransportResponse(
            statusCode: 204,
            headers: [
                "Upload-Offset": String(serverOffset), "X-Request-ID": "upload-patch-01",
            ])
    }

    func completeUpload(sessionId: String, session: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        completionCalls += 1
        guard let intent, sessionId == intent.idempotencyKey.rawValue,
            serverOffset == intent.expected.byteLength
        else { throw UploadFailure.offsetMismatch }
        completion = UploadCompletion(
            sessionId: sessionId,
            nodeId: try await NodeId.validated(queueUUID(3), using: QueueValidator()),
            fileVersionId: try await FileVersionId.validated(
                queueUUID(704), using: QueueValidator()),
            revision: try NodeRevision(validating: "2"), expected: intent.expected,
            committedAt: Date(timeIntervalSince1970: 1_791_720_000),
            requestId: "upload-complete-01")
        serverState = .committed
        if loseCompletion, !didLoseCompletion {
            didLoseCompletion = true
            throw UploadFailure.timeout
        }
        return try completionResponse(sessionId: sessionId)
    }

    func abortUpload(sessionId: String, session: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    {
        serverState = .aborted
        guard let intent else { throw UploadFailure.invalidMetadata }
        return try sessionResponse(intent: intent, status: 200, includeLocation: false)
    }

    private func sessionResponse(
        intent: UploadIntent, status: Int, includeLocation: Bool
    ) throws -> HTTPTransportResponse {
        let target: [String: Any]
        switch intent.target {
        case .createFile(let library, let parent, let name):
            target = [
                "operation": "CREATE_FILE", "library_id": library.rawValue,
                "parent_id": parent.rawValue, "node_id": queueUUID(3), "name": name,
            ]
        case .replaceContent(let library, let node, let revision):
            target = [
                "operation": "REPLACE_CONTENT", "library_id": library.rawValue,
                "node_id": node.rawValue, "expected_revision": revision.rawValue,
            ]
        }
        var attributes: [String: Any] = [
            "operation": intent.target.operation, "state": serverState.rawValue,
            "received_bytes": String(serverOffset),
            "expected_bytes": String(intent.expected.byteLength),
            "expected_sha256": intent.expected.sha256,
            "created_at": "2026-10-11T12:00:00Z",
            "updated_at": "2026-10-11T12:00:00Z",
            "expires_at": "2026-10-12T12:00:00Z", "target": target,
        ]
        if let completion {
            attributes["completion"] = completionResource(completion)
        }
        var headers = [
            "Content-Type": "application/json", "Upload-Offset": String(serverOffset),
            "X-Request-ID": "upload-status-01",
        ]
        if includeLocation {
            headers["Location"] = "/api/v1/upload-sessions/\(intent.idempotencyKey.rawValue)"
        }
        let object: [String: Any] = [
            "data": [
                "id": intent.idempotencyKey.rawValue, "type": "upload_session",
                "attributes": attributes,
            ],
            "meta": ["request_id": "upload-status-01"],
        ]
        return HTTPTransportResponse(
            statusCode: status, headers: headers,
            body: try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]))
    }

    private func completionResponse(sessionId: String) throws -> HTTPTransportResponse {
        guard let completion else { throw UploadFailure.invalidMetadata }
        let object: [String: Any] = [
            "data": completionResource(completion),
            "meta": ["request_id": completion.requestId],
        ]
        return HTTPTransportResponse(
            statusCode: 200,
            headers: ["Content-Type": "application/json", "X-Request-ID": completion.requestId],
            body: try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]))
    }

    private func completionResource(_ completion: UploadCompletion) -> [String: Any] {
        [
            "id": completion.sessionId, "type": "upload_completion",
            "attributes": [
                "node_id": completion.nodeId.rawValue,
                "file_version_id": completion.fileVersionId.rawValue,
                "node_revision": completion.revision.rawValue,
                "bytes": String(completion.expected.byteLength),
                "sha256": completion.expected.sha256,
                "committed_at": "2026-10-11T12:00:00Z",
            ],
        ]
    }
}

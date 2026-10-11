import CryptoKit
import Foundation
import XCTest

@testable import Synveil

func downloadDigest(_ data: Data) -> String {
    "sha256:" + SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}
func downloadVersionObject(
    version: String = queueUUID(501), node: String = queueUUID(1),
    data: Data = Data("hello".utf8)
) -> [String: Any] {
    [
        "data": [
            "id": version, "type": "file_version",
            "attributes": [
                "node_id": node, "byte_length": String(data.count), "sha256": downloadDigest(data),
                "created_at": "2026-10-11T01:00:00Z", "is_current": true,
            ],
        ],
        "meta": ["request_id": "download-request-01"],
    ]
}
func downloadHeaders(_ expected: DownloadExpectedContent, range: DownloadRange? = nil) -> [String:
    String]
{
    var headers = [
        "Content-Type": "application/octet-stream", "ETag": expected.etag,
        "Content-Length": String(range?.length ?? expected.byteLength),
        "Cache-Control": "private, no-store", "Accept-Ranges": "bytes",
    ]
    if let range {
        headers["Content-Range"] = "bytes \(range.start)-\(range.end)/\(expected.byteLength)"
    }
    return headers
}

actor DownloadMetadataTransport: HTTPTransportProtocol {
    var nodeResponse: HTTPTransportResponse
    var versionResponse: HTTPTransportResponse
    private var sent: [HTTPTransportRequest] = []
    init(node: HTTPTransportResponse, version: HTTPTransportResponse) {
        nodeResponse = node
        versionResponse = version
    }
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        sent.append(request)
        return request.url.path.contains("/versions/") ? versionResponse : nodeResponse
    }
    func setNode(_ response: HTTPTransportResponse) { nodeResponse = response }
    func setVersion(_ response: HTTPTransportResponse) { versionResponse = response }
    func requests() -> [HTTPTransportRequest] { sent }
}

actor DownloadContentFixture: FileContentTransportProtocol {
    var body: Data
    var status: Int?
    var overrideHeaders: [String: String]?
    var failAfterChunks: Int?
    var gate: QueueGate?
    private var sent: [HTTPTransportRequest] = []
    init(_ body: Data) { self.body = body }
    func configure(
        status: Int? = nil, headers: [String: String]? = nil,
        failAfterChunks: Int? = nil, gate: QueueGate? = nil, body: Data? = nil
    ) {
        self.status = status
        overrideHeaders = headers
        self.failAfterChunks = failAfterChunks
        self.gate = gate
        if let body { self.body = body }
    }
    func requests() -> [HTTPTransportRequest] { sent }
    func stream(
        _ request: HTTPTransportRequest, expected: DownloadExpectedContent, range: DownloadRange?,
        revalidating: Bool, receive: @escaping @Sendable (Data) async throws -> Void
    ) async throws {
        sent.append(request)
        if let gate {
            self.gate = nil
            await gate.arrive()
        }
        try Task.checkCancellation()
        let status = self.status ?? (revalidating ? 304 : (range == nil ? 200 : 206))
        let headers =
            overrideHeaders
            ?? (revalidating ? ["ETag": expected.etag] : downloadHeaders(expected, range: range))
        let response = try DownloadResponseMetadata.validate(
            status: status, headers: headers,
            expected: expected, range: range, revalidating: revalidating)
        if response.notModified { return }
        let start = Int(range?.start ?? 0)
        var count = 0
        var offset = start
        let end = min(body.count, start + Int(response.length))
        while offset < end {
            try Task.checkCancellation()
            let next = min(end, offset + DownloadPolicy.bufferSize)
            try await receive(body.subdata(in: offset..<next))
            count += 1
            if count == failAfterChunks { throw DownloadFailure.timeout }
            offset = next
        }
        guard UInt64(end - start) == response.length, body.count <= Int(expected.byteLength) else {
            throw DownloadFailure.contentLengthMismatch
        }
    }
}

final class DownloadFaultInjector: @unchecked Sendable {
    private let lock = NSLock()
    private var armed: DownloadFilesystemFault?
    func arm(_ point: DownloadFilesystemFault) {
        lock.lock()
        armed = point
        lock.unlock()
    }
    func hit(_ point: DownloadFilesystemFault) throws {
        lock.lock()
        defer { lock.unlock() }
        if armed == point {
            armed = nil
            throw DownloadFailure.filesystemFailure
        }
    }
}

struct DownloadFixture {
    let queue: QueueFixture
    let node: Node
    let version: FileVersionId
    let root: URL
    let metadata: DownloadMetadataTransport
    let content: DownloadContentFixture
    let provider: AuthenticatedLibraryRequestProvider
    let staging: DownloadStagingStore
    let coordinator: FileDownloadCoordinator
    let body: Data
}

@MainActor
func downloadFixture(
    _ test: XCTestCase, body: Data = Data("hello".utf8),
    databaseFault: QueueFaultInjector? = nil,
    filesystemFault: DownloadFaultInjector? = nil,
    free: UInt64 = UInt64.max
) async throws -> DownloadFixture {
    let queue = try await queueFixture(test, fault: databaseFault)
    let version = try await FileVersionId.validated(queueUUID(501), using: QueueValidator())
    let node = Node(
        id: try await NodeId.validated(queueUUID(1), using: QueueValidator()),
        libraryId: queue.scope.libraryId,
        parentId: try await NodeId.validated(queueUUID(2), using: QueueValidator()),
        currentVersionId: version, revision: try NodeRevision(validating: "3"),
        name: "../../private.txt",
        kind: .file, state: .active, createdAt: Date(), updatedAt: Date(), trashedAt: nil,
        restoreDeadline: nil, purgeEligible: false)
    let metadata = DownloadMetadataTransport(
        node: try projectionNodeHTTP(node),
        version: try queueHTTP(downloadVersionObject(data: body)))
    let content = DownloadContentFixture(body)
    let provider = AuthenticatedLibraryRequestProvider(
        controller: queue.controller,
        store: queue.credentials, transport: metadata, contentTransport: content)
    let root = queue.url.deletingLastPathComponent().resolvingSymlinksInPath()
        .appendingPathComponent("downloads")
    let faultCallback: (@Sendable (DownloadFilesystemFault) throws -> Void)?
    if let filesystemFault {
        faultCallback = { try filesystemFault.hit($0) }
    } else {
        faultCallback = nil
    }
    let staging = try DownloadStagingStore(
        root: root, freeBytes: { _ in free }, fault: faultCallback)
    let coordinator = FileDownloadCoordinator(
        repository: AuthenticatedFileDownloadRepository(
            provider: provider, bridge: QueueValidator()), database: queue.database,
        staging: staging)
    queue.controller.installMutationSessionInvalidator {
        [weak coordinator, weak durableQueue = queue.queue] in
        coordinator?.invalidateSession()
        durableQueue?.invalidateSession()
    }
    return DownloadFixture(
        queue: queue, node: node, version: version, root: root, metadata: metadata,
        content: content, provider: provider, staging: staging, coordinator: coordinator, body: body
    )
}

@MainActor
func downloadRun(_ f: DownloadFixture, resume: DownloadOperationId? = nil) async
    -> FileDownloadResult
{
    await f.coordinator.run(library: f.queue.scope.libraryId, node: f.node.id, resume: resume) {
        _ in
    }
}
@MainActor
func downloadRecord(_ f: DownloadFixture) async throws -> DownloadRecord {
    let row = try await f.queue.database.download(
        scope: f.queue.scope,
        credentialId: queueUUID(92), version: f.version, bridge: QueueValidator())
    return try XCTUnwrap(row)
}
@MainActor
func downloadCapability(_ result: FileDownloadResult) throws -> VerifiedDownloadedFile {
    guard case .completed(let file) = result else { throw DownloadFailure.integrityFailure }
    return file
}

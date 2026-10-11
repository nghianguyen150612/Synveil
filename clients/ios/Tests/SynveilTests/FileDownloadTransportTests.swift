import CryptoKit
import Foundation
import XCTest

@testable import Synveil

private struct DownloadURLFixture: Sendable {
    let status: Int
    let headers: [String: String]
    let body: Data?
    let repeatedBytes: Int
    let error: URLError?
    let redirect: Bool
}
private final class DownloadURLFixtureRegistry: @unchecked Sendable {
    private let lock = NSLock()
    private var fixtures: [URL: DownloadURLFixture] = [:]
    func set(_ fixture: DownloadURLFixture, for url: URL) {
        lock.lock()
        fixtures[url] = fixture
        lock.unlock()
    }
    func get(_ url: URL) -> DownloadURLFixture? {
        lock.lock()
        defer { lock.unlock() }
        return fixtures[url]
    }
    func remove(_ url: URL) {
        lock.lock()
        fixtures[url] = nil
        lock.unlock()
    }
}
private final class DownloadURLProtocol: URLProtocol, @unchecked Sendable {
    static let registry = DownloadURLFixtureRegistry()
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        guard let url = request.url, let fixture = Self.registry.get(url) else {
            client?.urlProtocol(self, didFailWithError: URLError(.resourceUnavailable))
            return
        }
        if let error = fixture.error {
            client?.urlProtocol(self, didFailWithError: error)
            return
        }
        guard
            let response = HTTPURLResponse(
                url: url, statusCode: fixture.status, httpVersion: "HTTP/1.1",
                headerFields: fixture.headers)
        else { return }
        if fixture.redirect {
            client?.urlProtocol(
                self,
                wasRedirectedTo: URLRequest(url: URL(string: "https://other.example/content")!),
                redirectResponse: response)
            return
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        if let body = fixture.body {
            for start in stride(from: 0, to: body.count, by: 4096) {
                client?.urlProtocol(
                    self, didLoad: body.subdata(in: start..<min(start + 4096, body.count)))
            }
        } else {
            let chunk = Data(repeating: 97, count: 4096)
            for _ in 0..<(fixture.repeatedBytes / 4096) {
                client?.urlProtocol(self, didLoad: chunk)
            }
        }
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private actor DownloadTransportSink {
    private var count: UInt64 = 0
    private var maximum = 0
    private var hasher = SHA256()
    func receive(_ data: Data) {
        count += UInt64(data.count)
        maximum = max(maximum, data.count)
        hasher.update(data: data)
    }
    func evidence() -> (UInt64, Int, String) {
        (count, maximum, "sha256:" + hasher.finalize().map { String(format: "%02x", $0) }.joined())
    }
}

final class FileDownloadTransportTests: XCTestCase {
    private func errorBody(_ code: String) -> Data {
        Data(
            "{\"error\":{\"code\":\"\(code)\",\"message\":\"Unavailable\",\"request_id\":\"download-request-01\",\"retryable\":false}}"
                .utf8)
    }

    private func stream(
        status: Int = 200, body: Data = Data("hello".utf8), range: DownloadRange? = nil,
        override: [String: String]? = nil, revalidating: Bool = false,
        error: URLError? = nil, redirect: Bool = false
    ) async throws -> (UInt64, Int, String) {
        let expected = try DownloadExpectedContent(
            length: "5", sha256: downloadDigest(Data("hello".utf8)))
        let url = URL(
            string: "https://" + UUID().uuidString.lowercased() + ".example/api/v1/versions/"
                + queueUUID(501) + "/content")!
        let headers = override ?? downloadHeaders(expected, range: range)
        DownloadURLProtocol.registry.set(
            DownloadURLFixture(
                status: status, headers: headers, body: body,
                repeatedBytes: 0, error: error, redirect: redirect), for: url)
        defer { DownloadURLProtocol.registry.remove(url) }
        var requestHeaders = [
            "Accept": "application/octet-stream", "Accept-Encoding": "identity",
            "Authorization": "Bearer " + queueBearer,
        ]
        requestHeaders["Range"] = range?.header
        requestHeaders["If-None-Match"] = revalidating ? expected.etag : nil
        let sink = DownloadTransportSink()
        try await URLSessionFileContentTransport(protocolClasses: [DownloadURLProtocol.self])
            .stream(
                HTTPTransportRequest(url: url, headers: requestHeaders), expected: expected,
                range: range,
                revalidating: revalidating, receive: { await sink.receive($0) })
        return await sink.evidence()
    }
    private func rejected(
        status: Int = 200, body: Data = Data("hello".utf8), range: DownloadRange? = nil,
        override: [String: String]? = nil, failure: DownloadFailure
    ) async {
        do {
            _ = try await stream(status: status, body: body, range: range, override: override)
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, failure) }
    }
    func testNativeURLSession200() async throws {
        let evidence = try await stream()
        XCTAssertEqual(evidence.0, 5)
        XCTAssertEqual(evidence.2, downloadDigest(Data("hello".utf8)))
    }
    func testNativeURLSession206() async throws {
        let evidence = try await stream(
            status: 206, body: Data("ello".utf8), range: DownloadRange(start: 1, end: 4, total: 5))
        XCTAssertEqual(evidence.0, 4)
    }
    func testNativeTruncated200() async {
        await rejected(body: Data("hel".utf8), failure: .contentLengthMismatch)
    }
    func testNativeOversized200() async {
        await rejected(body: Data("hello!".utf8), failure: .contentLengthMismatch)
    }
    func testNativeUnexpectedRange200() async throws {
        await rejected(range: try DownloadRange(start: 1, end: 4, total: 5), failure: .invalidRange)
    }
    func testNative304WithoutVerifiedLocalContext() async {
        await rejected(status: 304, body: Data(), failure: .unexpectedStatus)
    }
    func testNativeConditional304() async throws {
        let evidence = try await stream(
            status: 304, body: Data(),
            override: ["ETag": "\"\(downloadDigest(Data("hello".utf8)))\""], revalidating: true)
        XCTAssertEqual(evidence.0, 0)
    }
    func testNative416ReconciliationRequired() async throws {
        await rejected(
            status: 416, body: errorBody("invalid_range"),
            range: try DownloadRange(start: 1, end: 4, total: 5),
            override: ["Content-Range": "bytes */5", "Content-Type": "application/json"],
            failure: .invalidRange)
    }
    func testNativeHTTPErrorNeverDeliveredToSink() async {
        await rejected(
            status: 404, body: errorBody("not_found"),
            override: ["Content-Type": "application/json"], failure: .sourceUnavailable)
    }
    func testNativeTLSFailureClassified() async {
        do {
            _ = try await stream(error: URLError(.serverCertificateUntrusted))
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .tlsFailure) }
    }
    func testNativeCrossOriginRedirectRejected() async {
        do {
            _ = try await stream(status: 302, redirect: true)
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .redirectRejected) }
    }
    func testHTTPSRequiredBeforeDispatch() async throws {
        let expected = try DownloadExpectedContent(
            length: "5", sha256: downloadDigest(Data("hello".utf8)))
        do {
            try await URLSessionFileContentTransport(protocolClasses: [DownloadURLProtocol.self])
                .stream(
                    HTTPTransportRequest(url: URL(string: "http://127.0.0.1/content")!),
                    expected: expected,
                    range: nil, receive: { _ in XCTFail("Insecure content delivered") })
            XCTFail()
        } catch { XCTAssertEqual(error as? DownloadFailure, .scopeMismatch) }
    }
    @MainActor
    func testNativeLargeResponseHasBoundedSinkAndIncrementalHash() async throws {
        let size = 16 * 1024 * 1024
        let chunk = Data(repeating: 97, count: 4096)
        var hasher = SHA256()
        for _ in 0..<(size / chunk.count) { hasher.update(data: chunk) }
        let digest = "sha256:" + hasher.finalize().map { String(format: "%02x", $0) }.joined()
        let expected = try DownloadExpectedContent(length: String(size), sha256: digest)
        let url = URL(string: "https://" + UUID().uuidString.lowercased() + ".example/content")!
        DownloadURLProtocol.registry.set(
            DownloadURLFixture(
                status: 200, headers: downloadHeaders(expected),
                body: nil, repeatedBytes: size, error: nil, redirect: false), for: url)
        defer { DownloadURLProtocol.registry.remove(url) }
        let sink = DownloadTransportSink()
        try await URLSessionFileContentTransport(protocolClasses: [DownloadURLProtocol.self])
            .stream(
                HTTPTransportRequest(
                    url: url,
                    headers: [
                        "Accept": "application/octet-stream", "Accept-Encoding": "identity",
                        "Authorization": "Bearer " + queueBearer,
                    ]),
                expected: expected, range: nil, receive: { await sink.receive($0) })
        let evidence = await sink.evidence()
        XCTAssertEqual(evidence.0, UInt64(size))
        XCTAssertLessThanOrEqual(evidence.1, DownloadPolicy.bufferSize)
        XCTAssertEqual(evidence.2, digest)
        // Exercise the same generated response through production checkpointing, CryptoKit,
        // private filesystem rename and a real file-backed SQLite connection.
        let f = try await downloadFixture(self, body: Data())
        var object = downloadVersionObject(data: Data())
        var resource = object["data"] as! [String: Any]
        var attributes = resource["attributes"] as! [String: Any]
        attributes["byte_length"] = String(size)
        attributes["sha256"] = digest
        resource["attributes"] = attributes
        object["data"] = resource
        await f.metadata.setVersion(try queueHTTP(object))
        let contentURL = URL(
            string: f.queue.scope.serverEndpoint.urlString + "/api/v1/versions/"
                + f.version.rawValue + "/content")!
        DownloadURLProtocol.registry.set(
            DownloadURLFixture(
                status: 200, headers: downloadHeaders(expected),
                body: nil, repeatedBytes: size, error: nil, redirect: false), for: contentURL)
        defer { DownloadURLProtocol.registry.remove(contentURL) }
        let provider = AuthenticatedLibraryRequestProvider(
            controller: f.queue.controller,
            store: f.queue.credentials, transport: f.metadata,
            contentTransport: URLSessionFileContentTransport(protocolClasses: [
                DownloadURLProtocol.self
            ]))
        let coordinator = FileDownloadCoordinator(
            repository: AuthenticatedFileDownloadRepository(
                provider: provider, bridge: QueueValidator()), database: f.queue.database,
            staging: f.staging)
        let file = try downloadCapability(
            await coordinator.run(library: f.queue.scope.libraryId, node: f.node.id) { _ in })
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.checkpoint.offset, UInt64(size))
        XCTAssertEqual(record.state, .verified)
        let prefix = try await coordinator.read(file, offset: 0, count: 4096)
        XCTAssertEqual(prefix, chunk)
    }
}

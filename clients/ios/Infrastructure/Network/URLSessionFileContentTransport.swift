import Foundation

private final class DownloadRedirectDelegate: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) { completionHandler(nil) }
}

/// Foreground AsyncBytes with backpressure: at most 64 KiB of application-owned content is
/// buffered. Header validation precedes the first write; task cancellation stops AsyncBytes.
final class URLSessionFileContentTransport: FileContentTransportProtocol, Sendable {
    private let session: URLSession
    deinit { session.invalidateAndCancel() }
    init(protocolClasses: [AnyClass]? = nil) {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 30
        configuration.timeoutIntervalForResource = 300
        configuration.httpCookieStorage = nil
        configuration.httpCookieAcceptPolicy = .never
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalAndRemoteCacheData
        configuration.protocolClasses = protocolClasses
        session = URLSession(
            configuration: configuration, delegate: DownloadRedirectDelegate(), delegateQueue: nil)
    }

    func stream(
        _ request: HTTPTransportRequest, expected: DownloadExpectedContent, range: DownloadRange?,
        revalidating: Bool = false, receive: @escaping @Sendable (Data) async throws -> Void
    ) async throws {
        guard request.url.scheme == "https", request.url.host != nil,
            request.url.user == nil, request.url.password == nil,
            request.url.query == nil, request.url.fragment == nil,
            request.method == .get, request.body == nil,
            request.headers["Accept"] == "application/octet-stream",
            request.headers["Accept-Encoding"] == "identity",
            request.headers["Range"] == range?.header,
            request.headers["If-None-Match"] == (revalidating ? expected.etag : nil),
            !request.headers.keys.contains(where: {
                ["cookie", "x-csrf-token", "if-range"].contains($0.lowercased())
            })
        else { throw DownloadFailure.scopeMismatch }
        var outbound = URLRequest(url: request.url)
        outbound.httpShouldHandleCookies = false
        for (key, value) in request.headers { outbound.setValue(value, forHTTPHeaderField: key) }
        do {
            try Task.checkCancellation()
            let (bytes, response) = try await session.bytes(for: outbound)
            // Explicitly cancel on header rejection, overflow or sink failure as well as Task
            // cancellation; dropping an iterator is not used as a cancellation guarantee.
            defer { bytes.task.cancel() }
            guard let response = response as? HTTPURLResponse, response.url == request.url else {
                throw DownloadFailure.scopeMismatch
            }
            var headers: [String: String] = [:]
            for (key, value) in response.allHeaderFields {
                headers[String(describing: key)] = String(describing: value)
            }
            if response.statusCode >= 400 {
                // Errors are bounded strict JSON, never handed to a staging sink. Device revocation
                // is a server error code (including HTTP 401), not an inference from HTTP 403.
                var errorBody = Data()
                for try await byte in bytes {
                    guard errorBody.count < 16 * 1024 else {
                        throw DownloadFailure.unexpectedStatus
                    }
                    errorBody.append(byte)
                }
                guard response.mimeType == "application/json",
                    let code = try? LibraryResponseDecoder.errorCode(errorBody)
                else { throw DownloadFailure.unexpectedStatus }
                if response.statusCode == 401 {
                    if code == "device_revoked" { throw DownloadFailure.deviceRevoked }
                    if code == "authentication_failed" { throw DownloadFailure.unauthenticated }
                    throw DownloadFailure.unexpectedStatus
                }
                if response.statusCode == 403 { throw DownloadFailure.scopeMismatch }
                if response.statusCode == 404 { throw DownloadFailure.sourceUnavailable }
                if response.statusCode >= 500 { throw DownloadFailure.serverUnavailable }
                if response.statusCode == 416, code == "invalid_range", range != nil,
                    response.value(forHTTPHeaderField: "Content-Range")
                        == "bytes */\(expected.byteLength)"
                {
                    throw DownloadFailure.invalidRange
                }
                throw DownloadFailure.unexpectedStatus
            }
            let metadata = try DownloadResponseMetadata.validate(
                status: response.statusCode, headers: headers, expected: expected,
                range: range, revalidating: revalidating)
            var count: UInt64 = 0
            var buffer = Data()
            buffer.reserveCapacity(DownloadPolicy.bufferSize)
            for try await byte in bytes {
                try Task.checkCancellation()
                guard count < metadata.length else { throw DownloadFailure.contentLengthMismatch }
                count += 1
                buffer.append(byte)
                if buffer.count == DownloadPolicy.bufferSize {
                    try await receive(buffer)
                    buffer.removeAll(keepingCapacity: true)
                }
            }
            guard count == metadata.length else { throw DownloadFailure.contentLengthMismatch }
            try Task.checkCancellation()
            if !buffer.isEmpty { try await receive(buffer) }
        } catch { throw DownloadPolicy.classify(error) }
    }
}

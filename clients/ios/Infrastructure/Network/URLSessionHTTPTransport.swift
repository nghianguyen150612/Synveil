import Foundation

/// Delegate that explicitly rejects all HTTP redirects to prevent silent
/// cross-origin/endpoint redirection.
private final class RedirectRejectingDelegate:
    NSObject, URLSessionDataDelegate, @unchecked Sendable
{
    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        // Rejecting redirect by passing nil ensures URLSession returns the 3xx response directly.
        completionHandler(nil)
    }
}

/// Bounded maximum response body size default (64 KiB).
public let maxProbeResponseBodyBytes: Int = 64 * 1024

/// Production URLSession-backed implementation of `HTTPTransportProtocol`.
public final class URLSessionHTTPTransport: HTTPTransportProtocol, @unchecked Sendable {
    private let session: URLSession
    private let userAgent: String
    private let maxResponseBodyBytes: Int
    private let allowLoopbackTestHttp: Bool

    /// Initializes `URLSessionHTTPTransport` with explicit timeout and policy settings.
    ///
    /// - Parameters:
    ///   - userAgent: The User-Agent string to include on outbound requests
    ///     (default: "Synveil/0.1.0 (iOS)").
    ///   - requestTimeout: Timeout interval for single requests in seconds (default: 10s).
    ///   - resourceTimeout: Timeout interval for entire resource transfer in seconds (default: 15s).
    ///   - maxResponseBodyBytes: Maximum allowed response body size in bytes (default: 64 KiB).
    ///   - allowLoopbackTestHttp: Whether cleartext HTTP is permitted for numeric
    ///     loopback (`127.0.0.1`, `[::1]`).
    public init(
        userAgent: String = "Synveil/0.1.0 (iOS)",
        requestTimeout: TimeInterval = 10.0,
        resourceTimeout: TimeInterval = 15.0,
        maxResponseBodyBytes: Int = maxProbeResponseBodyBytes,
        allowLoopbackTestHttp: Bool = false
    ) {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = requestTimeout
        configuration.timeoutIntervalForResource = resourceTimeout
        configuration.httpCookieAcceptPolicy = .never
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalAndRemoteCacheData

        let delegate = RedirectRejectingDelegate()
        self.session = URLSession(
            configuration: configuration,
            delegate: delegate,
            delegateQueue: nil
        )
        self.userAgent = userAgent
        self.maxResponseBodyBytes = maxResponseBodyBytes
        self.allowLoopbackTestHttp = allowLoopbackTestHttp
    }

    /// Sends an HTTP request and returns the bounded response.
    ///
    /// - Parameter request: The outbound request details.
    /// - Throws: `SynveilTransportError` on transport, configuration, or size-limit failures.
    public func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        try validateSchemeAndHost(request.url)

        var urlRequest = URLRequest(url: request.url)
        urlRequest.httpMethod = request.method.rawValue
        urlRequest.httpBody = request.body

        // Set default headers if not provided
        var headers = request.headers
        if headers["Accept"] == nil && headers["accept"] == nil {
            headers["Accept"] = "application/json"
        }
        if headers["Accept-Encoding"] == nil && headers["accept-encoding"] == nil {
            headers["Accept-Encoding"] = "identity"
        }
        if headers["User-Agent"] == nil && headers["user-agent"] == nil {
            headers["User-Agent"] = userAgent
        }

        for (key, value) in headers {
            urlRequest.setValue(value, forHTTPHeaderField: key)
        }

        do {
            let (bytesAsync, response) = try await session.bytes(for: urlRequest)

            guard let httpResponse = response as? HTTPURLResponse else {
                throw SynveilTransportError.malformedResponse
            }

            var accumulated = Data()
            accumulated.reserveCapacity(min(self.maxResponseBodyBytes, 8192))

            for try await byte in bytesAsync {
                accumulated.append(byte)
                if accumulated.count > self.maxResponseBodyBytes {
                    throw SynveilTransportError.bodyLimitExceeded
                }
            }

            var responseHeaders: [String: String] = [:]
            for (key, value) in httpResponse.allHeaderFields {
                let keyString = String(describing: key)
                let valueString = String(describing: value)
                responseHeaders[keyString] = valueString
            }

            return HTTPTransportResponse(
                statusCode: httpResponse.statusCode,
                headers: responseHeaders,
                body: accumulated
            )
        } catch let error as SynveilTransportError {
            throw error
        } catch let urlError as URLError {
            throw mapURLError(urlError)
        } catch is CancellationError {
            throw SynveilTransportError.cancelled
        } catch {
            throw SynveilTransportError.offline
        }
    }

    /// Validates transport scheme and host policy.
    private func validateSchemeAndHost(_ url: URL) throws {
        guard let scheme = url.scheme?.lowercased() else {
            throw SynveilTransportError.configurationError
        }

        if scheme == "https" {
            return
        }

        if scheme == "http" {
            guard allowLoopbackTestHttp, let host = url.host?.lowercased() else {
                throw SynveilTransportError.configurationError
            }
            if host == "127.0.0.1" || host == "::1" || host == "[::1]" {
                return
            }
            throw SynveilTransportError.configurationError
        }

        throw SynveilTransportError.configurationError
    }

    /// Maps Foundation `URLError` to typed `SynveilTransportError`.
    private func mapURLError(_ urlError: URLError) -> SynveilTransportError {
        switch urlError.code {
        case .notConnectedToInternet, .cannotConnectToHost, .networkConnectionLost:
            return .offline
        case .cannotFindHost, .dnsLookupFailed:
            return .dnsFailure
        case .timedOut:
            return .timeout
        case .serverCertificateUntrusted,
            .serverCertificateHasBadDate,
            .serverCertificateHasUnknownRoot,
            .serverCertificateNotYetValid,
            .secureConnectionFailed,
            .clientCertificateRejected,
            .clientCertificateRequired:
            return .tlsError
        case .cancelled:
            return .cancelled
        default:
            return .offline
        }
    }
}

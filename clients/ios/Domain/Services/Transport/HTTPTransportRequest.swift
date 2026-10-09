import Foundation

/// Value object representing an outbound HTTP transport request.
public struct HTTPTransportRequest: Sendable, Equatable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    /// Requests may contain authorization headers or sensitive query/body values.
    public var description: String { "[REDACTED_HTTP_REQUEST]" }
    public var debugDescription: String { description }

    public let url: URL
    public let method: HTTPMethod
    public let headers: [String: String]
    public let body: Data?

    public init(
        url: URL,
        method: HTTPMethod = .get,
        headers: [String: String] = [:],
        body: Data? = nil
    ) {
        self.url = url
        self.method = method
        self.headers = headers
        self.body = body
    }
}

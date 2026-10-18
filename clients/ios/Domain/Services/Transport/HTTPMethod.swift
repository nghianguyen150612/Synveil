import Foundation

/// Supported HTTP request methods for Synveil API transports.
public enum HTTPMethod: String, Sendable, Equatable, Hashable, CaseIterable {
    case get = "GET"
    case post = "POST"
    case put = "PUT"
    case patch = "PATCH"
    case delete = "DELETE"
    case head = "HEAD"
}

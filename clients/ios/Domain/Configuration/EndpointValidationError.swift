import Foundation

/// Defines domain validation errors when attempting to parse or construct a `ServerEndpoint`.
public enum EndpointValidationError: Error, Equatable, Sendable, LocalizedError {
    case empty
    case invalidURL
    case unsupportedScheme(String)
    case missingHost
    case userinfoNotAllowed
    case queryNotAllowed
    case fragmentNotAllowed
    case invalidPath(String)

    public var errorDescription: String? {
        switch self {
        case .empty:
            return "Server endpoint URL cannot be empty."
        case .invalidURL:
            return "The provided server endpoint is not a valid URL."
        case .unsupportedScheme(let scheme):
            return
                "Unsupported URL scheme '\(scheme)'. "
                + "Synveil server endpoints require 'https' or 'http'."
        case .missingHost:
            return "Server endpoint URL is missing a host component."
        case .userinfoNotAllowed:
            return
                "Server endpoint URL must not contain "
                + "username or password userinfo credentials."
        case .queryNotAllowed:
            return "Server endpoint URL must not contain query parameters."
        case .fragmentNotAllowed:
            return "Server endpoint URL must not contain a URL fragment."
        case .invalidPath(let reason):
            return "Invalid server endpoint URL path: \(reason)"
        }
    }
}

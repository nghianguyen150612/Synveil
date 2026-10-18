import Foundation

/// Defines domain validation errors when attempting to parse or construct a `ServerEndpoint`.
public enum EndpointValidationError: Error, Equatable, Sendable, LocalizedError {
    case empty
    case invalidURL
    case unsupportedScheme(String)
    case tooLong
    case controlOrWhitespace
    case backslashNotAllowed
    case httpsRequired
    case loopbackHTTPNotAllowed
    case missingHost
    case invalidHost
    case invalidPort
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
                + "Synveil server endpoints require HTTPS."
        case .tooLong:
            return "Server endpoint URL must not exceed 2048 UTF-8 bytes."
        case .controlOrWhitespace:
            return "Server endpoint URL must not contain whitespace or control characters."
        case .backslashNotAllowed:
            return "Server endpoint URL must not contain backslashes."
        case .httpsRequired:
            return
                "HTTPS is required. Cleartext HTTP is available only for numeric loopback "
                + "in debug builds."
        case .loopbackHTTPNotAllowed:
            return "Cleartext HTTP is limited to numeric loopback addresses in debug builds."
        case .missingHost:
            return "Server endpoint URL is missing a host component."
        case .invalidHost:
            return "Server endpoint host is malformed."
        case .invalidPort:
            return "Server endpoint port must be an integer from 1 to 65535."
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

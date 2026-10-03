import Foundation

/// Strongly typed, normalized value semantics for a Synveil server base endpoint.
///
/// Ensures valid scheme (https or http for local dev), valid host, optional port, and normalized path,
/// while strictly rejecting credentials, query strings, and fragments.
public struct ServerEndpoint: Equatable, Hashable, Sendable, CustomStringConvertible {
    /// The canonical underlying URL representing the server base endpoint.
    public let url: URL

    /// Canonical string representation of the endpoint (guaranteed no trailing slash unless root path requires it).
    public var urlString: String {
        url.absoluteString
    }

    /// URL scheme (normalized to lowercase, e.g. "https" or "http").
    public var scheme: String {
        url.scheme ?? ""
    }

    /// Server host name or IP address (normalized to lowercase).
    public var host: String {
        url.host ?? ""
    }

    /// Explicit port number if non-default.
    public var port: Int? {
        url.port
    }

    /// Path component (normalized, e.g., "/" or "/synveil").
    public var path: String {
        url.path.isEmpty ? "/" : url.path
    }

    /// Indicates if transport scheme is HTTPS.
    public var isSecureScheme: Bool {
        scheme == "https"
    }

    public var description: String {
        urlString
    }

    /// Validates and constructs a normalized `ServerEndpoint` from a raw URL string.
    ///
    /// - Parameter rawValue: Raw string input from user entry or configuration.
    /// - Throws: `EndpointValidationError` if the URL is syntactically invalid or violates safety policies.
    public init(validating rawValue: String) throws {
        let trimmed = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw EndpointValidationError.empty
        }

        guard let components = URLComponents(string: trimmed) else {
            throw EndpointValidationError.invalidURL
        }

        // Scheme validation
        guard let rawScheme = components.scheme, !rawScheme.isEmpty else {
            throw EndpointValidationError.unsupportedScheme("")
        }

        let schemeLower = rawScheme.lowercased()
        guard schemeLower == "https" || schemeLower == "http" else {
            throw EndpointValidationError.unsupportedScheme(schemeLower)
        }

        // Host validation
        guard let rawHost = components.host, !rawHost.isEmpty else {
            throw EndpointValidationError.missingHost
        }
        let hostLower = rawHost.lowercased()

        // Prohibited components check
        if components.user != nil || components.password != nil {
            throw EndpointValidationError.userinfoNotAllowed
        }

        if let query = components.query, !query.isEmpty {
            throw EndpointValidationError.queryNotAllowed
        }

        if let fragment = components.fragment, !fragment.isEmpty {
            throw EndpointValidationError.fragmentNotAllowed
        }

        // Path normalization: root path is "/", subpaths have trailing slash trimmed (e.g. "/api/v1")
        var normalizedPath = components.path
        if normalizedPath.isEmpty {
            normalizedPath = "/"
        } else if normalizedPath.hasSuffix("/") && normalizedPath.count > 1 {
            normalizedPath = String(normalizedPath.dropLast())
        }

        // Reconstruct canonical URLComponents
        var normalizedComponents = URLComponents()
        normalizedComponents.scheme = schemeLower
        normalizedComponents.host = hostLower
        normalizedComponents.port = components.port
        normalizedComponents.path = normalizedPath

        guard let normalizedURL = normalizedComponents.url else {
            throw EndpointValidationError.invalidURL
        }

        self.url = normalizedURL
    }
}

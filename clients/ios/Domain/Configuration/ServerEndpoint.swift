import Foundation

/// Strongly typed, normalized value semantics for a Synveil server base endpoint.
///
/// Ensures a valid scheme, host, optional port, and canonical origin,
/// while strictly rejecting credentials, query strings, and fragments.
public struct ServerEndpoint: Equatable, Hashable, Sendable, CustomStringConvertible {
    /// Matches Android's debug-only numeric loopback HTTP exception.
    public static var defaultAllowsLoopbackTestHTTP: Bool {
        #if DEBUG
            true
        #else
            false
        #endif
    }

    /// The canonical underlying URL representing the server base endpoint.
    public let url: URL

    /// Canonical string representation of the origin, including its root slash.
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

    /// Path component, always `/` for a server origin.
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

    /// Validates and constructs a canonical server origin from a raw URL string.
    ///
    /// - Parameter rawValue: Raw string input from user entry or configuration.
    /// - Parameter allowLoopbackTestHTTP: Whether numeric loopback HTTP is permitted for a debug
    ///   or test build. Other cleartext hosts are rejected in every build.
    /// - Throws: `EndpointValidationError` if the URL is syntactically invalid or violates safety
    ///   policies.
    public init(
        validating rawValue: String,
        allowLoopbackTestHTTP: Bool = ServerEndpoint.defaultAllowsLoopbackTestHTTP
    ) throws {
        guard !rawValue.isEmpty else {
            throw EndpointValidationError.empty
        }

        guard rawValue.utf8.count <= 2_048 else {
            throw EndpointValidationError.tooLong
        }

        if rawValue.unicodeScalars.contains(where: {
            CharacterSet.controlCharacters.contains($0)
                || CharacterSet.whitespacesAndNewlines.contains($0)
        }) {
            throw EndpointValidationError.controlOrWhitespace
        }
        if rawValue.contains("\\") {
            throw EndpointValidationError.backslashNotAllowed
        }

        guard let components = URLComponents(string: rawValue) else {
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
        try Self.validateNumericIPv4Host(hostLower)

        // Prohibited components check
        if components.user != nil || components.password != nil {
            throw EndpointValidationError.userinfoNotAllowed
        }

        if components.percentEncodedQuery != nil {
            throw EndpointValidationError.queryNotAllowed
        }

        if components.percentEncodedFragment != nil {
            throw EndpointValidationError.fragmentNotAllowed
        }

        // ServerEndpoint is an origin, matching Android's CanonicalServerOrigin. API paths
        // are owned by the client and cannot be redirected by user-provided base paths.
        let rawPath = components.percentEncodedPath
        guard rawPath.isEmpty || rawPath == "/" else {
            throw EndpointValidationError.invalidPath("API base paths are not allowed")
        }

        try Self.validateAuthorityAndPort(rawValue)

        if schemeLower == "http" {
            guard allowLoopbackTestHTTP else {
                throw EndpointValidationError.httpsRequired
            }
            guard Self.isNumericLoopback(hostLower) else {
                throw EndpointValidationError.loopbackHTTPNotAllowed
            }
        }

        let normalizedPath = "/"

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

    private static func isNumericLoopback(_ host: String) -> Bool {
        host == "127.0.0.1" || host == "::1"
    }

    private static func validateNumericIPv4Host(_ host: String) throws {
        guard host.contains("."),
            host.allSatisfy({ $0.isNumber || $0 == "." })
        else { return }
        let octets = host.split(separator: ".", omittingEmptySubsequences: false)
        guard octets.count == 4, octets.allSatisfy(Self.isValidIPv4Octet) else {
            throw EndpointValidationError.invalidHost
        }
    }

    private static func isValidIPv4Octet(_ octet: Substring) -> Bool {
        guard !octet.isEmpty,
            octet.utf8.allSatisfy({ (48...57).contains($0) }),
            let value = Int(String(octet))
        else { return false }
        return (0...255).contains(value)
    }

    private static func validateAuthorityAndPort(_ rawValue: String) throws {
        guard let schemeRange = rawValue.range(of: "://") else {
            throw EndpointValidationError.invalidURL
        }
        let remainder = rawValue[schemeRange.upperBound...]
        let authority = remainder.prefix { $0 != "/" && $0 != "?" && $0 != "#" }
        guard !authority.isEmpty else { return }

        let portText: Substring?
        if authority.first == "[" {
            guard let close = authority.firstIndex(of: "]") else {
                throw EndpointValidationError.invalidURL
            }
            let suffix = authority[authority.index(after: close)...]
            if suffix.isEmpty {
                portText = nil
            } else if suffix.first == ":" {
                portText = suffix.dropFirst()
            } else {
                throw EndpointValidationError.invalidURL
            }
        } else {
            let colons = authority.filter { $0 == ":" }.count
            guard colons <= 1 else {
                throw EndpointValidationError.invalidURL
            }
            if let colon = authority.firstIndex(of: ":") {
                portText = authority[authority.index(after: colon)...]
            } else {
                portText = nil
            }
        }

        guard let portText else { return }
        let digits = portText.unicodeScalars
        guard !digits.isEmpty,
            digits.allSatisfy({ (48...57).contains($0.value) }),
            let port = Int(String(portText)), (1...65_535).contains(port)
        else {
            throw EndpointValidationError.invalidPort
        }
    }
}

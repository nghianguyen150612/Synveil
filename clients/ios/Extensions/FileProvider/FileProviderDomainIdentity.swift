import CryptoKit
import Foundation

/// Opaque identity for one server/account/device credential generation.
///
/// A domain groups all Libraries in a single v0.1 device session. Its visible identifier contains
/// only a versioned digest, never an endpoint, account identifier, or credential value.
struct FileProviderDomainIdentity: Codable, Hashable, Sendable {
    private static let synveilIDPattern = try! NSRegularExpression(
        pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")

    let rawValue: String
    let ownerScopeFingerprint: String
    let credentialGenerationFingerprint: String

    init(
        endpointOrigin: String,
        accountIdentity: String,
        deviceIdentity: String,
        credentialGeneration: String
    ) throws {
        let endpoint = try Self.canonicalOrigin(endpointOrigin)
        try Self.validateOpaqueIdentity(accountIdentity)
        try Self.validateOpaqueIdentity(deviceIdentity)
        try Self.validateOpaqueIdentity(credentialGeneration)

        let ownerScope = Self.fingerprint([
            "synveil-owner-v1", endpoint, accountIdentity, deviceIdentity,
        ])
        let credentialScope = Self.fingerprint([
            "synveil-credential-generation-v1", credentialGeneration,
        ])
        let domainScope = Self.fingerprint([
            "synveil-file-provider-domain-v1", endpoint, accountIdentity, deviceIdentity,
            credentialGeneration,
        ])

        rawValue = "synveil-fp-v1-\(domainScope)"
        ownerScopeFingerprint = ownerScope
        credentialGenerationFingerprint = credentialScope
    }

    var fingerprint: String { String(rawValue.dropFirst("synveil-fp-v1-".count)) }

    private static func canonicalOrigin(_ value: String) throws -> String {
        guard let components = URLComponents(string: value),
            let scheme = components.scheme?.lowercased(),
            let host = components.host?.lowercased(),
            !host.isEmpty,
            components.user == nil,
            components.password == nil,
            components.query == nil,
            components.fragment == nil,
            components.path.isEmpty || components.path == "/"
        else { throw FileProviderIdentityError.invalidScope }

        let allowedHTTPS = scheme == "https"
        let allowedLoopbackHTTP = scheme == "http" && ["127.0.0.1", "::1"].contains(host)
        guard allowedHTTPS || allowedLoopbackHTTP else {
            throw FileProviderIdentityError.invalidScope
        }

        let defaultPort = scheme == "https" ? 443 : 80
        let port = components.port.flatMap { $0 == defaultPort ? nil : $0 }
        let formattedHost = host.contains(":") ? "[\(host)]" : host
        return "\(scheme)://\(formattedHost)\(port.map { ":\($0)" } ?? "")"
    }

    private static func validateOpaqueIdentity(_ value: String) throws {
        guard isValidSynveilIdentifier(value),
            !value.hasPrefix("svd1_"), !value.hasPrefix("sve1_"),
            !value.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
        else { throw FileProviderIdentityError.invalidScope }
    }

    static func isValidSynveilIdentifier(_ value: String) -> Bool {
        let range = NSRange(value.startIndex..<value.endIndex, in: value)
        return synveilIDPattern.firstMatch(in: value, range: range)?.range == range
    }

    static func fingerprint(_ values: [String]) -> String {
        var input = Data()
        for value in values {
            var length = UInt32(value.utf8.count).bigEndian
            withUnsafeBytes(of: &length) { input.append(contentsOf: $0) }
            input.append(contentsOf: value.utf8)
        }
        return SHA256.hash(data: input).map { String(format: "%02x", $0) }.joined()
    }
}

enum FileProviderIdentityError: Error, Equatable {
    case invalidScope
    case invalidItemIdentity
    case scopeMismatch
    case unsupportedIdentity
}

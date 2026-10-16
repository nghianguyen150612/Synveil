import CryptoKit
import Foundation

/// Stable item identifiers bound to one domain and Library.
///
/// Node identifiers are encoded losslessly, while endpoint/account/device scope and Library scope
/// are bound by SHA-256 fingerprints. Display names and local paths never participate.
struct FileProviderItemIdentity: Codable, Hashable, Sendable {
    enum Kind: String, Codable, Sendable {
        case domainRoot = "root"
        case libraryRoot = "library"
        case node
    }

    let rawValue: String
    let kind: Kind
    let domainFingerprint: String
    let libraryFingerprint: String?
    let resolvedIdentifier: String?

    static func domainRoot(domain: FileProviderDomainIdentity) -> Self {
        Self(
            rawValue: "sfpi1.root.\(domain.fingerprint).-.", kind: .domainRoot,
            domainFingerprint: domain.fingerprint, libraryFingerprint: nil,
            resolvedIdentifier: nil
        )
    }

    static func libraryRoot(
        domain: FileProviderDomainIdentity, libraryID: String
    ) throws -> Self {
        try validateIdentifier(libraryID)
        let libraryScope = FileProviderDomainIdentity.fingerprint(["synveil-library-v1", libraryID])
        let encoded = encode(libraryID)
        return Self(
            rawValue: "sfpi1.library.\(domain.fingerprint).\(libraryScope).\(encoded)",
            kind: .libraryRoot, domainFingerprint: domain.fingerprint,
            libraryFingerprint: libraryScope, resolvedIdentifier: libraryID
        )
    }

    static func node(
        domain: FileProviderDomainIdentity, libraryID: String, nodeID: String
    ) throws -> Self {
        try validateIdentifier(libraryID)
        try validateIdentifier(nodeID)
        let libraryScope = FileProviderDomainIdentity.fingerprint(["synveil-library-v1", libraryID])
        return Self(
            rawValue: "sfpi1.node.\(domain.fingerprint).\(libraryScope).\(encode(nodeID))",
            kind: .node, domainFingerprint: domain.fingerprint,
            libraryFingerprint: libraryScope, resolvedIdentifier: nodeID
        )
    }

    init(
        rawValue: String,
        expectedDomain: FileProviderDomainIdentity,
        expectedLibraryID: String? = nil
    ) throws {
        let parts = rawValue.split(separator: ".", omittingEmptySubsequences: false)
            .map(String.init)
        guard parts.count == 5, parts[0] == "sfpi1",
            let kind = Kind(rawValue: parts[1]), parts[2] == expectedDomain.fingerprint
        else { throw FileProviderIdentityError.scopeMismatch }

        if kind == .domainRoot {
            guard parts[3] == "-", parts[4].isEmpty else {
                throw FileProviderIdentityError.invalidItemIdentity
            }
            self.init(
                rawValue: rawValue, kind: kind, domainFingerprint: parts[2],
                libraryFingerprint: nil, resolvedIdentifier: nil
            )
            return
        }

        guard parts[3].count == 64, parts[3].allSatisfy(Self.isLowercaseHex),
            let identifier = Self.decode(parts[4])
        else { throw FileProviderIdentityError.invalidItemIdentity }
        try Self.validateIdentifier(identifier)
        if kind == .libraryRoot {
            let expectedLibraryFingerprint = FileProviderDomainIdentity.fingerprint([
                "synveil-library-v1", identifier,
            ])
            guard expectedLibraryFingerprint == parts[3] else {
                throw FileProviderIdentityError.invalidItemIdentity
            }
        }
        if let expectedLibraryID {
            let expectedFingerprint = FileProviderDomainIdentity.fingerprint(
                ["synveil-library-v1", expectedLibraryID])
            guard parts[3] == expectedFingerprint else {
                throw FileProviderIdentityError.scopeMismatch
            }
        }
        self.init(
            rawValue: rawValue, kind: kind, domainFingerprint: parts[2],
            libraryFingerprint: parts[3], resolvedIdentifier: identifier
        )
    }

    private init(
        rawValue: String, kind: Kind, domainFingerprint: String,
        libraryFingerprint: String?, resolvedIdentifier: String?
    ) {
        self.rawValue = rawValue
        self.kind = kind
        self.domainFingerprint = domainFingerprint
        self.libraryFingerprint = libraryFingerprint
        self.resolvedIdentifier = resolvedIdentifier
    }

    private static func validateIdentifier(_ value: String) throws {
        guard FileProviderDomainIdentity.isValidSynveilIdentifier(value)
        else { throw FileProviderIdentityError.invalidItemIdentity }
    }

    private static func encode(_ value: String) -> String {
        Data(value.utf8).base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    private static func decode(_ value: String) -> String? {
        guard
            value.utf8.allSatisfy({
                (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0)
                    || $0 == 45 || $0 == 95
            })
        else {
            return nil
        }
        var base64 = value.replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
        let padding = (4 - base64.count % 4) % 4
        base64 += String(repeating: "=", count: padding)
        guard let data = Data(base64Encoded: base64),
            let decoded = String(data: data, encoding: .utf8),
            encode(decoded) == value
        else { return nil }
        return decoded
    }

    private static func isLowercaseHex(_ character: Character) -> Bool {
        let bytes = Array(String(character).utf8)
        guard bytes.count == 1 else { return false }
        return (48...57).contains(bytes[0]) || (97...102).contains(bytes[0])
    }
}

/// FileVersion identity is distinct from the mutable Node identity and is scope-bound.
struct FileProviderVersionIdentity: Codable, Hashable, Sendable {
    let rawValue: String
    let fileVersionID: String

    init(
        domain: FileProviderDomainIdentity,
        libraryID: String,
        nodeID: String,
        fileVersionID: String
    ) throws {
        let identities = [libraryID, nodeID, fileVersionID]
        guard identities.allSatisfy(FileProviderDomainIdentity.isValidSynveilIdentifier) else {
            throw FileProviderIdentityError.invalidItemIdentity
        }
        let libraryScope = FileProviderDomainIdentity.fingerprint(["synveil-library-v1", libraryID])
        self.fileVersionID = fileVersionID
        rawValue = [
            "sfpv1", domain.fingerprint, libraryScope,
            Self.encode(nodeID), Self.encode(fileVersionID),
        ].joined(separator: ".")
    }

    private static func encode(_ value: String) -> String {
        Data(value.utf8).base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

/// Maps server evidence to bounded opaque File Provider version components.
/// Missing server revisions remain absent and are never replaced with a local timestamp.
struct FileProviderItemVersionContract: Equatable, Sendable {
    let contentVersion: Data?
    let metadataVersion: Data?

    init(fileVersionIdentity: FileProviderVersionIdentity?, metadataRevision: String?) throws {
        if let metadataRevision {
            let containsControlCharacters = metadataRevision.unicodeScalars.contains {
                CharacterSet.controlCharacters.contains($0)
            }
            guard !metadataRevision.isEmpty, metadataRevision.utf8.count <= 256,
                !containsControlCharacters
            else {
                throw FileProviderIdentityError.invalidItemIdentity
            }
            let revisionEvidence = Data(("synveil-metadata-revision-v1:" + metadataRevision).utf8)
            metadataVersion = Data(SHA256.hash(data: revisionEvidence))
        } else {
            metadataVersion = nil
        }

        if let fileVersionIdentity {
            contentVersion = Data(SHA256.hash(data: Data(fileVersionIdentity.rawValue.utf8)))
        } else {
            contentVersion = nil
        }
    }
}

import Foundation

enum FileProviderContentManifestError: Error, Equatable {
    case unsupportedSchema
    case domainMismatch
    case ownerScopeMismatch
    case invalidField
    case corruptManifest
    case manifestTooLarge
    case metadataGenerationMismatch
}

struct FileProviderContentManifest: Codable, Equatable, Sendable {
    static let currentSchemaVersion = 1
    static let maximumManifestBytes = 64 * 1024

    let schemaVersion: Int
    let domainIdentity: String
    let ownerScopeFingerprint: String
    let authorizationGeneration: UInt64
    let libraryID: String
    let nodeID: String
    let fileVersionID: String
    let expectedByteLength: UInt64
    let expectedSHA256: String
    let contentID: String
    let storageKey: String
    let publicationGeneration: UInt64
    let verifiedState: String
    let metadataGeneration: UInt64

    init(
        schemaVersion: Int = Self.currentSchemaVersion,
        domainIdentity: FileProviderDomainIdentity,
        ownerScopeFingerprint: String,
        authorizationGeneration: UInt64,
        libraryID: String,
        nodeID: String,
        fileVersionID: String,
        expectedByteLength: UInt64,
        expectedSHA256: String,
        contentID: String,
        storageKey: String,
        publicationGeneration: UInt64,
        verifiedState: String = "verified",
        metadataGeneration: UInt64
    ) {
        self.schemaVersion = schemaVersion
        self.domainIdentity = domainIdentity.rawValue
        self.ownerScopeFingerprint = ownerScopeFingerprint
        self.authorizationGeneration = authorizationGeneration
        self.libraryID = libraryID
        self.nodeID = nodeID
        self.fileVersionID = fileVersionID
        self.expectedByteLength = expectedByteLength
        self.expectedSHA256 = expectedSHA256
        self.contentID = contentID
        self.storageKey = storageKey
        self.publicationGeneration = publicationGeneration
        self.verifiedState = verifiedState
        self.metadataGeneration = metadataGeneration
    }

    func validate(
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String
    ) throws -> Self {
        guard schemaVersion == Self.currentSchemaVersion else {
            throw FileProviderContentManifestError.unsupportedSchema
        }
        guard domainIdentity == expectedDomain.rawValue else {
            throw FileProviderContentManifestError.domainMismatch
        }
        guard ownerScopeFingerprint == expectedOwnerScopeFingerprint,
            ownerScopeFingerprint == expectedDomain.ownerScopeFingerprint
        else {
            throw FileProviderContentManifestError.ownerScopeMismatch
        }
        guard authorizationGeneration > 0,
            !libraryID.isEmpty, libraryID.utf8.count <= 256,
            !nodeID.isEmpty, nodeID.utf8.count <= 256,
            !fileVersionID.isEmpty, fileVersionID.utf8.count <= 256,
            Self.isValidSHA256(expectedSHA256),
            !contentID.isEmpty, contentID.utf8.count <= 256,
            !storageKey.isEmpty, storageKey.utf8.count <= 256,
            FileProviderSharedContainer.isSafePathComponent(storageKey),
            publicationGeneration > 0,
            verifiedState == "verified",
            metadataGeneration > 0
        else {
            throw FileProviderContentManifestError.invalidField
        }
        return self
    }

    func encoded(
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String
    ) throws -> Data {
        let validated = try validate(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(validated)
        guard data.count <= Self.maximumManifestBytes else {
            throw FileProviderContentManifestError.manifestTooLarge
        }
        return data
    }

    static func decode(
        _ data: Data,
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String
    ) throws -> Self {
        guard data.count <= Self.maximumManifestBytes else {
            throw FileProviderContentManifestError.manifestTooLarge
        }
        let decoder = JSONDecoder()
        let decoded: Self
        do {
            decoded = try decoder.decode(Self.self, from: data)
        } catch {
            throw FileProviderContentManifestError.corruptManifest
        }
        return try decoded.validate(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint)
    }

    private static func isValidSHA256(_ value: String) -> Bool {
        guard value.hasPrefix("sha256:"), value.utf8.count == 71 else { return false }
        return value.utf8.dropFirst(7).allSatisfy {
            (48...57).contains($0) || (97...102).contains($0)
        }
    }
}

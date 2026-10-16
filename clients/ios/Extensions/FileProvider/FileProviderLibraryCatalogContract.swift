import Foundation

/// Catalog snapshot containing all server-confirmed Libraries for a domain root.
struct FileProviderLibraryCatalogSnapshot: Codable, Equatable, Sendable {
    static let currentSchemaVersion = 1

    let schemaVersion: Int
    let domainIdentity: String
    let ownerScopeFingerprint: String
    let projectionGeneration: UInt64
    let completeness: FileProviderProjectionCompleteness
    let libraries: [FileProviderLibraryCatalogEntry]

    init(
        schemaVersion: Int = Self.currentSchemaVersion,
        domainIdentity: FileProviderDomainIdentity,
        ownerScopeFingerprint: String,
        projectionGeneration: UInt64,
        completeness: FileProviderProjectionCompleteness,
        libraries: [FileProviderLibraryCatalogEntry]
    ) {
        self.schemaVersion = schemaVersion
        self.domainIdentity = domainIdentity.rawValue
        self.ownerScopeFingerprint = ownerScopeFingerprint
        self.projectionGeneration = projectionGeneration
        self.completeness = completeness
        self.libraries = libraries
    }

    func validated(
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String
    ) throws -> Self {
        guard schemaVersion == Self.currentSchemaVersion else {
            throw FileProviderMetadataContractError.unsupportedSchema
        }
        guard domainIdentity == expectedDomain.rawValue else {
            throw FileProviderMetadataContractError.domainMismatch
        }
        guard ownerScopeFingerprint == expectedOwnerScopeFingerprint,
            ownerScopeFingerprint == expectedDomain.ownerScopeFingerprint
        else { throw FileProviderMetadataContractError.ownerScopeMismatch }
        guard projectionGeneration > 0, completeness == .complete else {
            throw FileProviderMetadataContractError.incompleteOrUnbounded
        }

        var seenIDs: Set<String> = []
        for entry in libraries {
            guard !entry.libraryID.isEmpty, entry.libraryID.utf8.count <= 256,
                !entry.displayName.isEmpty, entry.displayName.utf8.count <= 255
            else { throw FileProviderMetadataContractError.invalidDisplayName }
            guard seenIDs.insert(entry.libraryID).inserted else {
                throw FileProviderMetadataContractError.duplicateIdentity
            }
        }

        return Self(
            schemaVersion: schemaVersion,
            domainIdentity: expectedDomain,
            ownerScopeFingerprint: expectedOwnerScopeFingerprint,
            projectionGeneration: projectionGeneration,
            completeness: completeness,
            libraries: libraries.sorted { $0.libraryID < $1.libraryID }
        )
    }

    func encoded(
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String
    ) throws -> Data {
        let snapshot = try validated(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        encoder.dateEncodingStrategy = .iso8601
        let data = try encoder.encode(snapshot)
        guard data.count <= FileProviderSharedContainer.maximumSnapshotBytes else {
            throw FileProviderMetadataContractError.snapshotTooLarge
        }
        return data
    }

    static func decode(
        _ data: Data,
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String
    ) throws -> Self {
        guard data.count <= FileProviderSharedContainer.maximumSnapshotBytes else {
            throw FileProviderMetadataContractError.snapshotTooLarge
        }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let decoded: Self
        do {
            decoded = try decoder.decode(Self.self, from: data)
        } catch {
            throw FileProviderMetadataContractError.corruptSnapshot
        }
        return try decoded.validated(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint
        )
    }
}

struct FileProviderLibraryCatalogEntry: Codable, Equatable, Sendable {
    let libraryID: String
    let displayName: String
}

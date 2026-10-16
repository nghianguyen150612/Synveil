import Foundation

enum FileProviderProjectionCompleteness: String, Codable, Sendable {
    case complete
    case partial
}

enum FileProviderProjectionSource: String, Codable, Sendable {
    case serverConfirmed
    case unresolvedLocalMutation
    case conflictReview
}

enum FileProviderProjectedItemKind: String, Codable, Sendable {
    case libraryRoot
    case folder
    case file
}

/// Read-only, versioned cross-process projection. Optional fields remain unknown when the server
/// did not provide authoritative values. This contract is not wired to production publication.
struct FileProviderMetadataSnapshot: Codable, Equatable, Sendable {
    static let currentSchemaVersion = 1
    static let maximumItemCount = 10_000

    let schemaVersion: Int
    let domainIdentity: String
    let ownerScopeFingerprint: String
    let libraryID: String
    let projectionGeneration: UInt64
    let completeness: FileProviderProjectionCompleteness
    let items: [FileProviderProjectedItem]

    init(
        schemaVersion: Int = Self.currentSchemaVersion,
        domainIdentity: FileProviderDomainIdentity,
        ownerScopeFingerprint: String,
        libraryID: String,
        projectionGeneration: UInt64,
        completeness: FileProviderProjectionCompleteness,
        items: [FileProviderProjectedItem]
    ) {
        self.schemaVersion = schemaVersion
        self.domainIdentity = domainIdentity.rawValue
        self.ownerScopeFingerprint = ownerScopeFingerprint
        self.libraryID = libraryID
        self.projectionGeneration = projectionGeneration
        self.completeness = completeness
        self.items = items
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
        guard !libraryID.isEmpty, libraryID.utf8.count <= 256,
            projectionGeneration > 0,
            completeness == .complete,
            !items.isEmpty,
            items.count <= Self.maximumItemCount
        else { throw FileProviderMetadataContractError.incompleteOrUnbounded }

        let ordered = items.sorted { $0.identity.rawValue < $1.identity.rawValue }
        let expectedLibraryRoot = try FileProviderItemIdentity.libraryRoot(
            domain: expectedDomain, libraryID: libraryID)
        var byIdentity: [String: FileProviderProjectedItem] = [:]
        for item in ordered {
            guard item.source == .serverConfirmed else {
                throw FileProviderMetadataContractError.nonCanonicalSource
            }
            let parsedIdentity = try FileProviderItemIdentity(
                rawValue: item.identity.rawValue, expectedDomain: expectedDomain,
                expectedLibraryID: libraryID)
            guard parsedIdentity == item.identity,
                parsedIdentity.domainFingerprint == expectedDomain.fingerprint
            else { throw FileProviderMetadataContractError.invalidIdentity }

            switch item.kind {
            case .libraryRoot:
                let expected = try FileProviderItemIdentity.libraryRoot(
                    domain: expectedDomain, libraryID: libraryID)
                guard item.identity == expected,
                    item.parentIdentity == FileProviderItemIdentity.domainRoot(domain: expectedDomain),
                    !item.isTombstone,
                    item.fileVersionIdentity == nil,
                    item.byteLength == nil,
                    item.contentSHA256 == nil
                else { throw FileProviderMetadataContractError.invalidLibraryRoot }
            case .folder, .file:
                guard parsedIdentity.kind == .node,
                    parsedIdentity.libraryFingerprint == expectedLibraryRoot.libraryFingerprint
                else { throw FileProviderMetadataContractError.invalidIdentity }
            }
            try item.validateFields(expectedDomain: expectedDomain, libraryID: libraryID)

            guard byIdentity.updateValue(item, forKey: item.identity.rawValue) == nil else {
                throw FileProviderMetadataContractError.duplicateIdentity
            }
        }

        guard byIdentity[expectedLibraryRoot.rawValue]?.kind == .libraryRoot else {
            throw FileProviderMetadataContractError.missingLibraryRoot
        }
        for item in ordered where item.kind != .libraryRoot {
            guard let parent = item.parentIdentity,
                let parentItem = byIdentity[parent.rawValue],
                parentItem.kind == .libraryRoot || parentItem.kind == .folder,
                !parentItem.isTombstone,
                parent.domainFingerprint == expectedDomain.fingerprint,
                parent.libraryFingerprint == item.identity.libraryFingerprint
            else { throw FileProviderMetadataContractError.invalidParent }
        }

        try Self.validateNoCycles(ordered, root: expectedLibraryRoot)
        return Self(
            schemaVersion: schemaVersion,
            domainIdentity: expectedDomain,
            ownerScopeFingerprint: ownerScopeFingerprint,
            libraryID: libraryID,
            projectionGeneration: projectionGeneration,
            completeness: completeness,
            items: ordered
        )
    }

    func encoded(
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String
    ) throws -> Data {
        let snapshot = try validated(
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint)
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
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint)
    }

    private static func validateNoCycles(
        _ items: [FileProviderProjectedItem], root: FileProviderItemIdentity
    ) throws {
        let parents = Dictionary(uniqueKeysWithValues: items.map { ($0.identity.rawValue, $0.parentIdentity?.rawValue) })
        for item in items where item.kind == .folder || item.kind == .file {
            var current = item.identity.rawValue
            var visited: Set<String> = []
            var depth = 0
            while current != root.rawValue {
                guard visited.insert(current).inserted, let parent = parents[current] ?? nil else {
                    throw FileProviderMetadataContractError.invalidParent
                }
                current = parent
                depth += 1
                guard depth <= 128 else { throw FileProviderMetadataContractError.invalidParent }
            }
        }
    }
}

struct FileProviderProjectedItem: Codable, Equatable, Sendable {
    let identity: FileProviderItemIdentity
    let parentIdentity: FileProviderItemIdentity?
    let kind: FileProviderProjectedItemKind
    let displayName: String
    let byteLength: UInt64?
    let creationDate: Date?
    let contentModificationDate: Date?
    let fileVersionIdentity: FileProviderVersionIdentity?
    let contentSHA256: String?
    let metadataRevision: String?
    let isTombstone: Bool
    let source: FileProviderProjectionSource

    func validateFields(expectedDomain: FileProviderDomainIdentity, libraryID: String) throws {
        guard !displayName.isEmpty, displayName.utf8.count <= 255,
            displayName != ".", displayName != "..",
            !displayName.contains("/"), !displayName.contains("\\"),
            !displayName.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
        else { throw FileProviderMetadataContractError.invalidDisplayName }

        if let metadataRevision {
            guard !metadataRevision.isEmpty, metadataRevision.utf8.count <= 256,
                !metadataRevision.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
            else { throw FileProviderMetadataContractError.invalidRevision }
        }

        switch kind {
        case .libraryRoot:
            break
        case .folder:
            guard fileVersionIdentity == nil, byteLength == nil, contentSHA256 == nil else {
                throw FileProviderMetadataContractError.invalidContentMetadata
            }
        case .file:
            guard (byteLength == nil) == (contentSHA256 == nil) else {
                throw FileProviderMetadataContractError.invalidContentMetadata
            }
            if let digest = contentSHA256 {
                guard digest.range(of: "^sha256:[0-9a-f]{64}$", options: .regularExpression) != nil else {
                    throw FileProviderMetadataContractError.invalidContentMetadata
                }
            }
            if let fileVersionIdentity {
                let nodeID = identity.resolvedIdentifier
                guard identity.kind == .node, let nodeID,
                    fileVersionIdentity == (try FileProviderVersionIdentity(
                        domain: expectedDomain,
                        libraryID: libraryID,
                        nodeID: nodeID,
                        fileVersionID: fileVersionIdentity.fileVersionID
                    ))
                else { throw FileProviderMetadataContractError.invalidContentMetadata }
            }
        }
        if isTombstone, kind == .libraryRoot {
            throw FileProviderMetadataContractError.invalidLibraryRoot
        }
    }
}

enum FileProviderMetadataContractError: Error, Equatable {
    case unsupportedSchema
    case domainMismatch
    case ownerScopeMismatch
    case incompleteOrUnbounded
    case nonCanonicalSource
    case invalidIdentity
    case invalidLibraryRoot
    case duplicateIdentity
    case missingLibraryRoot
    case invalidParent
    case invalidDisplayName
    case invalidRevision
    case invalidContentMetadata
    case corruptSnapshot
    case snapshotTooLarge
}

struct FileProviderMetadataSnapshotStore {
    let container: FileProviderSharedContainer

    func publish(
        _ snapshot: FileProviderMetadataSnapshot,
        expectedDomain: FileProviderDomainIdentity,
        expectedOwnerScopeFingerprint: String,
        capability: FileProviderCapability = .production
    ) throws {
        try container.publishSnapshot(
            snapshot,
            expectedDomain: expectedDomain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint,
            capability: capability
        )
    }

    func read(
        domain: FileProviderDomainIdentity,
        libraryID: String,
        expectedOwnerScopeFingerprint: String
    ) throws -> FileProviderMetadataSnapshot {
        let url = try container.snapshotURL(domain: domain, libraryID: libraryID)
        let data: Data
        do {
            data = try Data(contentsOf: url, options: [.mappedIfSafe])
        } catch {
            throw FileProviderMetadataContractError.corruptSnapshot
        }
        let snapshot = try FileProviderMetadataSnapshot.decode(
            data,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: expectedOwnerScopeFingerprint)
        guard snapshot.libraryID == libraryID else {
            throw FileProviderMetadataContractError.ownerScopeMismatch
        }
        return snapshot
    }
}

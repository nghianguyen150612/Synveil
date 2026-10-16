import Foundation

/// v2 independently bounded, complete direct-child listing with its verified ancestor chain.
/// All folder listings must be present in a domain commit: no lazy/partial tree is authoritative.
/// A Library may span many units, while each unit keeps the P053 10,000-item and 8 MiB limits.
struct FileProviderContainerSnapshot: Codable, Equatable, Sendable {
    let schemaVersion: Int
    let domainIdentity: String
    let ownerScopeFingerprint: String
    let libraryID: String
    let projectionGeneration: UInt64
    let completeness: FileProviderProjectionCompleteness
    let ancestors: [FileProviderProjectedItem]
    let children: [FileProviderProjectedItem]

    var containerIdentity: String { ancestors.last?.identity.rawValue ?? "" }

    func validated(domain: FileProviderDomainIdentity, generation: UInt64) throws -> Self {
        guard schemaVersion == 2 else { throw FileProviderSynchronizationError.incompatible }
        guard domainIdentity == domain.rawValue,
            ownerScopeFingerprint == domain.ownerScopeFingerprint,
            projectionGeneration == generation, generation > 0,
            completeness == .complete, !ancestors.isEmpty, ancestors.count <= 129,
            ancestors.count + children.count <= FileProviderMetadataSnapshot.maximumItemCount
        else { throw FileProviderSynchronizationError.incomplete }
        let root = try FileProviderItemIdentity.libraryRoot(domain: domain, libraryID: libraryID)
        guard ancestors[0].identity == root, ancestors[0].kind == .libraryRoot,
            ancestors[0].parentIdentity == .domainRoot(domain: domain)
        else { throw FileProviderMetadataContractError.invalidLibraryRoot }
        var seen: Set<String> = []
        for (index, item) in ancestors.enumerated() {
            try validateItem(item, domain: domain)
            guard seen.insert(item.identity.rawValue).inserted, !item.isTombstone,
                index == 0
                    || (item.kind == .folder
                        && item.parentIdentity == ancestors[index - 1].identity)
            else { throw FileProviderMetadataContractError.invalidParent }
        }
        for item in children {
            try validateItem(item, domain: domain)
            guard seen.insert(item.identity.rawValue).inserted, item.kind != .libraryRoot,
                item.parentIdentity == ancestors.last?.identity
            else { throw FileProviderMetadataContractError.invalidParent }
        }
        return self
    }

    private func validateItem(_ item: FileProviderProjectedItem, domain: FileProviderDomainIdentity)
        throws
    {
        let parsed = try FileProviderItemIdentity(
            rawValue: item.identity.rawValue,
            expectedDomain: domain, expectedLibraryID: libraryID)
        guard parsed == item.identity, item.source == .serverConfirmed,
            item.kind == .libraryRoot || parsed.kind == .node,
            item.kind != .libraryRoot
                || (item.fileVersionIdentity == nil && item.contentSHA256 == nil
                    && item.byteLength == nil)
        else { throw FileProviderMetadataContractError.nonCanonicalSource }
        try item.validateFields(expectedDomain: domain, libraryID: libraryID)
    }

    func encoded(domain: FileProviderDomainIdentity, generation: UInt64) throws -> Data {
        let value = try validated(domain: domain, generation: generation)
        let data = try FileProviderGenerationCodec.encode(value)
        guard data.count <= FileProviderSharedContainer.maximumSnapshotBytes else {
            throw FileProviderMetadataContractError.snapshotTooLarge
        }
        return data
    }

    static func decode(_ data: Data, domain: FileProviderDomainIdentity, generation: UInt64) throws
        -> Self
    {
        guard data.count <= FileProviderSharedContainer.maximumSnapshotBytes else {
            throw FileProviderMetadataContractError.snapshotTooLarge
        }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let value: Self
        do { value = try decoder.decode(Self.self, from: data) } catch {
            throw FileProviderSynchronizationError.corrupt
        }
        return try value.validated(domain: domain, generation: generation)
    }

    /// Equality of repeated ancestor records prevents cross-unit mixing. Every live folder/root
    /// must have exactly one complete listing, including truly empty folders. Every non-root
    /// ancestor must also occur in its parent's children. This proves reachability, parentage and
    /// absence of cycles without increasing the per-unit validation limit.
    static func assemble(_ units: [Self], domain: FileProviderDomainIdentity, generation: UInt64)
        throws -> [FileProviderProjectedItem]
    {
        var containers: Set<String> = []
        var items: [String: FileProviderProjectedItem] = [:]
        var listed: Set<String> = []
        for unit in units {
            _ = try unit.validated(domain: domain, generation: generation)
            guard containers.insert(unit.containerIdentity).inserted else {
                throw FileProviderMetadataContractError.duplicateIdentity
            }
            for item in unit.ancestors + unit.children {
                if let previous = items[item.identity.rawValue], previous != item {
                    throw FileProviderSynchronizationError.corrupt
                }
                items[item.identity.rawValue] = item
            }
            for child in unit.children {
                guard listed.insert(child.identity.rawValue).inserted else {
                    throw FileProviderMetadataContractError.duplicateIdentity
                }
            }
        }
        for item in items.values {
            if item.kind != .libraryRoot, !listed.contains(item.identity.rawValue) {
                throw FileProviderSynchronizationError.incomplete
            }
            if item.kind != .file, !item.isTombstone, !containers.contains(item.identity.rawValue) {
                throw FileProviderSynchronizationError.incomplete
            }
        }
        return items.values.sorted { $0.identity.rawValue < $1.identity.rawValue }
    }
}

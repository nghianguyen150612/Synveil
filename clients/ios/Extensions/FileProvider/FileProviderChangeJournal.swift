import FileProvider
import Foundation

/// Opaque, versioned, domain-bound sync anchor token.
struct FileProviderSyncAnchorToken: Codable, Equatable, Sendable {
    static let currentVersion = 1

    let version: Int
    let domainFingerprint: String
    let libraryID: String?
    let generation: UInt64
    let timestamp: Date

    init(
        domain: FileProviderDomainIdentity,
        libraryID: String?,
        generation: UInt64,
        timestamp: Date = Date()
    ) {
        self.version = Self.currentVersion
        self.domainFingerprint = domain.fingerprint
        self.libraryID = libraryID
        self.generation = generation
        self.timestamp = timestamp
    }

    func toNSFileProviderSyncAnchor() throws -> NSFileProviderSyncAnchor {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        let data = try encoder.encode(self)
        return NSFileProviderSyncAnchor(data)
    }

    static func fromNSFileProviderSyncAnchor(
        _ anchor: NSFileProviderSyncAnchor,
        expectedDomain: FileProviderDomainIdentity,
        expectedLibraryID: String?
    ) throws -> FileProviderSyncAnchorToken {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let token: FileProviderSyncAnchorToken
        do {
            token = try decoder.decode(FileProviderSyncAnchorToken.self, from: anchor.rawValue)
        } catch {
            throw FileProviderErrorMapper.error(for: .unsupported)
        }
        guard token.version == Self.currentVersion,
            token.domainFingerprint == expectedDomain.fingerprint,
            token.libraryID == expectedLibraryID
        else {
            throw NSError(
                domain: NSFileProviderErrorDomain,
                code: NSFileProviderError.syncAnchorExpired.rawValue
            )
        }
        return token
    }
}

/// Represents a change item in the change journal.
struct FileProviderChangeItem: Codable, Equatable, Sendable {
    enum ChangeType: String, Codable, Sendable {
        case addedOrUpdated
        case deleted
    }

    let changeType: ChangeType
    let item: FileProviderProjectedItem
    let generation: UInt64
}

/// Change journal stored in App Group storage for tracking changes across snapshot generations.
struct FileProviderChangeJournal: Codable, Equatable, Sendable {
    static let maxJournalEntries = 1_000

    let domainFingerprint: String
    let libraryID: String
    let entries: [FileProviderChangeItem]

    init(
        domain: FileProviderDomainIdentity,
        libraryID: String,
        entries: [FileProviderChangeItem] = []
    ) {
        self.domainFingerprint = domain.fingerprint
        self.libraryID = libraryID
        self.entries = entries
    }

    /// Computes diffs between two snapshot generations.
    static func diff(
        previous: FileProviderMetadataSnapshot?,
        current: FileProviderMetadataSnapshot
    ) -> [FileProviderChangeItem] {
        var changes: [FileProviderChangeItem] = []
        let currentMap = Dictionary(
            uniqueKeysWithValues: current.items.map { ($0.identity.rawValue, $0) }
        )
        let prevMap = Dictionary(
            uniqueKeysWithValues: (previous?.items ?? []).map { ($0.identity.rawValue, $0) }
        )

        for (id, currentItem) in currentMap {
            if currentItem.isTombstone {
                changes.append(
                    FileProviderChangeItem(
                        changeType: .deleted,
                        item: currentItem,
                        generation: current.projectionGeneration
                    )
                )
            } else if let prevItem = prevMap[id] {
                if prevItem != currentItem {
                    changes.append(
                        FileProviderChangeItem(
                            changeType: .addedOrUpdated,
                            item: currentItem,
                            generation: current.projectionGeneration
                        )
                    )
                }
            } else {
                changes.append(
                    FileProviderChangeItem(
                        changeType: .addedOrUpdated,
                        item: currentItem,
                        generation: current.projectionGeneration
                    )
                )
            }
        }

        for (id, prevItem) in prevMap where currentMap[id] == nil && !prevItem.isTombstone {
            changes.append(
                FileProviderChangeItem(
                    changeType: .deleted,
                    item: prevItem,
                    generation: current.projectionGeneration
                )
            )
        }

        return changes
    }
}

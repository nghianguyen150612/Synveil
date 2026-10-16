import Foundation

/// Host application component responsible for publishing server-confirmed metadata snapshots
/// and recording durable change journal entries into App Group storage for the File Provider extension.
struct FileProviderMetadataPublicationCoordinator {
    let container: FileProviderSharedContainer

    init(container: FileProviderSharedContainer = FileProviderSharedContainer()) {
        self.container = container
    }

    /// Publishes domain identity and library catalog for the domain root.
    func publishLibraryCatalog(
        catalog: FileProviderLibraryCatalogSnapshot,
        domain: FileProviderDomainIdentity,
        ownerScopeFingerprint: String,
        capability: FileProviderCapability = .production
    ) throws {
        try container.publishDomainIdentity(domain, capability: capability)
        try container.publishLibraryCatalog(
            catalog,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: ownerScopeFingerprint,
            capability: capability
        )
    }

    /// Publishes a complete server-confirmed item snapshot for a specific library, updating the
    /// change journal.
    func publishSnapshot(
        snapshot: FileProviderMetadataSnapshot,
        domain: FileProviderDomainIdentity,
        ownerScopeFingerprint: String,
        capability: FileProviderCapability = .production
    ) throws {
        try container.publishDomainIdentity(domain, capability: capability)

        let previousSnapshot = try? FileProviderMetadataSnapshotStore(container: container)
            .read(
                domain: domain,
                libraryID: snapshot.libraryID,
                expectedOwnerScopeFingerprint: ownerScopeFingerprint
            )

        let newChanges = FileProviderChangeJournal.diff(
            previous: previousSnapshot, current: snapshot)

        let existingJournal =
            (try? container.readChangeJournal(domain: domain, libraryID: snapshot.libraryID))
            ?? FileProviderChangeJournal(domain: domain, libraryID: snapshot.libraryID)

        var combinedEntries = existingJournal.entries + newChanges
        if combinedEntries.count > FileProviderChangeJournal.maxJournalEntries {
            combinedEntries = Array(
                combinedEntries.suffix(FileProviderChangeJournal.maxJournalEntries))
        }

        let updatedJournal = FileProviderChangeJournal(
            domain: domain,
            libraryID: snapshot.libraryID,
            entries: combinedEntries
        )

        try container.publishChangeJournal(
            updatedJournal,
            domain: domain,
            libraryID: snapshot.libraryID,
            capability: capability
        )

        try container.publishSnapshot(
            snapshot,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: ownerScopeFingerprint,
            capability: capability
        )
    }
}

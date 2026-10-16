import FileProvider
import Foundation

/// Real NSFileProviderEnumerator implementation for domain root, library root, and folder containers.
final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let containerItemIdentifier: NSFileProviderItemIdentifier
    private let boundary: FileProviderAuthorizationBoundary
    private let container: FileProviderSharedContainer

    init(
        containerItemIdentifier: NSFileProviderItemIdentifier,
        boundary: FileProviderAuthorizationBoundary,
        container: FileProviderSharedContainer = FileProviderSharedContainer()
    ) {
        self.containerItemIdentifier = containerItemIdentifier
        self.boundary = boundary
        self.container = container
        super.init()
    }

    func invalidate() {}

    func enumerateItems(
        for observer: any NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        do {
            let auth = try boundary.validateAuthorization()
            let domain = boundary.domain

            if containerItemIdentifier == .rootContainer {
                try enumerateDomainRoot(
                    observer: observer,
                    page: page,
                    domain: domain,
                    auth: auth
                )
            } else {
                try enumerateContainerItem(
                    observer: observer,
                    page: page,
                    domain: domain,
                    auth: auth
                )
            }
        } catch {
            observer.finishEnumeratingWithError(error)
        }
    }

    func enumerateChanges(
        for observer: any NSFileProviderChangeObserver,
        from anchor: NSFileProviderSyncAnchor
    ) {
        do {
            let auth = try boundary.validateAuthorization()
            let domain = boundary.domain

            let expectedLibraryID = try resolveContainerLibraryID(domain: domain, auth: auth)

            let token = try FileProviderSyncAnchorToken.fromNSFileProviderSyncAnchor(
                anchor,
                expectedDomain: domain,
                expectedLibraryID: expectedLibraryID
            )

            var latestGeneration = token.generation
            var pendingUpdates: [NSFileProviderItem] = []
            var pendingDeletions: [NSFileProviderItemIdentifier] = []

            if let libID = expectedLibraryID {
                if let journal = try? container.readChangeJournal(
                    domain: domain,
                    libraryID: libID
                ) {
                    let generations = journal.entries.map { $0.generation }
                    if let minGen = generations.min(), token.generation < minGen - 1 {
                        throw NSError(
                            domain: NSFileProviderErrorDomain,
                            code: NSFileProviderError.syncAnchorExpired.rawValue
                        )
                    }
                    for entry in journal.entries where entry.generation > token.generation {
                        if entry.generation > latestGeneration {
                            latestGeneration = entry.generation
                        }
                        if entry.changeType == .deleted || entry.item.isTombstone {
                            pendingDeletions.append(
                                NSFileProviderItemIdentifier(entry.item.identity.rawValue)
                            )
                        } else {
                            if let item = try? SynveilFileProviderItem(
                                projectedItem: entry.item
                            ) {
                                pendingUpdates.append(item)
                            }
                        }
                    }
                }
            }

            if !pendingUpdates.isEmpty {
                observer.didUpdate(pendingUpdates)
            }
            if !pendingDeletions.isEmpty {
                observer.didDeleteItems(withIdentifiers: pendingDeletions)
            }

            let nextAnchor = try FileProviderSyncAnchorToken(
                domain: domain,
                libraryID: expectedLibraryID,
                generation: latestGeneration
            ).toNSFileProviderSyncAnchor()

            observer.finishEnumeratingChanges(upTo: nextAnchor, moreComing: false)
        } catch {
            observer.finishEnumeratingWithError(error)
        }
    }

    func currentSyncAnchor(completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void) {
        do {
            let auth = try boundary.validateAuthorization()
            let domain = boundary.domain

            let expectedLibraryID = try resolveContainerLibraryID(domain: domain, auth: auth)

            var currentGeneration: UInt64 = 1
            if containerItemIdentifier == .rootContainer {
                let catalogURL = try container.libraryCatalogURL(domain: domain)
                if let data = try? Data(contentsOf: catalogURL),
                    let catalog = try? FileProviderLibraryCatalogSnapshot.decode(
                        data,
                        expectedDomain: domain,
                        expectedOwnerScopeFingerprint: auth.ownerScopeFingerprint
                    )
                {
                    currentGeneration = catalog.projectionGeneration
                }
            } else if let libID = expectedLibraryID {
                let snapshotURL = try container.snapshotURL(domain: domain, libraryID: libID)
                if let data = try? Data(contentsOf: snapshotURL),
                    let snapshot = try? FileProviderMetadataSnapshot.decode(
                        data,
                        expectedDomain: domain,
                        expectedOwnerScopeFingerprint: auth.ownerScopeFingerprint
                    )
                {
                    currentGeneration = snapshot.projectionGeneration
                }
            }

            let token = try FileProviderSyncAnchorToken(
                domain: domain,
                libraryID: expectedLibraryID,
                generation: currentGeneration
            ).toNSFileProviderSyncAnchor()
            completionHandler(token)
        } catch {
            completionHandler(nil)
        }
    }

    // MARK: - Private Helpers

    private func resolveContainerLibraryID(
        domain: FileProviderDomainIdentity,
        auth: FileProviderAuthorizationEvidence
    ) throws -> String? {
        guard containerItemIdentifier != .rootContainer && containerItemIdentifier != .workingSet
        else {
            return nil
        }
        let parsedIdentity = try FileProviderItemIdentity(
            rawValue: containerItemIdentifier.rawValue,
            expectedDomain: domain
        )
        return try resolveLibraryID(parsedIdentity: parsedIdentity, domain: domain, auth: auth)
    }

    private func enumerateDomainRoot(
        observer: any NSFileProviderEnumerationObserver,
        page: NSFileProviderPage,
        domain: FileProviderDomainIdentity,
        auth: FileProviderAuthorizationEvidence
    ) throws {
        let catalogURL = try container.libraryCatalogURL(domain: domain)
        guard let data = try? Data(contentsOf: catalogURL) else {
            observer.finishEnumeratingWithError(FileProviderErrorMapper.unavailable())
            return
        }

        let catalog = try FileProviderLibraryCatalogSnapshot.decode(
            data,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: auth.ownerScopeFingerprint
        )

        let items: [NSFileProviderItem] = try catalog.libraries.map { entry in
            let libIdentity = try FileProviderItemIdentity.libraryRoot(
                domain: domain,
                libraryID: entry.libraryID
            )
            let projected = FileProviderProjectedItem(
                identity: libIdentity,
                parentIdentity: FileProviderItemIdentity.domainRoot(domain: domain),
                kind: .libraryRoot,
                displayName: entry.displayName,
                byteLength: nil,
                creationDate: nil,
                contentModificationDate: nil,
                fileVersionIdentity: nil,
                contentSHA256: nil,
                metadataRevision: nil,
                isTombstone: false,
                source: .serverConfirmed
            )
            return try SynveilFileProviderItem(projectedItem: projected)
        }

        let offset: Int
        let pageSize = FileProviderPageToken.defaultPageSize
        if page.rawValue.isEmpty {
            offset = 0
        } else {
            let pageToken = try FileProviderPageToken.fromNSFileProviderPage(
                page,
                expectedDomain: domain,
                expectedContainerIdentifier: containerItemIdentifier.rawValue,
                currentGeneration: catalog.projectionGeneration
            )
            offset = pageToken.offset
        }

        guard offset <= items.count else {
            throw NSError(
                domain: NSFileProviderErrorDomain,
                code: NSFileProviderError.pageExpired.rawValue
            )
        }

        let slice = Array(items.dropFirst(offset).prefix(pageSize))
        let nextOffset = offset + slice.count
        let hasMore = nextOffset < items.count

        observer.didEnumerate(slice)

        if hasMore {
            let nextPageToken = FileProviderPageToken(
                domain: domain,
                containerIdentifier: containerItemIdentifier.rawValue,
                projectionGeneration: catalog.projectionGeneration,
                offset: nextOffset,
                limit: pageSize
            )
            observer.finishEnumerating(upTo: try nextPageToken.toNSFileProviderPage())
        } else {
            observer.finishEnumerating(upTo: nil)
        }
    }

    private func enumerateContainerItem(
        observer: any NSFileProviderEnumerationObserver,
        page: NSFileProviderPage,
        domain: FileProviderDomainIdentity,
        auth: FileProviderAuthorizationEvidence
    ) throws {
        let parsedIdentity = try FileProviderItemIdentity(
            rawValue: containerItemIdentifier.rawValue,
            expectedDomain: domain
        )
        let libraryID = try resolveLibraryID(
            parsedIdentity: parsedIdentity,
            domain: domain,
            auth: auth
        )

        let snapshotURL = try container.snapshotURL(domain: domain, libraryID: libraryID)
        guard let data = try? Data(contentsOf: snapshotURL) else {
            observer.finishEnumeratingWithError(FileProviderErrorMapper.unavailable())
            return
        }

        let snapshot = try FileProviderMetadataSnapshot.decode(
            data,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: auth.ownerScopeFingerprint
        )

        let matchingItems = snapshot.items.filter { item in
            !item.isTombstone && item.parentIdentity?.rawValue == containerItemIdentifier.rawValue
        }

        let nativeItems: [NSFileProviderItem] = try matchingItems.map {
            try SynveilFileProviderItem(projectedItem: $0)
        }

        let offset: Int
        let pageSize = FileProviderPageToken.defaultPageSize
        if page.rawValue.isEmpty {
            offset = 0
        } else {
            let pageToken = try FileProviderPageToken.fromNSFileProviderPage(
                page,
                expectedDomain: domain,
                expectedContainerIdentifier: containerItemIdentifier.rawValue,
                currentGeneration: snapshot.projectionGeneration
            )
            offset = pageToken.offset
        }

        guard offset <= nativeItems.count else {
            throw NSError(
                domain: NSFileProviderErrorDomain,
                code: NSFileProviderError.pageExpired.rawValue
            )
        }

        let slice = Array(nativeItems.dropFirst(offset).prefix(pageSize))
        let nextOffset = offset + slice.count
        let hasMore = nextOffset < nativeItems.count

        observer.didEnumerate(slice)

        if hasMore {
            let nextPageToken = FileProviderPageToken(
                domain: domain,
                containerIdentifier: containerItemIdentifier.rawValue,
                projectionGeneration: snapshot.projectionGeneration,
                offset: nextOffset,
                limit: pageSize
            )
            observer.finishEnumerating(upTo: try nextPageToken.toNSFileProviderPage())
        } else {
            observer.finishEnumerating(upTo: nil)
        }
    }

    private func resolveLibraryID(
        parsedIdentity: FileProviderItemIdentity,
        domain: FileProviderDomainIdentity,
        auth: FileProviderAuthorizationEvidence
    ) throws -> String {
        if parsedIdentity.kind == .libraryRoot, let libID = parsedIdentity.resolvedIdentifier {
            return libID
        }

        guard let libraryScope = parsedIdentity.libraryFingerprint else {
            throw FileProviderErrorMapper.unavailable()
        }

        let catalogURL = try container.libraryCatalogURL(domain: domain)
        guard let data = try? Data(contentsOf: catalogURL),
            let catalog = try? FileProviderLibraryCatalogSnapshot.decode(
                data,
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: auth.ownerScopeFingerprint
            )
        else {
            throw FileProviderErrorMapper.unavailable()
        }

        for entry in catalog.libraries {
            let scope = FileProviderDomainIdentity.fingerprint([
                "synveil-library-v1", entry.libraryID,
            ])
            if scope == libraryScope {
                return entry.libraryID
            }
        }

        throw FileProviderErrorMapper.unavailable()
    }
}

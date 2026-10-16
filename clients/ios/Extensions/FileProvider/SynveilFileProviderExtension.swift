import FileProvider
import Foundation

typealias FileProviderItemMutationCompletion = (
    NSFileProviderItem?, NSFileProviderItemFields, Bool, (any Error)?
) -> Void

/// P054 framework entry point providing read-only metadata enumeration and change tracking.
/// It owns no app session, Keychain store, SQLite database, SwiftUI state, URLSession,
/// transfer coordinator, or File Provider domain-registration behavior.
final class SynveilFileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    private let lifecycle = FileProviderProcessLifecycle()
    private let domain: NSFileProviderDomain
    private let container = FileProviderSharedContainer()

    required init(domain: NSFileProviderDomain) {
        self.domain = domain
        super.init()
        lifecycle.launch()
    }

    func invalidate() {
        lifecycle.invalidate()
    }

    func enumerator(
        for containerItemIdentifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest
    ) throws -> any NSFileProviderEnumerator {
        guard let domainIdentity = parseDomainIdentity() else {
            throw FileProviderErrorMapper.unavailable()
        }
        let boundary = FileProviderAuthorizationBoundary(
            domain: domainIdentity, container: container)
        _ = try boundary.validateAuthorization()
        return FileProviderEnumerator(
            containerItemIdentifier: containerItemIdentifier,
            boundary: boundary,
            container: container
        )
    }

    func item(
        for identifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, (any Error)?) -> Void
    ) -> Progress {
        do {
            guard let domainIdentity = parseDomainIdentity() else {
                throw FileProviderErrorMapper.unavailable()
            }
            let boundary = FileProviderAuthorizationBoundary(
                domain: domainIdentity, container: container)
            let auth = try boundary.validateAuthorization()

            if identifier == .rootContainer {
                let rootIdentity = FileProviderItemIdentity.domainRoot(domain: domainIdentity)
                let projected = FileProviderProjectedItem(
                    identity: rootIdentity,
                    parentIdentity: nil,
                    kind: .libraryRoot,
                    displayName: "Synveil Root",
                    byteLength: nil,
                    creationDate: nil,
                    contentModificationDate: nil,
                    fileVersionIdentity: nil,
                    contentSHA256: nil,
                    metadataRevision: nil,
                    isTombstone: false,
                    source: .serverConfirmed
                )
                let item = try SynveilFileProviderItem(projectedItem: projected)
                completionHandler(item, nil)
            } else {
                let parsedIdentity = try FileProviderItemIdentity(
                    rawValue: identifier.rawValue,
                    expectedDomain: domainIdentity
                )

                let libraryID = try resolveLibraryID(
                    parsedIdentity: parsedIdentity, domain: domainIdentity, auth: auth)

                let snapshotURL = try container.snapshotURL(
                    domain: domainIdentity, libraryID: libraryID)
                guard let data = try? Data(contentsOf: snapshotURL) else {
                    throw FileProviderErrorMapper.unavailable()
                }

                let snapshot = try FileProviderMetadataSnapshot.decode(
                    data,
                    expectedDomain: domainIdentity,
                    expectedOwnerScopeFingerprint: auth.ownerScopeFingerprint
                )

                guard
                    let matchingItem = snapshot.items.first(where: {
                        $0.identity.rawValue == identifier.rawValue && !$0.isTombstone
                    })
                else {
                    throw NSError(
                        domain: NSFileProviderErrorDomain,
                        code: NSFileProviderError.noSuchItem.rawValue
                    )
                }

                let item = try SynveilFileProviderItem(projectedItem: matchingItem)
                completionHandler(item, nil)
            }
        } catch {
            completionHandler(nil, error)
        }
        return Self.completedProgress()
    }

    func fetchContents(
        for itemIdentifier: NSFileProviderItemIdentifier,
        version requestedVersion: NSFileProviderItemVersion?,
        request: NSFileProviderRequest,
        completionHandler: @escaping (URL?, NSFileProviderItem?, (any Error)?) -> Void
    ) -> Progress {
        completionHandler(nil, nil, FileProviderErrorMapper.unavailable())
        return Self.completedProgress()
    }

    func createItem(
        basedOn itemTemplate: NSFileProviderItem,
        fields: NSFileProviderItemFields,
        contents url: URL?,
        options: NSFileProviderCreateItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping FileProviderItemMutationCompletion
    ) -> Progress {
        completionHandler(nil, [], false, FileProviderErrorMapper.unsupported())
        return Self.completedProgress()
    }

    func modifyItem(
        _ item: NSFileProviderItem,
        baseVersion version: NSFileProviderItemVersion,
        changedFields: NSFileProviderItemFields,
        contents newContents: URL?,
        options: NSFileProviderModifyItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping FileProviderItemMutationCompletion
    ) -> Progress {
        completionHandler(nil, [], false, FileProviderErrorMapper.unsupported())
        return Self.completedProgress()
    }

    func deleteItem(
        identifier: NSFileProviderItemIdentifier,
        baseVersion version: NSFileProviderItemVersion,
        options: NSFileProviderDeleteItemOptions = [],
        request: NSFileProviderRequest,
        completionHandler: @escaping ((any Error)?) -> Void
    ) -> Progress {
        completionHandler(FileProviderErrorMapper.unsupported())
        return Self.completedProgress()
    }

    private func parseDomainIdentity() -> FileProviderDomainIdentity? {
        try? container.readDomainIdentity(domainIdentifier: domain.identifier.rawValue)
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
                data, expectedDomain: domain,
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

    private static func completedProgress() -> Progress {
        let progress = Progress(totalUnitCount: 1)
        progress.completedUnitCount = 1
        progress.isCancellable = false
        return progress
    }
}

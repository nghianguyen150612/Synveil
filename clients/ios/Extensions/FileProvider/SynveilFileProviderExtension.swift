import FileProvider
import Foundation

typealias FileProviderItemMutationCompletion = (
    NSFileProviderItem?, NSFileProviderItemFields, Bool, (any Error)?
) -> Void

/// Read-only metadata framework entry point. It owns no app session, Keychain store, SQLite
/// database, SwiftUI state, URLSession, transfer coordinator, or File Provider domain-registration
/// behavior.
final class SynveilFileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    private let lifecycle = FileProviderProcessLifecycle()
    private let domainID: String
    private let store = FileProviderGenerationStore(
        container: FileProviderSharedContainer(), capability: .production)

    private var reader: FileProviderMetadataReader {
        FileProviderMetadataReader(store: store, domainID: domainID)
    }

    required init(domain: NSFileProviderDomain) {
        domainID = domain.identifier.rawValue
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
        guard lifecycle.canContinueRequest() else { throw FileProviderErrorMapper.unavailable() }
        let container = FileProviderNativeIdentifiers.scope(containerItemIdentifier)
        do {
            try reader.withView { try $0.validateContainer(container) }
            return SynveilFileProviderEnumerator(
                reader: reader, container: container, process: lifecycle)
        } catch { throw FileProviderErrorMapper.map(error) }
    }

    func item(
        for identifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, (any Error)?) -> Void
    ) -> Progress {
        do {
            guard lifecycle.canContinueRequest() else {
                throw FileProviderSynchronizationError.cancelled
            }
            try reader.withView { view in
                if identifier == .rootContainer {
                    _ = try view.currentAnchor(container: FileProviderMetadataReader.root)
                }
                let item = identifier == .rootContainer ? nil : try view.lookup(identifier.rawValue)
                guard lifecycle.canContinueRequest() else {
                    throw FileProviderSynchronizationError.cancelled
                }
                try view.validateDelivery()
                completionHandler(
                    SynveilFileProviderItem(
                        projected: item,
                        domain: view.manifest.domain,
                        contentStore: FileProviderContentStore(container: store.container),
                        metadataStore: store,
                        authorizedGeneration: view.manifest.authorization.authorizationGeneration
                    ), nil)
            }
        } catch { completionHandler(nil, FileProviderErrorMapper.map(error)) }
        return Self.completedProgress()
    }

    func fetchContents(
        for itemIdentifier: NSFileProviderItemIdentifier,
        version requestedVersion: NSFileProviderItemVersion?,
        request: NSFileProviderRequest,
        completionHandler: @escaping (URL?, NSFileProviderItem?, (any Error)?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 100)

        Task {
            var temporaryCopyURL: URL? = nil
            do {
                guard lifecycle.canContinueRequest(), !progress.isCancelled else {
                    throw FileProviderSynchronizationError.cancelled
                }

                try reader.withView { view in
                    guard lifecycle.canContinueRequest(), !progress.isCancelled else {
                        throw FileProviderSynchronizationError.cancelled
                    }

                    guard let item = try? view.lookup(itemIdentifier.rawValue),
                        item.kind == .file,
                        !item.isTombstone,
                        let fileVersionIdentity = item.fileVersionIdentity
                    else {
                        throw FileProviderSynchronizationError.noSuchItem
                    }

                    let versionContract = try FileProviderItemVersionContract(
                        fileVersionIdentity: item.fileVersionIdentity,
                        metadataRevision: item.metadataRevision
                    )

                    if let requestedVersion = requestedVersion {
                        let requestedContent = requestedVersion.contentVersion
                        let expectedContent = versionContract.contentVersion ?? Data()
                        if !requestedContent.isEmpty && requestedContent != expectedContent {
                            throw FileProviderSynchronizationError.noSuchItem
                        }
                    }

                    progress.completedUnitCount = 20

                    let contentStore = FileProviderContentStore(container: store.container)
                    let authGen = view.manifest.authorization.authorizationGeneration
                    let ownerFingerprint = view.manifest.domain.ownerScopeFingerprint
                    let (_, contentURL) = try contentStore.verifyAndAccessContent(
                        domain: view.manifest.domain,
                        fileVersionID: fileVersionIdentity.fileVersionID,
                        expectedOwnerScopeFingerprint: ownerFingerprint,
                        expectedAuthorizationGeneration: authGen,
                        checkCancellation: {
                            if !lifecycle.canContinueRequest() || progress.isCancelled {
                                throw FileProviderSynchronizationError.cancelled
                            }
                        }
                    )

                    progress.completedUnitCount = 60

                    let tempDir: URL
                    let domainIdentifier = NSFileProviderDomainIdentifier(rawValue: domainID)
                    let domainObj = NSFileProviderDomain(
                        identifier: domainIdentifier,
                        displayName: "Synveil"
                    )
                    if let manager = NSFileProviderManager(for: domainObj),
                        let url = try? manager.temporaryDirectoryURL()
                    {
                        tempDir = url
                    } else {
                        tempDir = FileManager.default.temporaryDirectory
                    }

                    let destURL = tempDir.appendingPathComponent("\(UUID().uuidString).tmp")
                    temporaryCopyURL = destURL

                    if FileManager.default.fileExists(atPath: destURL.path) {
                        try? FileManager.default.removeItem(at: destURL)
                    }

                    try FileManager.default.copyItem(at: contentURL, to: destURL)
                    let attributes: [FileAttributeKey: Any] = [
                        .protectionKey: FileProtectionType.completeUntilFirstUserAuthentication,
                        .posixPermissions: 0o600,
                    ]
                    try FileManager.default.setAttributes(
                        attributes,
                        ofItemAtPath: destURL.path
                    )

                    guard lifecycle.canContinueRequest(), !progress.isCancelled else {
                        throw FileProviderSynchronizationError.cancelled
                    }

                    progress.completedUnitCount = 100

                    let returnedItem = SynveilFileProviderItem(
                        projected: item,
                        domain: view.manifest.domain,
                        contentStore: contentStore,
                        metadataStore: store,
                        authorizedGeneration: authGen
                    )
                    completionHandler(destURL, returnedItem, nil)
                }
            } catch {
                if let temporaryCopyURL = temporaryCopyURL {
                    try? FileManager.default.removeItem(at: temporaryCopyURL)
                }
                completionHandler(nil, nil, FileProviderErrorMapper.map(error))
            }
        }

        progress.cancellationHandler = {
            // Cancellation will be checked via progress.isCancelled
        }

        return progress
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

    private static func completedProgress() -> Progress {
        let progress = Progress(totalUnitCount: 1)
        progress.completedUnitCount = 1
        progress.isCancellable = false
        return progress
    }
}

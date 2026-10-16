import FileProvider
import Foundation

typealias FileProviderItemMutationCompletion = (
    NSFileProviderItem?, NSFileProviderItemFields, Bool, (any Error)?
) -> Void

/// Read-only metadata framework entry point. It owns no app session, Keychain store, SQLite database, SwiftUI
/// state, URLSession, transfer coordinator, or File Provider domain-registration behavior.
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
                let item = identifier == .rootContainer ? nil : try view.lookup(identifier.rawValue)
                guard lifecycle.canContinueRequest() else {
                    throw FileProviderSynchronizationError.cancelled
                }
                completionHandler(
                    SynveilFileProviderItem(projected: item, domain: view.manifest.domain), nil)
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

    private static func completedProgress() -> Progress {
        let progress = Progress(totalUnitCount: 1)
        progress.completedUnitCount = 1
        progress.isCancellable = false
        return progress
    }
}

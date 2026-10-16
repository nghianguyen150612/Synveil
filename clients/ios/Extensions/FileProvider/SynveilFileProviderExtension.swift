import FileProvider
import Foundation

/// P053 framework entry point. It owns no app session, Keychain store, SQLite database, SwiftUI
/// state, URLSession, transfer coordinator, or File Provider domain-registration behavior.
final class SynveilFileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    private let lifecycle = FileProviderProcessLifecycle()

    required init(domain: NSFileProviderDomain) {
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
        throw FileProviderErrorMapper.unavailable()
    }

    func item(
        for identifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, (any Error)?) -> Void
    ) -> Progress {
        completionHandler(nil, FileProviderErrorMapper.unavailable())
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
        completionHandler: @escaping (
            NSFileProviderItem?, NSFileProviderItemFields, Bool, (any Error)?
        ) -> Void
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
        completionHandler: @escaping (
            NSFileProviderItem?, NSFileProviderItemFields, Bool, (any Error)?
        ) -> Void
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

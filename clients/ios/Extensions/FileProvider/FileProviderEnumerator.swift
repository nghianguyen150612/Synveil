import FileProvider
import Foundation

/// A per-container independent lifecycle. A lease covers validation AND observer delivery, so a
/// successful host revocation fences callbacks, rather than merely deleting files behind readers.
final class SynveilFileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    let reader: FileProviderMetadataReader
    let container: String
    let process: FileProviderProcessLifecycle
    private let lifecycle = FileProviderProcessLifecycle()

    init(
        reader: FileProviderMetadataReader, container: String, process: FileProviderProcessLifecycle
    ) {
        self.reader = reader
        self.container = container
        self.process = process
        super.init()
        lifecycle.launch()
    }

    func invalidate() { lifecycle.invalidate() }

    private func checkCancellation() throws {
        guard lifecycle.canContinueRequest(), process.canContinueRequest() else {
            throw FileProviderSynchronizationError.cancelled
        }
    }

    func enumerateItems(
        for observer: any NSFileProviderEnumerationObserver, startingAt page: NSFileProviderPage
    ) {
        do {
            try checkCancellation()
            try reader.withView { view in
                let initial =
                    page.rawValue == (NSFileProviderPage.initialPageSortedByName as Data)
                    || page.rawValue == (NSFileProviderPage.initialPageSortedByDate as Data)
                let batch = try view.page(
                    container: container, token: initial ? nil : page.rawValue,
                    ordering: page.rawValue == (NSFileProviderPage.initialPageSortedByDate as Data)
                        ? "date" : "name")
                let items = batch.items.map {
                    SynveilFileProviderItem(projected: $0, domain: view.manifest.domain)
                }
                try checkCancellation()
                observer.didEnumerate(items)
                try checkCancellation()
                observer.finishEnumerating(upTo: batch.next.map { NSFileProviderPage($0) })
            }
        } catch { observer.finishEnumeratingWithError(FileProviderErrorMapper.map(error)) }
    }

    func enumerateChanges(
        for observer: any NSFileProviderChangeObserver, from anchor: NSFileProviderSyncAnchor
    ) {
        do {
            try checkCancellation()
            try reader.withView { view in
                let batch = try view.changes(container: container, anchor: anchor.rawValue)
                let items = batch.updated.map {
                    SynveilFileProviderItem(projected: $0, domain: view.manifest.domain)
                }
                try checkCancellation()
                observer.didUpdate(items)
                try checkCancellation()
                observer.didDeleteItems(
                    withIdentifiers: batch.deleted.map { NSFileProviderItemIdentifier($0) })
                try checkCancellation()
                observer.finishEnumeratingChanges(
                    upTo: NSFileProviderSyncAnchor(batch.anchor), moreComing: batch.moreComing)
            }
        } catch { observer.finishEnumeratingWithError(FileProviderErrorMapper.map(error)) }
    }

    func currentSyncAnchor(completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void) {
        do {
            try checkCancellation()
            try reader.withView { view in
                let anchor = try view.currentAnchor(container: container)
                try checkCancellation()
                completionHandler(NSFileProviderSyncAnchor(anchor))
            }
        } catch { completionHandler(nil) }
    }
}

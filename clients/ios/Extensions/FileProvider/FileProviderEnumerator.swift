import FileProvider
import Foundation

/// A defensive enumerator for future adapters. P053 never returns it from the extension because
/// no authorized, complete metadata snapshot is published yet.
final class FileProviderUnavailableEnumerator: NSObject, NSFileProviderEnumerator {
    func enumerateItems(
        for observer: any NSFileProviderEnumerationObserver,
        startingAt page: NSFileProviderPage
    ) {
        observer.finishEnumeratingWithError(FileProviderErrorMapper.unavailable())
    }

    func invalidate() {}
}

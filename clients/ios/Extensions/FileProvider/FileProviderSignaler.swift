import FileProvider
import Foundation

/// Signaler wrapping `NSFileProviderManager` for coalesced change signaling without auto-registering
/// production domains.
struct FileProviderSignaler: Sendable {
    func signalEnumerator(
        for containerItemIdentifier: NSFileProviderItemIdentifier,
        domain: NSFileProviderDomain?
    ) {
        guard let domain else { return }
        let manager = NSFileProviderManager(for: domain)
        manager?.signalEnumerator(for: containerItemIdentifier) { _ in }
    }
}

import Foundation

/// Kept separate from app startup so cold launch, cancellation, and invalidation are predictable.
final class FileProviderProcessLifecycle {
    enum State: Equatable {
        case cold
        case active
        case invalidated
    }

    private let lock = NSLock()
    private var currentState: State = .cold

    var state: State {
        lock.lock()
        defer { lock.unlock() }
        return currentState
    }

    var mayAccessApplicationSession: Bool { false }
    var mayStartAuthenticatedNetwork: Bool { false }
    var maySubmitBackgroundTransfers: Bool { false }

    func launch() {
        lock.lock()
        defer { lock.unlock() }
        if currentState == .cold { currentState = .active }
    }

    func invalidate() {
        lock.lock()
        defer { lock.unlock() }
        currentState = .invalidated
    }

    func canContinueRequest() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return currentState == .active
    }
}

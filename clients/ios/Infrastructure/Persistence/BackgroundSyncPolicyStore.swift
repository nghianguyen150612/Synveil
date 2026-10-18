import Foundation

@MainActor
protocol BackgroundSyncPolicyStorage {
    func read() -> BackgroundSyncPolicy
    @discardableResult func write(_ policy: BackgroundSyncPolicy) -> Bool
}

/// Versioned non-secret settings. Missing, oversized or invalid data always disables refresh.
/// No schema migration, token, filesystem path, execution history or protocol state lives here.
@MainActor
final class BackgroundSyncPolicyStore: BackgroundSyncPolicyStorage {
    static let key = "synveil.background-metadata.policy.v1"
    private let defaults: UserDefaults
    private var failedClosed = false
    init(defaults: UserDefaults = .standard) { self.defaults = defaults }
    func read() -> BackgroundSyncPolicy {
        guard !failedClosed, let bytes = defaults.data(forKey: Self.key), bytes.count <= 4096,
            let policy = try? JSONDecoder().decode(BackgroundSyncPolicy.self, from: bytes),
            policy.isValid
        else { return .disabled }
        return policy
    }
    @discardableResult
    func write(_ policy: BackgroundSyncPolicy) -> Bool {
        guard policy.isValid, let bytes = try? JSONEncoder().encode(policy), bytes.count <= 4096
        else {
            failedClosed = true
            return false
        }
        defaults.set(bytes, forKey: Self.key)
        failedClosed = defaults.data(forKey: Self.key) != bytes
        return !failedClosed
    }
}

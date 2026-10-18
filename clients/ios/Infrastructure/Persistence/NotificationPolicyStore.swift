import CryptoKit
import Foundation

enum NotificationFingerprint {
    static func hash(_ values: [String]) -> String {
        let bytes = (try? JSONEncoder().encode(values)) ?? Data()
        return SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    }
    static func scope(_ session: LibraryRequestScope) -> String {
        hash([
            session.serverEndpoint.urlString, session.ownerUserId, session.deviceIdentifier,
            session.credentialIdentifier,
        ])
    }
}

@MainActor
final class NotificationPolicyStore: NotificationPolicyStorage {
    private let defaults: UserDefaults
    private let key = "synveil.local-notifications.policy.v1"
    init(defaults: UserDefaults = .standard) { self.defaults = defaults }
    func read() -> NotificationPolicy {
        guard let bytes = defaults.data(forKey: key), bytes.count <= 4096,
            let policy = try? JSONDecoder().decode(NotificationPolicy.self, from: bytes),
            policy.isValid
        else { return NotificationPolicy() }
        return policy
    }
    func write(_ policy: NotificationPolicy) -> Bool {
        guard policy.isValid, let bytes = try? JSONEncoder().encode(policy) else { return false }
        defaults.set(bytes, forKey: key)
        return read() == policy
    }
}

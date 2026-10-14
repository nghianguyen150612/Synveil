import Foundation

enum LocalNotificationCategory: String, Codable, CaseIterable, Sendable {
    case verifiedDownload, committedUpload, backgroundSync, recovery

    var label: String {
        switch self {
        case .verifiedDownload: "Verified Downloads"
        case .committedUpload: "Committed Uploads"
        case .backgroundSync: "Background Sync Updates"
        case .recovery: "Recovery Requires Attention"
        }
    }
    var title: String {
        switch self {
        case .verifiedDownload: "Synveil download ready"
        case .committedUpload: "Synveil upload completed"
        case .backgroundSync: "Synveil synchronization updated"
        case .recovery: "Synveil needs attention"
        }
    }
}

enum NotificationAuthorization: String, Sendable {
    case notDetermined, authorized, denied, provisional, ephemeral, unavailable
    var label: String {
        switch self {
        case .notDetermined: "Permission not requested"
        case .authorized: "Allowed by iOS"
        case .denied: "Denied by iOS"
        case .provisional: "Provisional permission — delivery disabled"
        case .ephemeral: "Temporary permission — delivery disabled"
        case .unavailable: "Notifications unavailable"
        }
    }
}

struct NotificationPolicy: Codable, Equatable, Sendable {
    var version = 1
    var enabled = false
    var categories: Set<LocalNotificationCategory> = []
    var scopeFingerprint: String?
    var generation = UUID()
    var isValid: Bool {
        version == 1 && (scopeFingerprint == nil || NotificationLimits.validHash(scopeFingerprint!))
            && (!enabled || scopeFingerprint != nil)
    }
}

enum NotificationEvidence: String, Codable, Sendable {
    case verifiedDownload, committedUpload, confirmedInbound, uncertainUpload, unknownAck
}

/// References only existing durable evidence. No names, paths, response bodies or credentials.
struct NotificationCandidate: Codable, Equatable, Sendable {
    let category: LocalNotificationCategory
    let evidence: NotificationEvidence
    let operation: String
    let library: String
    let scopeFingerprint: String
    let proofFingerprint: String
    var isValid: Bool {
        UUID(uuidString: operation)?.uuidString.lowercased() == operation
            && BackgroundSyncIdentity.validIdentifier(library)
            && NotificationLimits.validHash(scopeFingerprint)
            && NotificationLimits.validHash(proofFingerprint)
            && category == evidence.category
    }
}

extension NotificationEvidence {
    var category: LocalNotificationCategory {
        switch self {
        case .verifiedDownload: .verifiedDownload
        case .committedUpload: .committedUpload
        case .confirmedInbound: .backgroundSync
        case .uncertainUpload, .unknownAck: .recovery
        }
    }
}

enum NotificationDeliveryState: String, Codable, Sendable {
    case eligible, claimed, scheduled, handled, invalidated
}

struct NotificationEvent: Codable, Equatable, Sendable {
    let id: UUID
    let candidate: NotificationCandidate
    let generation: UUID
    let lifecycleRevision: UInt64
    let recordedAt: Date
    let expiresAt: Date
    var state: NotificationDeliveryState
    var isValid: Bool {
        candidate.isValid && recordedAt.timeIntervalSince1970.isFinite
            && expiresAt.timeIntervalSince1970.isFinite && expiresAt > recordedAt
            && expiresAt.timeIntervalSince(recordedAt) <= NotificationLimits.routingLifetime
    }
    var deduplicationKey: String {
        [
            generation.uuidString, candidate.category.rawValue, candidate.evidence.rawValue,
            candidate.operation, candidate.library, candidate.scopeFingerprint,
            candidate.proofFingerprint,
        ]
        .joined(separator: ":")
    }
    var requestIdentifier: String { NotificationLimits.prefix + id.uuidString.lowercased() }
}

struct LocalNotificationPayload: Equatable, Sendable {
    let handle: UUID
    let category: LocalNotificationCategory
    var identifier: String { NotificationLimits.prefix + handle.uuidString.lowercased() }
    var title: String { category.title }
    var body: String { "Open Synveil to view the current status." }
    var userInfo: [String: String] { ["event": handle.uuidString.lowercased()] }
}

enum NotificationFailure: String, Error, Sendable {
    case authorizationDenied, userDisabled, categoryDisabled, schedulingError, payloadRejected
    case invalidIntent, sessionInvalidated, accountMismatch, durableEventUnavailable
    case alreadyHandled, startupPending, storageFailure, capacity
}

enum NotificationLimits {
    static let prefix = "synveil.local."
    static let maximumPending = 16
    static let maximumRecords = 512
    static let maximumBytes = 512 * 1024
    static let routingLifetime: TimeInterval = 7 * 24 * 60 * 60
    static let intentLifetime: TimeInterval = 10 * 60
    static let syncQuietPeriod: TimeInterval = 6 * 60 * 60
    static func validHash(_ value: String) -> Bool {
        LibraryWireValidation.matches(value, pattern: "^[0-9a-f]{64}$")
    }
    static func handle(identifier: String, userInfo: [String: String]) -> UUID? {
        guard userInfo.count == 1, let raw = userInfo["event"],
            let id = UUID(uuidString: raw), id.uuidString.lowercased() == raw,
            identifier == prefix + raw
        else { return nil }
        return id
    }
}

@MainActor
protocol LocalNotificationScheduling {
    func authorization() async -> NotificationAuthorization
    func requestAuthorization() async throws
    func pendingIdentifiers() async -> [String]
    func schedule(_ payload: LocalNotificationPayload) async throws
    func remove(_ identifiers: [String])
}

@MainActor
protocol NotificationPolicyStorage {
    func read() -> NotificationPolicy
    func write(_ policy: NotificationPolicy) -> Bool
}

protocol NotificationLedgerStorage: Sendable {
    func events() async throws -> [NotificationEvent]
    func insert(_ event: NotificationEvent) async throws -> NotificationEvent
    func claim(_ id: UUID, now: Date) async throws -> Bool
    func transition(_ id: UUID, from: NotificationDeliveryState, to: NotificationDeliveryState)
        async throws -> Bool
    func invalidate(generation: UUID) async throws -> [String]
}

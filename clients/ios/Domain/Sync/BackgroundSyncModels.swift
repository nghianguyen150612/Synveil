import Foundation

enum BackgroundSyncNetworkPolicy: String, Codable, CaseIterable, Sendable {
    case unmetered, cellular
    var label: String {
        switch self {
        case .unmetered: "Wi-Fi preferred / unmetered only"
        case .cellular: "Wi-Fi and cellular"
        }
    }
}

/// Non-secret identifiers fence opt-in to one endpoint, account, device and credential identity.
struct BackgroundSyncIdentity: Codable, Equatable, Sendable {
    let endpoint: String
    let owner: String
    let device: String
    let credential: String
    init(_ session: LibraryRequestScope) {
        endpoint = session.serverEndpoint.urlString
        owner = session.ownerUserId
        device = session.deviceIdentifier
        credential = session.credentialIdentifier
    }
    var isValid: Bool {
        guard endpoint.utf8.count <= 2048,
            let parsed = try? ServerEndpoint(validating: endpoint),
            parsed.isSecureScheme, parsed.urlString == endpoint
        else { return false }
        return [owner, device, credential].allSatisfy(Self.validIdentifier)
    }
    static func validIdentifier(_ value: String) -> Bool {
        LibraryWireValidation.matches(
            value, pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    }
}

struct BackgroundSyncPolicy: Codable, Equatable, Sendable {
    var version = 1
    var enabled = false
    var network: BackgroundSyncNetworkPolicy = .unmetered
    var identity: BackgroundSyncIdentity?
    var grant: UUID?
    /// A selection cursor is fairness information, never a sync checkpoint or permission grant.
    var afterLibrary: String?
    var nextSubmissionAllowedAt: Date?
    static let disabled = Self()
    var isValid: Bool {
        version == 1 && (!enabled || identity?.isValid == true && grant != nil)
            && (identity == nil || identity?.isValid == true)
            && (afterLibrary == nil || BackgroundSyncIdentity.validIdentifier(afterLibrary!))
    }
}

enum BackgroundSyncState: String, Sendable {
    case disabled, eligible, scheduled, running, deferred, completed, partial, expired, offline
    case authenticationRequired, recoveryRequired, failed, invalidated
}

enum BackgroundSyncLimits {
    static let identifier = "com.synveil.ios.sync.refresh.v1"
    /// A lower boundary only. iOS may run much later or never deliver a task.
    static let schedulingDelay: TimeInterval = 6 * 60 * 60
    static let maximumLibraries = 2
    static let runtime: TimeInterval = 24
    static let requestTimeout: TimeInterval = 3
    static let resourceTimeout: TimeInterval = 4
    static let cleanupReserve: TimeInterval = 1
    static let responseBytes = 64 * 1024
}

/// The date and positions come from an ACK_CONFIRMED SQLite page, not a scheduler clock.
struct BackgroundSyncConfirmation: Equatable, Sendable {
    let library: LibraryId
    let recordedAt: Date
    let position: SyncJournalPosition
}

struct BackgroundSyncOutcome: Equatable, Sendable {
    let state: BackgroundSyncState
    var pages = 0
    var events = 0
    var confirmation: BackgroundSyncConfirmation?
    var taskSucceeded: Bool { state == .completed || state == .partial && pages > 0 }
}

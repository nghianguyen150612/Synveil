import Foundation

/// High-level kinds of enrollment UI state.
public enum EnrollmentStateKind: Sendable, Equatable {
    case notEnrolled
    case invalidToken
    case pending
    case enrolled
    case recoveryRequired
    case secureStoreUnavailable
}

/// UI enrollment state with optional user message.
public struct EnrollmentState: Sendable, Equatable {
    public let kind: EnrollmentStateKind
    public let message: String?

    public init(kind: EnrollmentStateKind, message: String? = nil) {
        self.kind = kind
        self.message = message
    }
}

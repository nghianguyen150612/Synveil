import Foundation

/// High-level application recovery/error classifications.
public enum AppRecoveryReason: Equatable, Hashable, Sendable {
    case configuration
    case authentication
    case deviceRevoked
    case secureStore
    case credential
    case scopeMismatch
    case tls
    case protocolFailure
    case enrollmentAmbiguous
    case transport
}

/// Root application startup and lifecycle state representation.
///
/// Pure domain/application state enum driving top-level UI routing.
/// Contains no UI framework imports or secret payloads.
public enum AppStartupState: Equatable, Hashable, Sendable {
    /// Transient state during initial app launching / configuration evaluation.
    case initializing

    /// No configured server endpoint / profile is available.
    case needsServerProfile

    /// Bootstrap configuration or server endpoint exists, ready for server validation in Phase D.
    case readyForServerValidation

    /// Server profile exists and is reachable, but device enrollment is required.
    case needsEnrollment

    /// A locally valid stored session is waiting for a successful remote authorization check.
    case restorationVerificationPending

    /// Device is enrolled and authenticated session is active.
    case authenticated

    /// Explicit local credential cleanup is running; authenticated work is blocked.
    case logoutInProgress

    /// Local credential cleanup failed or could not be verified; explicit retry is available.
    case logoutCleanupRequired

    /// Application encountered an error requiring explicit user recovery action.
    case recoveryRequired(AppRecoveryReason)
}

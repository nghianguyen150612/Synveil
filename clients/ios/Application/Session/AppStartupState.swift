import Foundation

/// High-level application recovery/error classifications.
public enum AppRecoveryReason: Equatable, Hashable, Sendable {
    case configuration
    case authentication
    case deviceRevoked
    case secureStore
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

    /// Device is enrolled and authenticated session is active.
    case authenticated

    /// Application encountered an error requiring explicit user recovery action.
    case recoveryRequired(AppRecoveryReason)
}

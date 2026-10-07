import Foundation

/// Boundary protocol defining the Keychain credential storage preflight and handoff contract.
///
/// # Prompt024 ↔ Prompt025 Security Invariant
/// Prompt024 performs device enrollment exchange over the network. Prompt025 owns durable Keychain persistence.
/// Before invoking single-shot `POST /api/v1/device-enrollment/exchange`, `SecureCredentialSinkProtocol.preflight()`
/// must verify that secure storage is available so a single-shot token is never consumed without secure persistence readiness.
public protocol SecureCredentialSinkProtocol: Sendable {
    /// Checks whether secure storage (Keychain) is available and functional.
    ///
    /// - Throws: An error if secure storage is unavailable or failing preflight checks.
    func preflight() async throws

    /// Stores the newly exchanged device credential record securely.
    ///
    /// - Parameter record: Validated `DeviceCredentialRecord` returned from enrollment exchange.
    /// - Throws: An error if storage fails.
    func store(_ record: DeviceCredentialRecord) async throws
}

/// Stub implementation of `SecureCredentialSinkProtocol` used in Prompt024 when Keychain persistence is deferred to Prompt025.
///
/// By default, this stub passes `preflight()` for testing/gating or fails if configured to simulate secure storage unavailability.
public struct StubSecureCredentialSink: SecureCredentialSinkProtocol {
    private let isAvailable: Bool

    public init(isAvailable: Bool = true) {
        self.isAvailable = isAvailable
    }

    public func preflight() async throws {
        guard isAvailable else {
            throw SecureCredentialSinkError.unavailable
        }
    }

    public func store(_ record: DeviceCredentialRecord) async throws {
        guard isAvailable else {
            throw SecureCredentialSinkError.storageFailed
        }
    }
}

/// Errors raised by `SecureCredentialSinkProtocol`.
public enum SecureCredentialSinkError: Error, Equatable, Sendable {
    case unavailable
    case storageFailed
}

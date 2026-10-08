import Foundation
import Observation

/// Authoritative MainActor-isolated session manager driving application root startup state.
///
/// Responsible for orchestrating top-level application startup transitions and exposing
/// state updates to the SwiftUI root shell.
@Observable
@MainActor
public final class SessionController {
    /// The current application startup state.
    public private(set) var state: AppStartupState

    /// The active server endpoint profile, if configured.
    public private(set) var serverEndpoint: ServerEndpoint?

    /// Bootstrap application configuration.
    private let configuration: AppConfiguration

    /// Indicates whether initial startup state resolution has completed.
    private var isStarted: Bool = false

    /// Initializes `SessionController` with application configuration.
    ///
    /// - Parameter configuration: App configuration containing non-secret bootstrap settings.
    public init(configuration: AppConfiguration = AppConfiguration.load()) {
        self.configuration = configuration
        self.serverEndpoint = configuration.serverEndpoint
        self.state = .initializing
    }

    /// Resolves the initial application state deterministically and idempotently.
    ///
    /// If startup resolution has already executed, subsequent invocations are no-ops.
    public func start() async {
        guard !isStarted else {
            return
        }
        isStarted = true

        if configuration.serverEndpoint != nil {
            state = .readyForServerValidation
        } else {
            state = .needsServerProfile
        }
    }

    // MARK: - Controlled State Transitions (Seams for future Phase D integration)

    /// Transitions root state to `.needsServerProfile`.
    public func showServerProfileSetup() {
        state = .needsServerProfile
    }

    /// Configures a server endpoint and transitions state to `.readyForServerValidation`.
    ///
    /// - Parameter endpoint: The validated server base endpoint.
    public func configureServerEndpoint(_ endpoint: ServerEndpoint) {
        guard state == .needsServerProfile else {
            return
        }
        self.serverEndpoint = endpoint
        state = .readyForServerValidation
    }

    /// Transitions from server setup to `.readyForServerValidation`.
    ///
    /// Invalid source states are ignored so future integration cannot bypass root lifecycle gates.
    public func markServerReadyForValidation() {
        guard state == .needsServerProfile else {
            return
        }
        state = .readyForServerValidation
    }

    /// Transitions from a validated server boundary to `.needsEnrollment`.
    ///
    /// P021 does not perform server validation; later Phase D work owns the real validation event.
    public func requireEnrollment() {
        guard state == .readyForServerValidation else {
            return
        }
        state = .needsEnrollment
    }

    /// Transitions from enrollment-required state to `.authenticated` after verified persistence.
    ///
    /// The receipt is returned by the secure credential boundary only after read-back verification.
    /// Its endpoint must still match the configured server.
    public func markAuthenticated(after receipt: SecureCredentialPersistenceReceipt) {
        guard
            state == .needsEnrollment,
            serverEndpoint?.urlString == receipt.canonicalServerEndpoint
        else {
            return
        }
        state = .authenticated
    }

    /// Transitions root state to `.recoveryRequired` with the given reason.
    ///
    /// - Parameter reason: High-level classification of the recovery requirement.
    public func requireRecovery(_ reason: AppRecoveryReason) {
        state = .recoveryRequired(reason)
    }
}

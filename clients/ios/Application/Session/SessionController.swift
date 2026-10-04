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

    /// Bootstrap application configuration.
    private let configuration: AppConfiguration

    /// Indicates whether initial startup state resolution has completed.
    private var isStarted: Bool = false

    /// Initializes `SessionController` with application configuration.
    ///
    /// - Parameter configuration: App configuration containing non-secret bootstrap settings.
    public init(configuration: AppConfiguration = AppConfiguration.load()) {
        self.configuration = configuration
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

    /// Transitions from server setup to `.readyForServerValidation`.
    ///
    /// Invalid source states are ignored so future integration cannot bypass root lifecycle gates.
    public func markServerReadyForValidation() {
        guard state == .needsServerProfile else {
            return
        }
        state = .readyForServerValidation
    }

    /// Transitions from server setup or validated server boundary to `.needsEnrollment`.
    public func requireEnrollment() {
        guard state == .needsServerProfile || state == .readyForServerValidation else {
            return
        }
        state = .needsEnrollment
    }

    /// Transitions from enrollment-required state to `.authenticated`.
    ///
    /// Authentication cannot be entered directly from recovery or unrelated root states.
    public func markAuthenticated() {
        guard state == .needsEnrollment else {
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

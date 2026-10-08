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

    @ObservationIgnored private var restorationService: (any SessionRestorationServiceProtocol)?
    @ObservationIgnored private var logoutService: (any SessionLogoutServiceProtocol)?
    @ObservationIgnored private var restorationSetupFailed = false
    @ObservationIgnored private var hasCompletedStartup = false
    @ObservationIgnored private var isStartupInProgress = false
    @ObservationIgnored private var transitionRevision: UInt64 = 0
    @ObservationIgnored private var pendingRestorationSession: DeviceCredentialSession?
    @ObservationIgnored private var activeRestorationTask: Task<SessionRestorationResult, Never>?
    @ObservationIgnored private var activeRestorationOperationID: UUID?
    @ObservationIgnored private var logoutTask: Task<Void, Never>?

    /// Observable only so the root retry action can disable itself while a probe is in flight.
    public private(set) var isRestorationRetryInProgress = false

    /// Typed non-secret failure context; cleared on every authoritative transition.
    public private(set) var restorationFailure: SessionRestorationVerificationFailure?
    public private(set) var secureStorageFailure: SessionSecureStorageFailure?

    /// Used by phase view models to discard callbacks after any newer lifecycle transition.
    var lifecycleRevision: UInt64 { transitionRevision }

    /// Non-secret cleanup failure retained while the user retries secure deletion.
    public private(set) var logoutFailure: SessionLogoutError?

    /// Initializes `SessionController` with application configuration and optional test service.
    ///
    /// - Parameters:
    ///   - configuration: App configuration containing non-secret bootstrap settings.
    ///   - restorationService: Injectable restoration service. Production composition installs it
    ///     after Rust validation and Keychain dependencies initialize.
    ///   - logoutService: Injectable local credential cleanup service.
    public init(
        configuration: AppConfiguration = AppConfiguration.load(),
        restorationService: (any SessionRestorationServiceProtocol)? = nil,
        logoutService: (any SessionLogoutServiceProtocol)? = nil
    ) {
        self.configuration = configuration
        self.serverEndpoint = configuration.serverEndpoint
        self.state = .initializing
        self.restorationService = restorationService
        self.logoutService = logoutService
    }

    /// Installs the production restoration service before startup begins.
    ///
    /// - Parameter service: The service backed by the initialized Rust bridge and Keychain store.
    public func installRestorationService(_ service: any SessionRestorationServiceProtocol) {
        guard state == .initializing, !isStartupInProgress else {
            return
        }
        restorationService = service
        restorationSetupFailed = false
    }

    /// Installs local credential cleanup before startup begins.
    public func installLogoutService(_ service: any SessionLogoutServiceProtocol) {
        guard state == .initializing, !isStartupInProgress else {
            return
        }
        logoutService = service
    }

    /// Records fail-closed startup when Rust validation or secure storage could not initialize.
    public func markRestorationDependenciesUnavailable() {
        guard state == .initializing, !isStartupInProgress else {
            return
        }
        restorationService = nil
        restorationSetupFailed = true
    }

    /// Restores a previously enrolled session and verifies it with the server.
    ///
    /// Simultaneous or repeated startup calls do not create additional Keychain loads or probes.
    public func start() async {
        guard state == .initializing, !hasCompletedStartup, !isStartupInProgress else {
            return
        }
        isStartupInProgress = true
        defer { isStartupInProgress = false }
        let capturedRevision = transitionRevision

        if restorationSetupFailed {
            guard isCurrent(capturedRevision, expectedState: .initializing) else {
                return
            }
            transition(to: .recoveryRequired(.secureStore))
            hasCompletedStartup = true
            return
        }

        guard let restorationService else {
            // Dependency-free previews and state-machine tests remain unauthenticated. Production
            // composition marks failed initialization explicitly before calling start().
            applyNoStoredSession(configuredEndpoint: configuration.serverEndpoint)
            hasCompletedStartup = true
            return
        }

        let operationID = UUID()
        let task = Task {
            await restorationService.restore(configuredServerEndpoint: serverEndpoint)
        }
        activeRestorationOperationID = operationID
        activeRestorationTask = task
        let result = await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
        guard activeRestorationOperationID == operationID else {
            return
        }
        activeRestorationOperationID = nil
        activeRestorationTask = nil

        let safeResult = cancellationSafeResult(result)
        guard isCurrent(capturedRevision, expectedState: .initializing) else {
            return
        }
        apply(safeResult)
        hasCompletedStartup = true
    }

    /// Retries remote authorization for the same locally validated stored session.
    ///
    /// If startup was cancelled before a session was loaded, the retry performs the same validated
    /// Keychain lookup. It never invokes enrollment or changes the stored credential.
    public func retrySessionRestoration() async {
        guard
            state == .restorationVerificationPending,
            !isStartupInProgress,
            let restorationService
        else {
            return
        }

        isStartupInProgress = true
        isRestorationRetryInProgress = true
        restorationFailure = nil
        defer {
            isRestorationRetryInProgress = false
            isStartupInProgress = false
        }
        let capturedRevision = transitionRevision

        let operationID = UUID()
        let task: Task<SessionRestorationResult, Never>
        if let pendingRestorationSession, let serverEndpoint {
            task = Task {
                await restorationService.retryVerification(
                    of: pendingRestorationSession,
                    expectedServerEndpoint: serverEndpoint
                )
            }
        } else {
            task = Task {
                await restorationService.restore(configuredServerEndpoint: serverEndpoint)
            }
        }
        activeRestorationOperationID = operationID
        activeRestorationTask = task
        let result = await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
        guard activeRestorationOperationID == operationID else {
            return
        }
        activeRestorationOperationID = nil
        activeRestorationTask = nil

        let safeResult = cancellationSafeResult(result)
        guard isCurrent(capturedRevision, expectedState: .restorationVerificationPending) else {
            return
        }
        apply(safeResult)
    }

    // MARK: - Controlled State Transitions (Seams for future Phase D integration)

    /// Transitions root state to `.needsServerProfile`.
    public func showServerProfileSetup() {
        guard state == .initializing || state == .needsServerProfile else { return }
        transition(to: .needsServerProfile)
    }

    /// Configures a server endpoint and transitions state to `.readyForServerValidation`.
    ///
    /// - Parameter endpoint: The validated server base endpoint.
    public func configureServerEndpoint(_ endpoint: ServerEndpoint) {
        guard state == .needsServerProfile else {
            return
        }
        self.serverEndpoint = endpoint
        transition(to: .readyForServerValidation)
    }

    /// Transitions from server setup to `.readyForServerValidation`.
    ///
    /// Invalid source states are ignored so future integration cannot bypass root lifecycle gates.
    public func markServerReadyForValidation() {
        guard state == .needsServerProfile else {
            return
        }
        transition(to: .readyForServerValidation)
    }

    /// Transitions from a validated server boundary to `.needsEnrollment`.
    ///
    /// P021 does not perform server validation; later Phase D work owns the real validation event.
    public func requireEnrollment() {
        guard state == .readyForServerValidation else {
            return
        }
        transition(to: .needsEnrollment)
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
        transition(to: .authenticated)
    }

    /// Transitions root state to `.recoveryRequired` with the given reason.
    ///
    /// - Parameter reason: High-level classification of the recovery requirement.
    public func requireRecovery(_ reason: AppRecoveryReason) {
        guard !isLogoutTransitionActive else { return }
        pendingRestorationSession = nil
        transition(to: .recoveryRequired(reason))
    }

    /// Starts explicit local logout or retries a previously failed secure cleanup.
    ///
    /// The authenticated state is invalidated before cleanup starts. The cleanup task is owned by
    /// this controller and deliberately outlives cancellation or disappearance of the SwiftUI
    /// caller. Concurrent requests await the same operation.
    public func requestLogout() async {
        if let logoutTask {
            await logoutTask.value
            return
        }

        guard
            state == .authenticated
                || state == .restorationVerificationPending
                || state == .logoutCleanupRequired
        else {
            return
        }

        transition(to: .logoutInProgress)
        logoutFailure = nil
        hasCompletedStartup = true
        pendingRestorationSession = nil
        isStartupInProgress = false
        isRestorationRetryInProgress = false

        activeRestorationTask?.cancel()
        activeRestorationTask = nil
        activeRestorationOperationID = nil

        let capturedRevision = transitionRevision
        let service = logoutService
        let task = Task { @MainActor [weak self, service] in
            let result: SessionLogoutResult
            if let service {
                result = await service.logoutLocally()
            } else {
                result = .failed(.serviceUnavailable)
            }
            guard let self else { return }
            self.finishLogout(result, capturedRevision: capturedRevision)
            self.logoutTask = nil
        }
        logoutTask = task
        await task.value
    }

    private func apply(_ result: SessionRestorationResult) {
        switch result {
        case .noStoredSession(let configuredEndpoint):
            pendingRestorationSession = nil
            applyNoStoredSession(configuredEndpoint: configuredEndpoint)
        case .remotelyVerifiedAuthorizedSession(let session):
            guard serverEndpoint == nil || serverEndpoint == session.serverEndpoint else {
                pendingRestorationSession = nil
                transition(to: .recoveryRequired(.scopeMismatch))
                return
            }
            serverEndpoint = session.serverEndpoint
            pendingRestorationSession = nil
            markAuthenticated(afterVerifiedRestoration: session)
        case .locallyValidStoredSession(let session, let verification):
            serverEndpoint = session.serverEndpoint
            switch verification {
            case .serverUnavailable, .dnsFailure, .networkFailure, .timeout, .cancelled:
                pendingRestorationSession = session
                transition(to: .restorationVerificationPending)
            case .authenticationRejected:
                pendingRestorationSession = nil
                transition(to: .recoveryRequired(.authentication))
            case .deviceRevoked:
                pendingRestorationSession = nil
                transition(to: .recoveryRequired(.deviceRevoked))
            case .tlsFailure:
                pendingRestorationSession = nil
                transition(to: .recoveryRequired(.tls))
            case .unexpectedContentType, .unexpectedProtocolResponse, .redirectRejected:
                pendingRestorationSession = nil
                transition(to: .recoveryRequired(.protocolFailure))
            }
            restorationFailure = verification
        case .keychainUnavailable:
            applyStorageFailure(.unavailable)
        case .keychainReadFailure:
            applyStorageFailure(.readFailure)
        case .keychainFailure:
            applyStorageFailure(.failure)
        case .corruptedKeychainSession:
            applyStorageFailure(.corruptRecord)
        case .unsupportedKeychainFormat:
            applyStorageFailure(.unsupportedRecord)
        case .serverOriginScopeMismatch:
            pendingRestorationSession = nil
            transition(to: .recoveryRequired(.scopeMismatch))
        case .sessionChanged(let endpoint):
            serverEndpoint = endpoint
            pendingRestorationSession = nil
            transition(to: .restorationVerificationPending)
        case .invalidCredential:
            pendingRestorationSession = nil
            transition(to: .recoveryRequired(.credential))
        case .cancelled:
            pendingRestorationSession = nil
            transition(to: .restorationVerificationPending)
        }
    }

    private func applyStorageFailure(_ failure: SessionSecureStorageFailure) {
        pendingRestorationSession = nil
        transition(to: .recoveryRequired(.secureStore))
        secureStorageFailure = failure
    }

    private func applyNoStoredSession(configuredEndpoint: ServerEndpoint?) {
        serverEndpoint = configuredEndpoint
        if configuredEndpoint == nil {
            transition(to: .needsServerProfile)
        } else {
            transition(to: .readyForServerValidation)
        }
    }

    /// Restoration has its own authorization proof; it does not reuse the enrollment receipt gate.
    private func markAuthenticated(afterVerifiedRestoration session: DeviceCredentialSession) {
        guard
            state == .initializing || state == .restorationVerificationPending,
            serverEndpoint == session.serverEndpoint
        else {
            return
        }
        guard !Task.isCancelled else {
            pendingRestorationSession = session
            transition(to: .restorationVerificationPending)
            return
        }
        transition(to: .authenticated)
    }

    private func cancellationSafeResult(
        _ result: SessionRestorationResult
    ) -> SessionRestorationResult {
        guard Task.isCancelled else {
            return result
        }
        switch result {
        case .remotelyVerifiedAuthorizedSession(let session),
            .locallyValidStoredSession(let session, _):
            return .locallyValidStoredSession(session, verification: .cancelled)
        case .noStoredSession, .keychainUnavailable, .keychainReadFailure,
            .corruptedKeychainSession,
            .unsupportedKeychainFormat, .serverOriginScopeMismatch, .sessionChanged,
            .invalidCredential, .keychainFailure, .cancelled:
            return .cancelled
        }
    }

    private func isCurrent(_ revision: UInt64, expectedState: AppStartupState) -> Bool {
        transitionRevision == revision && state == expectedState
    }

    private var isLogoutTransitionActive: Bool {
        state == .logoutInProgress || state == .logoutCleanupRequired
    }

    private func finishLogout(
        _ result: SessionLogoutResult,
        capturedRevision: UInt64
    ) {
        guard isCurrent(capturedRevision, expectedState: .logoutInProgress) else {
            return
        }

        switch result {
        case .credentialAbsent:
            logoutFailure = nil
            pendingRestorationSession = nil
            if serverEndpoint == nil {
                transition(to: .needsServerProfile)
            } else {
                transition(to: .readyForServerValidation)
            }
        case .failed(let error):
            logoutFailure = error
            pendingRestorationSession = nil
            transition(to: .logoutCleanupRequired)
        }
    }

    private func transition(to newState: AppStartupState) {
        transitionRevision &+= 1
        restorationFailure = nil
        secureStorageFailure = nil
        state = newState
    }
}

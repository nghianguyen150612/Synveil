import SwiftUI

enum SessionLogoutAccessibility {
    static let logoutButton = "synveil.session.logout"
    static let logoutConfirmation = "synveil.session.logout-confirm"
    static let logoutCancellation = "synveil.session.logout-cancel"
    static let logoutProgress = "synveil.logout.progress"
    static let logoutRetry = "synveil.logout.retry"
}

/// Root application shell router driven by `SessionController.state`.
///
/// Observes the authoritative session state and renders the appropriate root application surface.
struct RootView: View {
    var sessionController: SessionController
    var rustBridge: (any RustBridgeProtocol)? = nil
    var credentialSink: SecureCredentialSinkProtocol? = nil
    var libraryCatalog: (any LibraryCatalogRepositoryProtocol)? = nil
    var nodeRepository: (any NodeRepositoryProtocol)? = nil
    var offlineNodeBrowserService: (any OfflineNodeBrowserServiceProtocol)? = nil
    var metadataMutationFeature: (any MetadataMutationFeatureProtocol)? = nil
    var inboundSyncCoordinator: (any InboundSyncCoordinatorProtocol)? = nil
    var nodeProjectionRepository: (any NodeProjectionRepositoryProtocol)? = nil
    var rebaselineCoordinator: RebaselineCoordinator? = nil
    var syncCheckpointService: (any SyncCheckpointPreparationProtocol)? = nil

    var body: some View {
        switch sessionController.state {
        case .initializing:
            LaunchView()
        case .needsServerProfile:
            ServerSetupView(viewModel: ServerSetupViewModel(sessionController: sessionController))
        case .readyForServerValidation:
            ServerValidationView(
                viewModel: ServerValidationViewModel(sessionController: sessionController)
            )
        case .needsEnrollment:
            EnrollmentView(
                viewModel: EnrollmentViewModel(
                    sessionController: sessionController,
                    credentialSink: credentialSink,
                    rustBridge: rustBridge
                )
            )
        case .restorationVerificationPending:
            RestorationVerificationPendingView(sessionController: sessionController)
        case .authenticated:
            LibraryCatalogView(
                repository: libraryCatalog,
                nodeRepository: nodeRepository,
                offlineNodeBrowserService: offlineNodeBrowserService,
                metadataMutationFeature: metadataMutationFeature,
                inboundSyncCoordinator: inboundSyncCoordinator,
                nodeProjectionRepository: nodeProjectionRepository,
                syncCheckpointService: syncCheckpointService,
                rebaselineCoordinator: rebaselineCoordinator,
                sessionController: sessionController
            )
            // A later authenticated lifecycle receives a fresh transient catalog and navigation stack.
            .id(sessionController.lifecycleRevision)
        case .logoutInProgress:
            LogoutInProgressView()
        case .logoutCleanupRequired:
            LogoutCleanupRecoveryView(sessionController: sessionController)
        case .recoveryRequired(let reason):
            AuthenticationRecoveryView(
                presentation: AuthenticationRecoveryPresenter.recovery(
                    reason, storageFailure: sessionController.secureStorageFailure)
            )
        }
    }
}

// MARK: - Root Surface Placeholders

/// Root transient loading view during app launch and initialization.
struct LaunchView: View {
    var body: some View {
        VStack(spacing: 16) {
            ProgressView()
            Text("Restoring session…")
                .font(.headline)
                .foregroundColor(.secondary)
        }
        .padding()
        .accessibilityIdentifier("synveil.root.initializing")
    }
}

/// Minimal retry surface for a locally valid session whose server status is not yet known.
struct RestorationVerificationPendingView: View {
    let sessionController: SessionController
    @State private var isShowingLogoutConfirmation = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                AuthenticationRecoveryMessageView(
                    presentation: AuthenticationRecoveryPresenter.restoration(
                        sessionController.restorationFailure)
                )
                AuthenticationRecoveryRetryButton(
                    action: .retryVerification,
                    isInProgress: sessionController.isRestorationRetryInProgress
                ) {
                    await sessionController.retrySessionRestoration()
                }

                Button("Forget saved session", role: .destructive) {
                    isShowingLogoutConfirmation = true
                }
                .accessibilityIdentifier("synveil.session.forget-pending")
            }
            .padding()
        }
        .accessibilityIdentifier("synveil.root.verification-pending")
        .confirmationDialog(
            "Forget this device session?",
            isPresented: $isShowingLogoutConfirmation,
            titleVisibility: .visible
        ) {
            Button("Forget Session on This Device", role: .destructive) {
                Task { await sessionController.requestLogout() }
            }
            .accessibilityIdentifier(SessionLogoutAccessibility.logoutConfirmation)
            Button("Cancel", role: .cancel) {}
                .accessibilityIdentifier(SessionLogoutAccessibility.logoutCancellation)
        } message: {
            Text(
                "This deletes the saved credential from this device. It does not revoke "
                    + "the credential on the server."
            )
        }
    }
}

/// Placeholder surface when no server profile is configured.
struct ServerSetupPlaceholderView: View {
    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "server.rack")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)
            Text("Synveil")
                .font(.largeTitle)
                .bold()
            Text("Server Setup Required")
                .font(.title3)
                .foregroundColor(.primary)
            Text("No server profile configured. Please set up a server connection to proceed.")
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal)
        }
        .padding()
        .accessibilityIdentifier("synveil.root.server-setup")
    }
}

/// Placeholder surface when a bootstrap server endpoint is configured but not yet validated.
struct ServerValidationPlaceholderView: View {
    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "network")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)
            Text("Server Configured")
                .font(.title2)
                .bold()
            Text("Connection check not yet performed.")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .padding()
        .accessibilityIdentifier("synveil.root.server-validation")
    }
}

/// Placeholder surface when server is configured but device enrollment is required.
struct EnrollmentPlaceholderView: View {
    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "key.fill")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)
            Text("Device Enrollment")
                .font(.title2)
                .bold()
            Text("Device enrollment required before accessing application data.")
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding()
        .accessibilityIdentifier("synveil.root.enrollment")
    }
}

/// Visible while verified local credential cleanup is running.
struct LogoutInProgressView: View {
    var body: some View {
        VStack(spacing: 16) {
            ProgressView()
                .accessibilityLabel("Removing saved device credential")
            Text("Forgetting this device session…")
                .font(.headline)
            Text("Authenticated work is paused while secure storage is checked.")
                .font(.body)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal)
        }
        .padding()
        .accessibilityIdentifier(SessionLogoutAccessibility.logoutProgress)
    }
}

/// Retry surface retained when Keychain cleanup fails or cannot be verified.
struct LogoutCleanupRecoveryView: View {
    let sessionController: SessionController

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                AuthenticationRecoveryMessageView(
                    presentation: AuthenticationRecoveryPresenter.logoutCleanup
                )
                AuthenticationRecoveryRetryButton(
                    action: .retryCleanup,
                    isInProgress: sessionController.state == .logoutInProgress
                ) {
                    await sessionController.requestLogout()
                }
            }
            .padding(24)
        }
        .accessibilityIdentifier("synveil.logout.recovery")
    }
}

#Preview("Needs Server Setup") {
    RootView(sessionController: SessionController())
}

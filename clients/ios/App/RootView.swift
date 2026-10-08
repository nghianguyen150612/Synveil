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
            AuthenticatedShellPlaceholderView(sessionController: sessionController)
        case .logoutInProgress:
            LogoutInProgressView()
        case .logoutCleanupRequired:
            LogoutCleanupRecoveryView(sessionController: sessionController)
        case .recoveryRequired(let reason):
            RecoveryPlaceholderView(reason: reason)
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
        VStack(spacing: 16) {
            Image(systemName: "network")
                .font(.system(size: 40))
                .foregroundColor(.accentColor)
                .accessibilityHidden(true)
            Text("Session verification pending")
                .font(.title2)
                .bold()
            Text("Connect to the server to verify the saved device session.")
                .font(.body)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal)
            Button {
                Task {
                    await sessionController.retrySessionRestoration()
                }
            } label: {
                if sessionController.isRestorationRetryInProgress {
                    ProgressView("Retrying verification…")
                } else {
                    Text("Retry verification")
                }
            }
            .disabled(sessionController.isRestorationRetryInProgress)
            .accessibilityIdentifier("synveil.root.restoration-retry")
            .accessibilityHint("Checks the saved device session with its server.")

            Button("Forget saved session", role: .destructive) {
                isShowingLogoutConfirmation = true
            }
            .accessibilityIdentifier("synveil.session.forget-pending")
        }
        .padding()
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

/// Placeholder surface for authenticated application home shell.
struct AuthenticatedShellPlaceholderView: View {
    let sessionController: SessionController
    @State private var isShowingLogoutConfirmation = false

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "lock.shield.fill")
                .font(.system(size: 48))
                .foregroundColor(.green)
            Text("Synveil Shell")
                .font(.title2)
                .bold()
            Text("Authenticated Shell Placeholder")
                .font(.subheadline)
                .foregroundColor(.secondary)

            Button("Log Out / Forget Session", role: .destructive) {
                isShowingLogoutConfirmation = true
            }
            .buttonStyle(.bordered)
            .accessibilityIdentifier(SessionLogoutAccessibility.logoutButton)
            .accessibilityLabel("Log out and forget this device session")
            .accessibilityHint(
                "Deletes the saved credential from this device. Server authorization is unchanged."
            )

            Text(
                "Local logout removes this device's saved credential. The server may continue "
                    + "to accept it until an owner revokes it."
            )
            .font(.footnote)
            .foregroundColor(.secondary)
            .multilineTextAlignment(.center)
            .padding(.horizontal)
        }
        .padding()
        .accessibilityIdentifier("synveil.root.authenticated")
        .confirmationDialog(
            "Forget this device session?",
            isPresented: $isShowingLogoutConfirmation,
            titleVisibility: .visible
        ) {
            Button("Log Out and Forget Session", role: .destructive) {
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
        VStack(spacing: 16) {
            Image(systemName: "lock.trianglebadge.exclamationmark")
                .font(.system(size: 40))
                .foregroundColor(.orange)
                .accessibilityHidden(true)
            Text("Secure cleanup needs attention")
                .font(.title2)
                .bold()
            Text(
                "Authenticated work is stopped, but Synveil could not verify that the saved "
                    + "credential was deleted. Retry secure cleanup."
            )
            .font(.body)
            .foregroundColor(.secondary)
            .multilineTextAlignment(.center)
            .padding(.horizontal)
            Button("Retry Cleanup") {
                Task { await sessionController.requestLogout() }
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier(SessionLogoutAccessibility.logoutRetry)
            .accessibilityHint("Retries local credential deletion. No network request is made.")
        }
        .padding()
        .accessibilityIdentifier("synveil.logout.recovery")
    }
}

/// Placeholder surface for recovery situations.
struct RecoveryPlaceholderView: View {
    let reason: AppRecoveryReason

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 48))
                .foregroundColor(.orange)
            Text("Recovery Required")
                .font(.title2)
                .bold()
            Text(description(for: reason))
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal)
        }
        .padding()
        .accessibilityIdentifier("synveil.root.recovery")
    }

    private func description(for reason: AppRecoveryReason) -> String {
        switch reason {
        case .configuration:
            return "Configuration error encountered. Please verify app setup."
        case .authentication:
            return "Authentication failure encountered. Please re-authenticate."
        case .deviceRevoked:
            return "This device authorization has been revoked."
        case .secureStore:
            return "Secure storage error encountered on device."
        case .credential:
            return "The saved device credential could not be validated locally."
        case .scopeMismatch:
            return "The saved session belongs to a different server. "
                + "Return to that server to continue."
        case .tls:
            return "A secure connection to the server could not be verified."
        case .protocolFailure:
            return "The server returned a response Synveil could not verify."
        case .enrollmentAmbiguous:
            return "Enrollment state is ambiguous. Re-enrollment may be required."
        case .transport:
            return "Network transport failure encountered."
        }
    }
}

#Preview("Needs Server Setup") {
    RootView(sessionController: SessionController())
}

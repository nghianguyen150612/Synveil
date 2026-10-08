import SwiftUI

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
        case .authenticated:
            AuthenticatedShellPlaceholderView()
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
            Text("Initializing Synveil…")
                .font(.headline)
                .foregroundColor(.secondary)
        }
        .padding()
        .accessibilityIdentifier("synveil.root.initializing")
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
        }
        .padding()
        .accessibilityIdentifier("synveil.root.authenticated")
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

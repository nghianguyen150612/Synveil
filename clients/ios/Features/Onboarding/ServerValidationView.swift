import SwiftUI

/// Native SwiftUI view for pre-authentication server reachability and readiness validation.
public struct ServerValidationView: View {
    @Bindable var viewModel: ServerValidationViewModel

    public init(viewModel: ServerValidationViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        VStack(spacing: 24) {
            Spacer()

            statusHeaderView

            contentAreaView

            Spacer()

            actionAreaView
        }
        .padding(24)
        .accessibilityIdentifier("synveil.server-validation.container")
        .task {
            viewModel.validateServer()
        }
        .onDisappear {
            viewModel.cancelValidation()
        }
    }

    // MARK: - Header View

    @ViewBuilder
    private var statusHeaderView: some View {
        VStack(spacing: 12) {
            switch viewModel.state {
            case .idle, .checking:
                ProgressView()
                    .scaleEffect(1.5)
                    .padding(.bottom, 8)
                    .accessibilityIdentifier("synveil.server-validation.progress")
            case .ready:
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 56))
                    .foregroundColor(.green)
                    .accessibilityIdentifier("synveil.server-validation.success-icon")
            case .aliveButNotReady:
                Image(systemName: "clock.badge.exclamationmark")
                    .font(.system(size: 56))
                    .foregroundColor(.orange)
                    .accessibilityIdentifier("synveil.server-validation.warning-icon")
            case .failed:
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.system(size: 56))
                    .foregroundColor(.red)
                    .accessibilityIdentifier("synveil.server-validation.error-icon")
            }

            Text("Server Connectivity")
                .font(.title2)
                .bold()
                .accessibilityIdentifier("synveil.server-validation.title")
        }
    }

    // MARK: - Content Area View

    @ViewBuilder
    private var contentAreaView: some View {
        VStack(spacing: 8) {
            switch viewModel.state {
            case .idle, .checking:
                Text("Verifying server reachability and readiness…")
                    .font(.body)
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)
                    .accessibilityIdentifier("synveil.server-validation.checking-text")

            case .ready:
                Text("Server is reachable and ready.")
                    .font(.body)
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)
                    .accessibilityIdentifier("synveil.server-validation.ready-text")

            case .aliveButNotReady, .failed:
                let error = viewModel.userFacingErrorMessage
                Text(error.title)
                    .font(.headline)
                    .foregroundColor(.primary)
                    .accessibilityIdentifier("synveil.server-validation.error-title")

                Text(error.message)
                    .font(.subheadline)
                    .foregroundColor(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)
                    .accessibilityIdentifier("synveil.server-validation.error-message")
            }
        }
    }

    // MARK: - Action Area View

    @ViewBuilder
    private var actionAreaView: some View {
        VStack(spacing: 12) {
            switch viewModel.state {
            case .aliveButNotReady, .failed:
                Button(action: {
                    viewModel.retry()
                }) {
                    HStack {
                        Image(systemName: "arrow.clockwise")
                        Text("Retry Connection")
                    }
                    .font(.headline)
                    .frame(maxWidth: .infinity)
                    .padding()
                    .background(Color.accentColor)
                    .foregroundColor(.white)
                    .cornerRadius(10)
                }
                .accessibilityIdentifier("synveil.server-validation.retry-button")

            case .idle, .checking, .ready:
                EmptyView()
            }
        }
    }
}

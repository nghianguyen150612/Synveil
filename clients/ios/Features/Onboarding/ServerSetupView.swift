import SwiftUI

/// Native SwiftUI view for configuring the Synveil server endpoint.
///
/// Provides user input for the server URL, displays inline validation feedback,
/// and advances the pre-authentication app state when a valid endpoint is entered.
public struct ServerSetupView: View {
    @Bindable var viewModel: ServerSetupViewModel

    public init(viewModel: ServerSetupViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                // MARK: - Header
                VStack(spacing: 12) {
                    Image(systemName: "server.rack")
                        .font(.system(size: 56))
                        .foregroundColor(.accentColor)
                        .accessibilityHidden(true)

                    Text("Connect to Synveil")
                        .font(.largeTitle)
                        .bold()
                        .multilineTextAlignment(.center)
                        .accessibilityIdentifier("synveil.server-setup.title")

                    Text("Enter your Synveil server URL to configure this device connection.")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal)
                }
                .padding(.top, 32)

                // MARK: - Server Address Input
                VStack(alignment: .leading, spacing: 8) {
                    Text("Server Address")
                        .font(.headline)
                        .foregroundColor(.primary)

                    TextField("https://synveil.example.com", text: $viewModel.serverAddressInput)
                        .keyboardType(.URL)
                        .textContentType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled(true)
                        .submitLabel(.continue)
                        .padding(12)
                        .background(Color(uiColor: .secondarySystemBackground))
                        .cornerRadius(10)
                        .onSubmit {
                            viewModel.submit()
                        }
                        .accessibilityIdentifier("synveil.server-setup.input")
                        .accessibilityLabel("Server Address Input Field")

                    Text("Example: https://synveil.example.com or http://192.168.1.100:8443")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }

                // MARK: - Validation Error Feedback
                if let error = viewModel.validationError {
                    HStack(alignment: .top, spacing: 10) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundColor(.red)
                            .accessibilityHidden(true)

                        Text(error)
                            .font(.footnote)
                            .foregroundColor(.red)
                            .multilineTextAlignment(.leading)

                        Spacer(minLength: 0)
                    }
                    .padding(12)
                    .background(Color.red.opacity(0.1))
                    .cornerRadius(8)
                    .accessibilityIdentifier("synveil.server-setup.error-message")
                    .accessibilityLabel("Validation Error: \(error)")
                }

                // MARK: - Action Button
                Button(action: {
                    viewModel.submit()
                }) {
                    HStack {
                        Spacer()
                        if viewModel.isValidating {
                            ProgressView()
                                .progressViewStyle(CircularProgressViewStyle())
                        } else {
                            Text("Continue")
                                .font(.headline)
                        }
                        Spacer()
                    }
                    .padding(.vertical, 12)
                }
                .buttonStyle(.borderedProminent)
                .disabled(
                    viewModel.serverAddressInput.trimmingCharacters(in: .whitespacesAndNewlines)
                        .isEmpty || viewModel.isValidating
                )
                .accessibilityIdentifier("synveil.server-setup.continue-button")
                .accessibilityLabel("Continue")

                Spacer(minLength: 16)
            }
            .padding(.horizontal, 24)
        }
        .accessibilityIdentifier("synveil.server-setup.screen")
        .accessibilityIdentifier("synveil.root.server-setup")
    }
}

#Preview("Server Setup View") {
    let controller = SessionController()
    let viewModel = ServerSetupViewModel(sessionController: controller)
    return ServerSetupView(viewModel: viewModel)
}

import SwiftUI

/// Native SwiftUI device enrollment view for entering one-time `sve1_` enrollment token.
public struct EnrollmentView: View {
    @Bindable var viewModel: EnrollmentViewModel

    public init(viewModel: EnrollmentViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        ScrollView {
            VStack(spacing: 24) {
                HeaderView()

                VStack(alignment: .leading, spacing: 16) {
                    Text("Enrollment Token")
                        .font(.headline)
                        .foregroundColor(.primary)

                    TextField(
                        "sve1_...",
                        text: $viewModel.rawTokenInput
                    )
                    .textFieldStyle(.roundedBorder)
                    .font(.system(.body, design: .monospaced))
                    .autocapitalization(.none)
                    .disableAutocorrection(true)
                    .disabled(viewModel.isSubmitting)
                    .accessibilityIdentifier("synveil.enrollment.token-input")
                    .accessibilityLabel("One-time enrollment token input")

                    if let presentation = viewModel.recoveryPresentation {
                        AuthenticationRecoveryMessageView(presentation: presentation)
                            .accessibilityIdentifier("synveil.enrollment.error-message")
                    }
                }
                .padding(.horizontal)

                if viewModel.isSubmitting {
                    VStack(spacing: 8) {
                        ProgressView()
                            .accessibilityLabel("Enrollment exchange in progress")
                        Text("Exchanging enrollment token…")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                    .accessibilityIdentifier("synveil.enrollment.status")
                } else if viewModel.state == .succeeded {
                    VStack(spacing: 8) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 32))
                            .foregroundColor(.green)
                        Text("Enrollment Exchange Succeeded")
                            .font(.headline)
                            .foregroundColor(.green)
                        Text("Secure credential handoff complete.")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                    .accessibilityIdentifier("synveil.enrollment.status")
                }

                Button(action: {
                    viewModel.submitEnrollment()
                }) {
                    HStack {
                        Spacer()
                        Text("Enroll Device")
                            .bold()
                        Spacer()
                    }
                    .padding()
                    .background(viewModel.isSubmitting ? Color.gray : Color.accentColor)
                    .foregroundColor(.white)
                    .cornerRadius(10)
                }
                .disabled(viewModel.isSubmitting || viewModel.rawTokenInput.isEmpty)
                .accessibilityHint("Validates and exchanges one enrollment grant once.")
                .padding(.horizontal)
                .accessibilityIdentifier("synveil.enrollment.submit-button")

            }
            .padding(.vertical, 24)
        }
        .accessibilityIdentifier("synveil.enrollment.view")
    }
}

private struct HeaderView: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "key.fill")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)

            Text("Device Enrollment")
                .font(.title2)
                .bold()

            Text(
                "Enter the one-time enrollment token provided by your trusted server owner "
                    + "to authorize this device."
            )
            .font(.subheadline)
            .foregroundColor(.secondary)
            .multilineTextAlignment(.center)
            .padding(.horizontal)
        }
    }
}

#Preview {
    let sessionController = SessionController()
    let viewModel = EnrollmentViewModel(sessionController: sessionController)
    return EnrollmentView(viewModel: viewModel)
}

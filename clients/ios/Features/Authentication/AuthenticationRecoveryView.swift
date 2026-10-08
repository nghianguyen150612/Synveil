import SwiftUI

/// Shared native message content. Only allowlisted diagnostic fields reach this component.
struct AuthenticationRecoveryMessageView: View {
    let presentation: AuthenticationRecoveryPresentation

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label(presentation.title, systemImage: presentation.severity.symbol)
                .font(.title2.bold())
                .accessibilityLabel(presentation.title)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("synveil.recovery.title")
            Text(presentation.message)
                .accessibilityIdentifier("synveil.recovery.message")
            Text(presentation.sessionProtection)
                .font(.callout)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("synveil.recovery.protection")
            Text(presentation.nextStep)
                .accessibilityIdentifier("synveil.recovery.next-step")
            if let requestID = presentation.requestID {
                DisclosureGroup("Diagnostic details") {
                    Text("Request ID: \(requestID)")
                        .font(.footnote.monospaced())
                        .textSelection(.enabled)
                        .accessibilityLabel("Request identifier: \(requestID)")
                        .accessibilityIdentifier("synveil.recovery.request-id")
                }
                .accessibilityIdentifier("synveil.recovery.diagnostics")
            }
        }
        .font(.body)
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(presentation.accessibilityIdentifier)
    }
}

/// Permanent recovery has guidance only; no operation can replace lifecycle state from this view.
struct AuthenticationRecoveryView: View {
    let presentation: AuthenticationRecoveryPresentation

    var body: some View {
        ScrollView {
            AuthenticationRecoveryMessageView(presentation: presentation)
                .padding(24)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("synveil.root.recovery")
    }
}

/// A retry caller owns its SwiftUI task, while application services retain lifecycle fencing.
struct AuthenticationRecoveryRetryButton: View {
    let action: AuthenticationRecoveryAction
    let isInProgress: Bool
    let operation: @MainActor () async -> Void
    @State private var task: Task<Void, Never>?
    @State private var isInvoking = false

    var body: some View {
        Button {
            guard !isInvoking, !isInProgress else { return }
            isInvoking = true
            task = Task { @MainActor in
                await operation()
                isInvoking = false
                task = nil
            }
        } label: {
            if isInProgress || isInvoking {
                ProgressView(action == .retryCleanup ? "Retrying cleanup…" : "Retrying verification…")
                    .accessibilityIdentifier("synveil.recovery.retry-progress")
            } else {
                Text(action.label)
            }
        }
        .buttonStyle(.borderedProminent)
        .disabled(isInProgress || isInvoking)
        .accessibilityLabel(isInProgress || isInvoking ? "\(action.label) in progress" : action.label)
        .accessibilityHint(action.hint)
        .accessibilityIdentifier(action.accessibilityIdentifier)
        .onDisappear {
            // Controller-owned logout cleanup deliberately survives caller cancellation (P027).
            task?.cancel()
            task = nil
        }
    }
}

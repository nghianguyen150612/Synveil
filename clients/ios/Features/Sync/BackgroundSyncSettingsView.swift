import SwiftUI

struct BackgroundSyncSettingsView: View {
    let scheduler: BGAppRefreshScheduler
    @State private var changing = false
    var body: some View {
        Form {
            Section {
                Toggle(
                    "Enable Background Sync",
                    isOn: Binding(
                        get: { scheduler.settings.enabled },
                        set: { enabled in
                            changing = true
                            Task {
                                await scheduler.setEnabled(enabled)
                                changing = false
                            }
                        })
                )
                .disabled(changing)
                .accessibilityIdentifier("synveil.background-sync.enabled")
                Picker(
                    "Network preference",
                    selection: Binding(
                        get: { scheduler.settings.network },
                        set: { network in
                            changing = true
                            Task {
                                await scheduler.setNetwork(network)
                                changing = false
                            }
                        })
                ) {
                    ForEach(BackgroundSyncNetworkPolicy.allCases, id: \.self) { policy in
                        Text(policy.label).tag(policy)
                    }
                }
                .disabled(changing)
                .accessibilityIdentifier("synveil.background-sync.network")
            } header: {
                Text("Background metadata synchronization")
            } footer: {
                Text(
                    "iOS controls when background refresh occurs and may not run it. Low Power Mode and constrained networks defer refresh. Only previously set up, eligible libraries in this account are considered. File contents are not transferred."
                )
                .fixedSize(horizontal: false, vertical: true)
            }
            Section("Status") {
                if scheduler.manualRecoveryRequired {
                    Text(
                        "Saved synchronization evidence requires a manual action. Open the library’s sync status."
                    )
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.background-sync.recovery-required")
                }
                Text(message)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.background-sync.status")
                if let confirmation = scheduler.lastConfirmation {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Last saved server confirmation")
                        Text(confirmation.recordedAt, format: .dateTime)
                        Text(
                            "Epoch \(confirmation.position.epoch.rawValue), sequence \(confirmation.position.sequence.rawValue)"
                        )
                        .font(.caption)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("synveil.background-sync.confirmed")
                } else {
                    Text("No saved server confirmation is available.")
                        .accessibilityIdentifier("synveil.background-sync.no-confirmation")
                }
                Text(
                    "A scheduled request is only a request to iOS. A completed batch does not mean every library is fully synchronized."
                )
                .font(.footnote)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
        .navigationTitle("Background Sync")
        .navigationBarTitleDisplayMode(.inline)
        .task { await scheduler.refreshStatus() }
    }
    var message: String {
        switch scheduler.state {
        case .disabled: "Background Sync is off."
        case .eligible: "Background Sync is enabled."
        case .scheduled: "A refresh request is pending with iOS. Delivery is not guaranteed."
        case .running: "A bounded metadata refresh is running."
        case .deferred:
            "Refresh is deferred or unavailable under the current system or scheduling policy."
        case .completed:
            "The permitted batch completed. Other libraries may still need synchronization."
        case .partial: "The refresh budget ended. Saved progress is preserved."
        case .expired:
            "The execution window ended. Saved progress and recovery evidence are preserved."
        case .offline:
            "The server was temporarily unavailable. A future iOS opportunity may try again."
        case .authenticationRequired:
            "Current authorization must be verified before Background Sync can proceed."
        case .recoveryRequired:
            "Manual synchronization setup or recovery is required. Open the library’s sync status."
        case .failed:
            "Scheduling or synchronization could not be verified. Review the library’s sync status."
        case .invalidated:
            "Authorization or settings changed. The previous refresh cannot publish a result."
        }
    }
}

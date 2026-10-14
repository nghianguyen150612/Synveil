import SwiftUI
import UIKit

struct NotificationSettingsView: View {
    let coordinator: NotificationCoordinator
    @State private var changingPermission = false
    @Environment(\.scenePhase) private var scenePhase
    var body: some View {
        Form {
            Section {
                Toggle(
                    "Enable Local Notifications",
                    isOn: Binding(
                        get: { coordinator.policy.enabled },
                        set: { enabled in
                            changingPermission = true
                            Task {
                                await coordinator.setEnabled(enabled)
                                changingPermission = false
                            }
                        })
                )
                .disabled(changingPermission)
                .accessibilityIdentifier("synveil.notifications.enable")
                Text(coordinator.authorization.label)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.notifications.permission")
                Text(
                    "iOS controls notification delivery, including Focus and notification restrictions."
                )
                .font(.footnote)
                .fixedSize(horizontal: false, vertical: true)
                if coordinator.authorization == .denied || coordinator.authorization == .unavailable
                {
                    Button("Open iOS Settings") {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            UIApplication.shared.open(url)
                        }
                    }
                    .accessibilityHint(
                        "Opens Synveil's system settings. Permission is controlled by iOS."
                    )
                    .accessibilityIdentifier("synveil.notifications.system-settings")
                }
            } header: {
                Text("Local Notifications")
            } footer: {
                Text(
                    "Notifications contain generic status text. Enabling them does not enable background sync or start an operation."
                )
            }
            Section("Categories") {
                ForEach(LocalNotificationCategory.allCases, id: \.self) { category in
                    Toggle(
                        category.label,
                        isOn: Binding(
                            get: { coordinator.policy.categories.contains(category) },
                            set: { coordinator.setCategory(category, enabled: $0) })
                    )
                    .disabled(!coordinator.policy.enabled || changingPermission)
                    .accessibilityIdentifier("synveil.notifications.category.\(category.rawValue)")
                }
                Text("Each category starts off and must be enabled separately.")
                    .font(.footnote)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if coordinator.failure == .storageFailure || coordinator.failure == .capacity {
                Section {
                    Text(
                        "Notification presentation is unavailable. Ordinary Synveil operations remain available."
                    )
                    .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .navigationTitle("Notifications")
        .accessibilityIdentifier("synveil.notifications.settings")
        .task { await coordinator.refreshStatus() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await coordinator.refreshStatus() } }
        }
    }
}

import SwiftUI

public struct PhotoBackupSettingsView: View {
    @Bindable public var viewModel: PhotoBackupSettingsViewModel

    public init(viewModel: PhotoBackupSettingsViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        Form {
            Section(
                header: Text("Camera Roll Discovery Opt-In"),
                footer: Text("P056 prepares metadata discovery for camera roll media only. Content upload and automatic transfers remain completely disabled.")
            ) {
                Toggle("Opt-In Camera Roll Discovery", isOn: Binding(
                    get: { viewModel.isOptedIn },
                    set: { newValue in
                        Task { await viewModel.toggleOptIn(newValue) }
                    }
                ))
                .accessibilityIdentifier("photo_backup_opt_in_toggle")

                HStack {
                    Text("Permission Status")
                    Spacer()
                    Text(viewModel.permissionState.rawValue)
                        .foregroundColor(.secondary)
                        .accessibilityIdentifier("photo_backup_permission_status")
                }

                if !viewModel.permissionState.isAuthorized {
                    Button(action: {
                        Task { await viewModel.requestPermission() }
                    }) {
                        Text("Grant Photos Permission")
                    }
                    .accessibilityIdentifier("grant_photos_permission_button")
                }
            }

            if viewModel.isOptedIn {
                Section(header: Text("Discovery Settings")) {
                    Toggle("Include Videos", isOn: Binding(
                        get: { viewModel.includeVideos },
                        set: { newValue in
                            Task { await viewModel.updateIncludeVideos(newValue) }
                        }
                    ))
                    .accessibilityIdentifier("photo_backup_include_videos_toggle")

                    Toggle("Pause Discovery", isOn: Binding(
                        get: { viewModel.pauseDiscovery },
                        set: { newValue in
                            Task { await viewModel.togglePauseDiscovery(newValue) }
                        }
                    ))
                    .accessibilityIdentifier("photo_backup_pause_toggle")
                }

                Section(header: Text("Current Status")) {
                    Text(viewModel.currentStatusText)
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                        .accessibilityIdentifier("photo_backup_status_text")
                }
            }
        }
        .navigationTitle("Photo Backup")
        .task {
            await viewModel.loadSettings()
        }
    }
}

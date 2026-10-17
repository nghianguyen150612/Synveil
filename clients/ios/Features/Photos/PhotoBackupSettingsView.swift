import SwiftUI

struct PhotoBackupSettingsDestination: View {
    let factory: @MainActor () async -> PhotoBackupSettingsViewModel?
    @State private var viewModel: PhotoBackupSettingsViewModel?
    @State private var didLoad = false

    var body: some View {
        Group {
            if let viewModel {
                PhotoBackupSettingsView(viewModel: viewModel)
            } else if didLoad {
                ContentUnavailableView(
                    "Photo Backup Unavailable",
                    systemImage: "photo.on.rectangle.angled",
                    description: Text(
                        "An authenticated session and private local storage are required."))
            } else {
                ProgressView("Loading Photo Backup…")
            }
        }
        .task {
            guard !didLoad else { return }
            viewModel = await factory()
            didLoad = true
        }
    }
}

public struct PhotoBackupSettingsView: View {
    @Bindable public var viewModel: PhotoBackupSettingsViewModel

    public init(viewModel: PhotoBackupSettingsViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        Form {
            Section(
                header: Text("Camera Roll Discovery Opt-In"),
                footer: Text(
                    "Discovery reads media metadata. Preparing a selected original and uploading "
                        + "it are separate user actions. Automatic upload, iCloud retrieval, and "
                        + "background transfer remain off."
                )
            ) {
                Toggle(
                    "Opt-In Camera Roll Discovery",
                    isOn: Binding(
                        get: { viewModel.isOptedIn },
                        set: { newValue in
                            Task { await viewModel.toggleOptIn(newValue) }
                        }
                    )
                )
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
                    Toggle(
                        "Include Videos",
                        isOn: Binding(
                            get: { viewModel.includeVideos },
                            set: { newValue in
                                Task { await viewModel.updateIncludeVideos(newValue) }
                            }
                        )
                    )
                    .accessibilityIdentifier("photo_backup_include_videos_toggle")

                    Toggle(
                        "Pause Discovery",
                        isOn: Binding(
                            get: { viewModel.pauseDiscovery },
                            set: { newValue in
                                Task { await viewModel.togglePauseDiscovery(newValue) }
                            }
                        )
                    )
                    .accessibilityIdentifier("photo_backup_pause_toggle")
                }

                Section(header: Text("Current Status")) {
                    Text(viewModel.currentStatusText)
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                        .accessibilityIdentifier("photo_backup_status_text")
                }

                Section(
                    header: Text("Prepare One Original Resource"),
                    footer: Text(
                        "Original availability is unknown until PhotoKit extraction succeeds. "
                            + "iCloud downloads are disabled; previews are never substituted."
                    )
                ) {
                    Picker(
                        "Observed asset",
                        selection: Binding(
                            get: { viewModel.selectedAssetIdentifier },
                            set: { viewModel.selectAsset($0) })
                    ) {
                        Text("Choose an observed asset").tag(nil as String?)
                        ForEach(Array(viewModel.observations.prefix(100))) { observation in
                            Text(viewModel.assetLabel(observation))
                                .tag(Optional(observation.localIdentifier))
                        }
                    }
                    .accessibilityIdentifier("photo_backup_asset_picker")

                    Button("Load original resource choices") {
                        Task { await viewModel.loadResourceChoices() }
                    }
                    .disabled(viewModel.selectedAssetIdentifier == nil || viewModel.isProcessing)
                    .accessibilityIdentifier("photo_backup_load_resources_button")

                    ForEach(viewModel.resourceChoices) { resource in
                        Button {
                            viewModel.selectResource(resource)
                        } label: {
                            HStack {
                                Text(viewModel.resourceLabel(resource))
                                    .foregroundStyle(.primary)
                                Spacer()
                                if viewModel.selectedResource == resource {
                                    Image(systemName: "checkmark")
                                        .accessibilityHidden(true)
                                }
                            }
                        }
                        .accessibilityIdentifier("photo_backup_resource_choice")
                    }

                    if viewModel.isPreparing {
                        Button("Cancel preparation", role: .cancel) {
                            viewModel.cancelPreparation()
                        }
                        .accessibilityIdentifier("photo_backup_cancel_preparation_button")
                    } else {
                        Button("Prepare selected resource") {
                            Task { await viewModel.prepareSelectedAsset() }
                        }
                        .disabled(!viewModel.canPrepareSelectedResource)
                        .accessibilityIdentifier("photo_backup_prepare_selected_button")
                    }
                }

                if let record = viewModel.extractionRecord {
                    Section(header: Text("Selected Resource Status")) {
                        Text(viewModel.uploadStatusText ?? record.state.rawValue)
                            .font(.subheadline)
                            .accessibilityIdentifier("photo_backup_resource_status")
                        if let reason = viewModel.failureReasonText {
                            Text(reason)
                                .foregroundStyle(.secondary)
                                .accessibilityIdentifier("photo_backup_failure_reason")
                        }
                    }

                    if record.state == .verifiedReady || record.state == .handoffPending {
                        Section(
                            header: Text("Foreground Upload Destination"),
                            footer: Text(
                                "Choose an existing active Library and parent directory. "
                                    + "P045 validates the destination and preserves its retry identity."
                            )
                        ) {
                            TextField("Library ID", text: $viewModel.destinationLibraryIdentifier)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                                .accessibilityIdentifier("photo_backup_destination_library")
                            TextField(
                                "Parent directory ID", text: $viewModel.destinationParentIdentifier)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                                .accessibilityIdentifier("photo_backup_destination_parent")
                            TextField("New filename", text: $viewModel.destinationFilename)
                                .accessibilityIdentifier("photo_backup_destination_filename")
                            Button(record.state == .handoffPending
                                ? "Continue foreground upload handoff"
                                : "Upload selected verified asset") {
                                Task { await viewModel.uploadSelectedVerifiedAsset() }
                            }
                            .disabled(!viewModel.canUploadSelectedResource)
                            .accessibilityIdentifier("photo_backup_upload_selected_button")
                        }
                    }

                    if record.state == .handedOff {
                        Section(header: Text("P045 Foreground Upload")) {
                            Text(
                                "The upload is owned by the existing foreground coordinator. "
                                    + "Verified staging does not mean the server has confirmed upload."
                            )
                            .foregroundStyle(.secondary)
                            if let uploadId = record.relatedUploadOperationId {
                                Text("Upload operation: \(uploadId)")
                                    .font(.caption)
                                    .textSelection(.enabled)
                            }
                            Button("Pause foreground upload") {
                                Task { await viewModel.pauseForegroundUpload() }
                            }
                            .accessibilityIdentifier("photo_backup_pause_upload_button")
                            Button("Resume existing foreground upload") {
                                Task { await viewModel.resumeForegroundUpload() }
                            }
                            .accessibilityIdentifier("photo_backup_resume_upload_button")
                        }
                    }

                    if record.state == .uploadedConfirmed {
                        Section(header: Text("Upload Confirmed")) {
                            Text("The P045 foreground upload was confirmed by the server.")
                                .accessibilityIdentifier("photo_backup_upload_confirmed")
                        }
                    }
                } else if let reason = viewModel.failureReasonText {
                    Section(header: Text("Preparation Status")) {
                        Text(reason)
                            .foregroundStyle(.secondary)
                            .accessibilityIdentifier("photo_backup_failure_reason")
                    }
                }
            }
        }
        .navigationTitle("Photo Backup")
        .task {
            await viewModel.loadSettings()
        }
    }
}

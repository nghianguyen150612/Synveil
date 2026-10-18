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
    @State private var showingAutomaticBackupConfirmation = false

    public init(viewModel: PhotoBackupSettingsViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        Form {
            Section(
                header: Text("Camera Roll Discovery Opt-In"),
                footer: Text(
                    "Discovery reads media metadata. Preparing a selected original and uploading "
                        + "it are separate from discovery. Automatic foreground backup has its "
                        + "own opt-in. iCloud retrieval and background transfer remain off."
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

            Section(
                header: Text("Automatic Foreground Backup"),
                footer: Text(
                    "Automatic uploads run only while Synveil is authenticated and active in the "
                        + "foreground. Background upload and silent iCloud retrieval are disabled."
                )
            ) {
                Toggle(
                    "Automatic foreground backup",
                    isOn: Binding(
                        get: { viewModel.automaticForegroundBackupEnabled },
                        set: { enabled in
                            if enabled {
                                showingAutomaticBackupConfirmation = true
                            } else {
                                Task { await viewModel.disableAutomaticForegroundBackup() }
                            }
                        }
                    )
                )
                .accessibilityIdentifier("photo_backup_automatic_toggle")

                Picker(
                    "Target Library",
                    selection: Binding(
                        get: { viewModel.destinationLibraryIdentifier },
                        set: { identifier in
                            Task { await viewModel.updateTargetLibrary(identifier) }
                        }
                    )
                ) {
                    Text("Choose an active Library").tag("")
                    ForEach(viewModel.availableLibraries, id: \.id.rawValue) { library in
                        Text(library.name).tag(library.id.rawValue)
                    }
                }
                .accessibilityIdentifier("photo_backup_target_library_picker")

                Picker(
                    "Destination directory",
                    selection: Binding(
                        get: { viewModel.destinationParentIdentifier },
                        set: { identifier in Task { await viewModel.updateTargetParent(identifier) }
                        }
                    )
                ) {
                    Text("Choose a destination").tag("")
                    ForEach(viewModel.availableParentChoices) { parent in
                        Text(parent.name).tag(parent.id)
                    }
                }
                .disabled(viewModel.selectedTargetLibrary == nil)
                .accessibilityIdentifier("photo_backup_target_parent_picker")

                Toggle(
                    "Include still photos",
                    isOn: Binding(
                        get: { viewModel.includeStillImages },
                        set: { value in Task { await viewModel.updateIncludeStillImages(value) } }
                    )
                )
                .accessibilityIdentifier("photo_backup_include_stills_toggle")
                Toggle(
                    "Include videos",
                    isOn: Binding(
                        get: { viewModel.includeVideos },
                        set: { value in Task { await viewModel.updateIncludeVideos(value) } }
                    )
                )
                .accessibilityIdentifier("photo_backup_include_videos_auto_toggle")
                Toggle(
                    "Include Live Photos",
                    isOn: Binding(
                        get: { viewModel.includeLivePhotos },
                        set: { value in Task { await viewModel.updateIncludeLivePhotos(value) } }
                    )
                )
                .accessibilityIdentifier("photo_backup_include_live_photos_toggle")

                Picker(
                    "Rendition",
                    selection: Binding(
                        get: { viewModel.renditionPolicy },
                        set: { value in Task { await viewModel.updateRenditionPolicy(value) } }
                    )
                ) {
                    Text("Original only").tag(PhotoBackupRenditionPolicy.originalOnly)
                    Text("Original and edited").tag(PhotoBackupRenditionPolicy.originalAndEdited)
                }
                .accessibilityIdentifier("photo_backup_rendition_picker")

                Picker(
                    "Network",
                    selection: Binding(
                        get: { viewModel.networkRestriction },
                        set: { value in Task { await viewModel.updateNetworkRestriction(value) } }
                    )
                ) {
                    Text("Wi-Fi only").tag(PhotoBackupNetworkRestriction.wifiOnly)
                    Text("Wi-Fi and cellular").tag(PhotoBackupNetworkRestriction.wifiAndCellular)
                }
                .accessibilityIdentifier("photo_backup_network_picker")

                if viewModel.networkRestriction == .wifiAndCellular {
                    Toggle(
                        "Allow cellular transfer",
                        isOn: Binding(
                            get: { viewModel.allowCellularTransfer },
                            set: { value in Task { await viewModel.updateCellularPermission(value) }
                            }
                        )
                    )
                    .accessibilityIdentifier("photo_backup_cellular_toggle")
                }
                Toggle(
                    "Pause in Low Data Mode",
                    isOn: Binding(
                        get: { viewModel.respectLowDataMode },
                        set: { value in Task { await viewModel.updateLowDataModePreference(value) }
                        }
                    )
                )
                .accessibilityIdentifier("photo_backup_low_data_toggle")
                Picker(
                    "Maximum resource size",
                    selection: Binding(
                        get: { viewModel.maximumResourceSizeBytes },
                        set: { value in Task { await viewModel.updateMaximumResourceSize(value) } }
                    )
                ) {
                    Text("128 MB").tag(UInt64(134_217_728))
                    Text("512 MB").tag(UInt64(536_870_912))
                    Text("1 GB").tag(UInt64(1_073_741_824))
                }
                .accessibilityIdentifier("photo_backup_maximum_resource_size_picker")

                Toggle(
                    "Pause automatic backup",
                    isOn: Binding(
                        get: { viewModel.pauseAutomaticBackup },
                        set: { value in Task { await viewModel.togglePauseAutomaticBackup(value) } }
                    )
                )
                .disabled(!viewModel.automaticBackupConsentConfirmed)
                .accessibilityIdentifier("photo_backup_pause_automatic_toggle")

                if !viewModel.availableLibraries.isEmpty,
                    viewModel.selectedTargetLibrary == nil
                {
                    Text("Select an active Library and destination before enabling uploads.")
                        .foregroundStyle(.secondary)
                } else if viewModel.availableLibraries.isEmpty {
                    Text("No active Synveil Library is available for this account.")
                        .foregroundStyle(.secondary)
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
                    header: Text("Backup Activity"),
                    footer: Text(
                        "A canonical file upload is not proof of PhotoAsset binding or retained "
                            + "backup. Remote deletion is never inferred from PhotoKit changes."
                    )
                ) {
                    LabeledContent(
                        "Eligible", value: "\(viewModel.queueSummary.count(for: .eligible))")
                    LabeledContent("Queued", value: "\(viewModel.queueSummary.queuedCount)")
                    LabeledContent("Extracting", value: "\(viewModel.queueSummary.extractingCount)")
                    LabeledContent("Uploading", value: "\(viewModel.queueSummary.uploadingCount)")
                    LabeledContent(
                        "Files uploaded to Synveil",
                        value: "\(viewModel.queueSummary.canonicalFileUploadedCount)")
                    LabeledContent(
                        "Awaiting PhotoAsset binding",
                        value: "\(viewModel.queueSummary.uploadedAwaitingBindingCount)")
                    LabeledContent(
                        "Backup confirmed", value: "\(viewModel.queueSummary.backupConfirmedCount)")
                    LabeledContent("Partial", value: "\(viewModel.queueSummary.partialCount)")
                    LabeledContent("Failed", value: "\(viewModel.queueSummary.failedCount)")
                    LabeledContent(
                        "Source unavailable", value: "\(viewModel.queueSummary.unavailableCount)")
                    Text("Worker: \(viewModel.automaticWorkerStatusText)")
                        .font(.caption)
                    Text("PhotoAsset binding: \(viewModel.photoBindingAvailability.rawValue)")
                        .font(.caption)
                    Text("Backup retention: \(viewModel.photoRetentionAvailability.rawValue)")
                        .font(.caption)
                    if !viewModel.livePhotoGroups.isEmpty {
                        Text("Live Photo groups")
                            .font(.headline)
                        ForEach(viewModel.livePhotoGroups) { group in
                            VStack(alignment: .leading) {
                                Text(group.state.rawValue)
                                Text(
                                    "Still: \(group.stillState?.rawValue ?? "pending") · Motion: "
                                        + "\(group.motionState?.rawValue ?? "pending")"
                                )
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            }
                            .accessibilityElement(children: .combine)
                        }
                    }
                    Button("Refresh backup activity") {
                        Task { await viewModel.refreshQueueSummary() }
                    }
                    .accessibilityIdentifier("photo_backup_refresh_activity_button")
                    if viewModel.automaticForegroundBackupEnabled,
                        viewModel.automaticBackupConsentConfirmed,
                        !viewModel.pauseAutomaticBackup
                    {
                        Button("Run one foreground backup pass") {
                            Task { await viewModel.runAutomaticBackupPass() }
                        }
                        .accessibilityIdentifier("photo_backup_run_once_button")
                    }
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
                                "Parent directory ID", text: $viewModel.destinationParentIdentifier
                            )
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("photo_backup_destination_parent")
                            TextField("New filename", text: $viewModel.destinationFilename)
                                .accessibilityIdentifier("photo_backup_destination_filename")
                            Button(
                                record.state == .handoffPending
                                    ? "Continue foreground upload handoff"
                                    : "Upload selected verified asset"
                            ) {
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
        .confirmationDialog(
            "Enable automatic foreground backup?",
            isPresented: $showingAutomaticBackupConfirmation,
            titleVisibility: .visible
        ) {
            Button("Enable Automatic Backup") {
                Task { await viewModel.enableAutomaticForegroundBackupAfterConfirmation() }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(viewModel.automaticBackupConfirmationDescription)
        }
        .task {
            await viewModel.loadSettings()
        }
    }
}

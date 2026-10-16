import SwiftUI
import UniformTypeIdentifiers

struct FileUploadControls: View {
    private let coordinator: FileUploadCoordinator?
    private let controller: SessionController
    private let library: LibraryId
    private let libraryStatus: LibraryStatus
    private let createParent: NodeId?
    private let replaceNode: Node?
    private let onCommitted: (() -> Void)?

    @State private var model: FileUploadViewModel
    @State private var pickerTarget: UploadPickerTarget?
    @State private var pendingSelection: PendingUploadSelection?
    @State private var isShowingImporter = false
    @State private var pendingAbort: UploadRecord?
    @Environment(\.scenePhase) private var scenePhase

    init(
        coordinator: FileUploadCoordinator?, controller: SessionController,
        library: LibraryId, libraryStatus: LibraryStatus,
        createParent: NodeId? = nil, replaceNode: Node? = nil,
        onCommitted: (() -> Void)? = nil
    ) {
        self.coordinator = coordinator
        self.controller = controller
        self.library = library
        self.libraryStatus = libraryStatus
        self.createParent = createParent
        self.replaceNode = replaceNode
        self.onCommitted = onCommitted
        _model = State(
            initialValue: FileUploadViewModel(
                coordinator: coordinator, controller: controller,
                library: library, libraryStatus: libraryStatus))
    }

    var body: some View {
        Group {
            if model.isCurrent {
                uploadActions
                savedUploads
            }
        }
        .fileImporter(
            isPresented: $isShowingImporter,
            allowedContentTypes: [.data],
            allowsMultipleSelection: false,
            onCompletion: handleImport
        )
        .sheet(item: $pendingSelection) { selection in
            NavigationStack {
                UploadConfirmationView(
                    selection: selection,
                    onCancel: { pendingSelection = nil },
                    onConfirm: { logicalName, backgroundEnabled in
                        pendingSelection = nil
                        let target = selection.target.makeUploadTarget(
                            library: library, name: logicalName)
                        Task {
                            let result = await model.start(
                                selectedURL: selection.url, target: target,
                                backgroundEnabled: backgroundEnabled)
                            if case .completed = result { onCommitted?() }
                        }
                    }
                )
            }
            .presentationDetents([.medium, .large])
        }
        .confirmationDialog(
            "Abort this upload?",
            isPresented: Binding(
                get: { pendingAbort != nil },
                set: { if !$0 { pendingAbort = nil } }),
            titleVisibility: .visible
        ) {
            Button("Abort Upload", role: .destructive) {
                guard let record = pendingAbort else { return }
                pendingAbort = nil
                Task {
                    let result = await model.abort(record)
                    if case .success(let updated) = result, updated.state == .committed {
                        onCommitted?()
                    }
                }
            }
            .accessibilityIdentifier("synveil.upload.abort.confirm")
            Button("Keep Upload", role: .cancel) { pendingAbort = nil }
                .accessibilityIdentifier("synveil.upload.abort.cancel")
        } message: {
            Text(
                "This asks the server to mark the upload session as aborted. Pause stops local transfer and keeps the upload resumable."
            )
        }
        .task { await model.loadSaved() }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await model.loadSaved() } }
        }
        .onChange(of: coordinator?.background?.stateRevision ?? 0) { _, _ in
            Task { await model.refreshBackgroundState() }
        }
        .onChange(of: controller.lifecycleRevision) { _, _ in
            model.invalidate()
            pendingSelection = nil
            pendingAbort = nil
        }
        .onChange(of: controller.state) { _, state in
            if state != .authenticated {
                model.invalidate()
                pendingSelection = nil
                pendingAbort = nil
            }
        }
    }

    @ViewBuilder
    private var uploadActions: some View {
        Section("Upload") {
            if coordinator == nil || libraryStatus != .active {
                Label(model.unavailableMessage, systemImage: "lock")
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.upload.unavailable")
            } else {
                if let createParent {
                    Button {
                        pickerTarget = .create(parent: createParent)
                        isShowingImporter = true
                    } label: {
                        Label("Upload File", systemImage: "arrow.up.doc")
                    }
                    .disabled(model.isRunning || model.isLoadingSaved || !model.isAvailable)
                    .accessibilityHint(
                        "Choose one file, confirm its destination name, and upload it into this folder."
                    )
                    .accessibilityIdentifier("synveil.upload.create-file")
                }
                if let node = replaceNode, node.kind == .file, node.state == .active,
                    node.trashedAt == nil, node.restoreDeadline == nil, !node.purgeEligible
                {
                    Button(role: .destructive) {
                        pickerTarget = .replace(node: node.id, revision: node.revision)
                        isShowingImporter = true
                    } label: {
                        Label("Replace File Content", systemImage: "arrow.triangle.2.circlepath")
                    }
                    .disabled(model.isRunning || model.isLoadingSaved || !model.isAvailable)
                    .accessibilityHint(
                        "Creates a new immutable file version using revision \(node.revision.rawValue). Previous versions remain available."
                    )
                    .accessibilityIdentifier("synveil.upload.replace-content")
                }
            }

            if let message = model.displayedActivityMessage(isSceneActive: scenePhase == .active) {
                ProgressView(message)
                    .accessibilityLabel(message)
                    .accessibilityIdentifier("synveil.upload.source-progress")
            } else if let progress = model.liveProgress {
                liveProgress(progress)
            }
            if model.isRunning {
                Button("Pause", role: .cancel) { Task { await model.pause() } }
                    .accessibilityHint(
                        "Stops this foreground transfer and preserves its saved source and server identity."
                    )
                    .accessibilityIdentifier("synveil.upload.pause")
            }
            if let failure = model.failure {
                Text(failure.message)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.upload.failure")
            }
            if model.savedStatusUnavailable {
                Text(
                    "Saved upload status could not be read. Resume is unavailable until local operation and background ownership evidence can be verified."
                )
                .fixedSize(horizontal: false, vertical: true)
                Button("Reload Saved Upload Status") { Task { await model.loadSaved() } }
                    .disabled(model.isLoadingSaved || model.isRunning)
                    .accessibilityIdentifier("synveil.upload.reload-saved-status")
            }
        }
    }

    @ViewBuilder
    private var savedUploads: some View {
        if model.isLoadingSaved {
            Section("Uploads") { ProgressView("Loading saved uploads…") }
        } else if !model.records.isEmpty {
            Section("Recent Uploads") {
                ForEach(model.records, id: \.id) { record in
                    uploadRow(record)
                }
            }
            .accessibilityIdentifier("synveil.upload.list")
        }
    }

    private func uploadRow(_ record: UploadRecord) -> some View {
        let progress = model.displayedProgress(for: record)
        return VStack(alignment: .leading, spacing: 10) {
            Text(uploadTitle(record))
                .font(.headline)
                .fixedSize(horizontal: false, vertical: true)
            if ![.committed, .aborted, .expired].contains(record.state) {
                Text(model.backgroundJobs[record.id]?.state.label ?? "Foreground only")
                    .accessibilityIdentifier(
                        "synveil.upload.background.state.\(record.id.rawValue)")
            }
            Text(
                model.statusLabel(
                    for: record, background: model.backgroundJobs[record.id],
                    isSceneActive: scenePhase == .active))
                .accessibilityAddTraits(
                    model.isLiveOperation(isSceneActive: scenePhase == .active)
                        ? .updatesFrequently : [])
                .accessibilityIdentifier("synveil.upload.state.\(record.id.rawValue)")
            if progress.totalBytes > 0 {
                ProgressView(
                    value: Double(progress.acknowledgedBytes),
                    total: Double(progress.totalBytes)
                )
                .accessibilityLabel("Server-confirmed upload progress")
                .accessibilityValue(
                    "\(progress.acknowledgedBytes) of \(progress.totalBytes) bytes accepted by the server"
                )
            }
            Text(
                "\(progress.acknowledgedBytes) of \(progress.totalBytes) bytes confirmed by the server"
            )
            .font(.footnote)
            if let recovery = ClientRecoveryPresentation.upload(
                record, background: model.backgroundJobs[record.id])
            {
                ClientRecoveryNotice(presentation: recovery)
            }
            if let job = model.backgroundJobs[record.id], job.authorized, job.blocksForeground {
                Button("Pause Background Transfer") { Task { await model.pauseBackground(record) } }
                    .accessibilityIdentifier(
                        "synveil.upload.background.pause.\(record.id.rawValue)")
            } else if model.canResume(record),
                record.authoritativeOffset < record.intent.expected.byteLength,
                coordinator?.background != nil
            {
                Button("Background Transfer Unavailable") {}
                    .disabled(true)
                    .accessibilityHint(BackgroundUploadCapability.current.userMessage)
                    .accessibilityIdentifier(
                        "synveil.upload.background.enable.\(record.id.rawValue)")
                Text(BackgroundUploadCapability.current.userMessage)
                    .font(.footnote)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.upload.background.unavailable")
            }
            HStack {
                if record.state.canResume {
                    Button(record.state == .readyToComplete ? "Resume and Complete" : "Resume") {
                        Task {
                            let result = await model.resume(record)
                            if case .completed = result { onCommitted?() }
                        }
                    }
                    .disabled(!model.canResume(record))
                    .accessibilityIdentifier("synveil.upload.resume.\(record.id.rawValue)")
                }
                if record.serverSessionId != nil, record.state != .committed,
                    record.state != .aborted, record.state != .expired
                {
                    Button("Check Server Status") {
                        Task {
                            let result = await model.checkServerStatus(record)
                            if case .success(let updated) = result,
                                updated.state == .committed
                            {
                                onCommitted?()
                            }
                        }
                    }
                    .disabled(
                        model.isRunning || model.isLoadingSaved || model.savedStatusUnavailable
                            || !model.canInspectSaved
                    )
                    .accessibilityIdentifier("synveil.upload.status.\(record.id.rawValue)")
                }
                if mayRequestAbort(record) {
                    Button("Abort Upload", role: .destructive) { pendingAbort = record }
                        .disabled(
                            model.isRunning || model.isLoadingSaved || model.savedStatusUnavailable
                                || !model.canInspectSaved
                        )
                        .accessibilityIdentifier("synveil.upload.abort.\(record.id.rawValue)")
                }
            }
            .buttonStyle(.bordered)
            .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, 4)
    }

    @ViewBuilder
    private func liveProgress(_ progress: UploadProgress) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(
                model.isRunning && !model.isLiveOperation(isSceneActive: scenePhase == .active)
                    ? "Checking saved transfer status…" : progress.state.label
            )
                .font(.headline)
                .accessibilityAddTraits(
                    model.isLiveOperation(isSceneActive: scenePhase == .active)
                        ? .updatesFrequently : [])
            if progress.totalBytes > 0 {
                ProgressView(
                    value: Double(progress.acknowledgedBytes), total: Double(progress.totalBytes)
                )
                .accessibilityLabel("Server-confirmed upload progress")
                .accessibilityValue(
                    "\(progress.acknowledgedBytes) of \(progress.totalBytes) bytes accepted by the server"
                )
            }
            Text("\(progress.acknowledgedBytes) of \(progress.totalBytes) bytes confirmed")
                .font(.footnote)
        }
    }

    private func uploadTitle(_ record: UploadRecord) -> String {
        switch record.intent.target {
        case .createFile(_, _, let name): name
        case .replaceContent: "Replace file content"
        }
    }

    private func mayRequestAbort(_ record: UploadRecord) -> Bool {
        guard record.serverSessionId != nil, record.state != .committed,
            record.state != .aborted, record.state != .expired,
            record.serverState == .open || record.state == .abortPending
        else { return false }
        return record.state.canResume || record.state == .abortPending
            || record.state == .failedPermanent
    }

    private func handleImport(_ result: Result<[URL], Error>) {
        switch result {
        case .success(let urls):
            guard urls.count == 1, let url = urls.first, let pickerTarget else {
                model.reportImportFailure()
                self.pickerTarget = nil
                return
            }
            pendingSelection = PendingUploadSelection(url: url, target: pickerTarget)
            self.pickerTarget = nil
        case .failure(let error):
            if (error as NSError).code != NSUserCancelledError { model.reportImportFailure() }
            pickerTarget = nil
        }
    }
}

private enum UploadPickerTarget: Equatable {
    case create(parent: NodeId)
    case replace(node: NodeId, revision: NodeRevision)

    func makeUploadTarget(library: LibraryId, name: String) -> UploadTarget {
        switch self {
        case .create(let parent):
            .createFile(library: library, parent: parent, name: name)
        case .replace(let node, let revision):
            .replaceContent(library: library, node: node, expectedRevision: revision)
        }
    }
}

private struct PendingUploadSelection: Identifiable {
    let id = UUID()
    let url: URL
    let target: UploadPickerTarget

    init(url: URL, target: UploadPickerTarget) {
        self.url = url
        self.target = target
    }
}

private struct UploadConfirmationView: View {
    let selection: PendingUploadSelection
    let onCancel: () -> Void
    let onConfirm: (String, Bool) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var backgroundEnabled = false
    @State private var logicalName: String

    init(
        selection: PendingUploadSelection, onCancel: @escaping () -> Void,
        onConfirm: @escaping (String, Bool) -> Void
    ) {
        self.selection = selection
        self.onCancel = onCancel
        self.onConfirm = onConfirm
        _logicalName = State(initialValue: selection.url.lastPathComponent)
    }

    var body: some View {
        Form {
            Section("Selected File") {
                Text(selection.url.lastPathComponent)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                switch selection.target {
                case .create:
                    TextField("File name", text: $logicalName)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityLabel("Destination file name")
                        .accessibilityIdentifier("synveil.upload.confirm.filename")
                case .replace(_, let revision):
                    Text("Expected file revision: \(revision.rawValue)")
                        .font(.footnote.monospaced())
                    Text(
                        "A new immutable FileVersion will be created. Previous versions remain available."
                    )
                    .fixedSize(horizontal: false, vertical: true)
                }
            }
            Section("Transfer Execution") {
                Toggle("Allow Background Transfer for This File", isOn: $backgroundEnabled)
                    .disabled(!BackgroundUploadCapability.current.isAvailable)
                    .accessibilityHint(BackgroundUploadCapability.current.userMessage)
                    .accessibilityIdentifier("synveil.upload.background.authorization")
                Text(BackgroundUploadCapability.current.userMessage)
                    .font(.footnote)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Section {
                Button(actionTitle) {
                    onConfirm(logicalName, backgroundEnabled)
                    dismiss()
                }
                .disabled(selection.target.isCreate && logicalName.isEmpty)
                .accessibilityIdentifier("synveil.upload.confirm.submit")
            } footer: {
                Text(
                    "Synveil copies this file into private staging, calculates its exact length and SHA-256, then sends it only after this confirmation."
                )
            }
        }
        .navigationTitle(selection.target.isCreate ? "Upload File" : "Replace Content")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") {
                    onCancel()
                    dismiss()
                }
            }
        }
        .accessibilityIdentifier("synveil.upload.confirmation")
    }

    private var actionTitle: String {
        selection.target.isCreate ? "Upload File" : "Replace File Content"
    }
}

private extension UploadPickerTarget {
    var isCreate: Bool {
        if case .create = self { return true }
        return false
    }
}

import SwiftUI

struct MutationActivityView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var viewModel: MutationActivityViewModel
    @State private var unknownConfirmationId: String?
    @State private var restoreConfirmationId: String?
    private let library: MetadataMutationLibraryContext
    private let sessionController: SessionController
    private let onConfirmedMutation: () -> Void

    init(
        library: MetadataMutationLibraryContext,
        feature: any MetadataMutationFeatureProtocol,
        sessionController: SessionController,
        onConfirmedMutation: @escaping () -> Void = {}
    ) {
        self.library = library
        self.sessionController = sessionController
        self.onConfirmedMutation = onConfirmedMutation
        _viewModel = State(
            initialValue: MutationActivityViewModel(
                library: library, feature: feature, sessionController: sessionController))
    }

    var body: some View {
        List {
            Section {
                Text(
                    "Queued changes have not changed server metadata. Applied appears only after the server result is verified and saved on this device."
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.mutation.activity.explanation")
                Text(
                    "Showing up to 100 operations. Unresolved changes are prioritized; older terminal history may be omitted."
                )
                .font(.footnote)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.mutation.activity.limit")
                if library.status == .active,
                    viewModel.state == .idle || viewModel.state == .loading
                {
                    ProgressView("Checking durable synchronization state…")
                        .accessibilityLabel("Checking synchronization state")
                        .accessibilityIdentifier("synveil.mutation.activity.checkpoint-progress")
                } else if library.status == .active, viewModel.availability == .ready {
                    Button {
                        Task { await viewModel.sendPendingChanges() }
                    } label: {
                        if viewModel.isSending {
                            Label("Sending Pending Changes…", systemImage: "arrow.up.circle")
                        } else {
                            Label("Send Pending Changes", systemImage: "paperplane")
                        }
                    }
                    .disabled(
                        viewModel.isSending || viewModel.isReconciling || viewModel.isRestoring
                    )
                    .accessibilityHint(
                        "Submits a bounded batch of queued changes using the authenticated mutation coordinator."
                    )
                    .accessibilityIdentifier("synveil.mutation.send-pending")
                } else if library.status == .active,
                    viewModel.availability == .setupRequired
                {
                    Text(
                        "A first-time server connection establishes synchronization state. It does not send a metadata change."
                    )
                    .fixedSize(horizontal: false, vertical: true)
                    Button {
                        Task { await viewModel.enableChanges() }
                    } label: {
                        if viewModel.isPreparingChanges {
                            ProgressView("Preparing synchronization…")
                        } else {
                            Label(
                                "Enable Changes for This Library",
                                systemImage: "arrow.triangle.2.circlepath")
                        }
                    }
                    .disabled(viewModel.isPreparingChanges)
                    .accessibilityIdentifier("synveil.mutation.activity.enable-changes")
                } else if library.status == .active,
                    case .unavailable(let reason) = viewModel.availability
                {
                    Text(MetadataMutationViewModel.message(for: reason))
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("synveil.mutation.activity.editing-unavailable")
                } else {
                    Text(
                        library.status == .readOnly
                            ? "This Library is read-only. Pending changes cannot be sent."
                            : "This Library is quarantined. Pending changes cannot be sent."
                    )
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.mutation.activity.editing-unavailable")
                }
                if viewModel.isSending {
                    ProgressView("Sending a bounded batch…")
                        .accessibilityLabel("Sending pending changes")
                        .accessibilityIdentifier("synveil.mutation.send-progress")
                }
                if let notice = viewModel.notice {
                    MutationNoticeView(
                        notice: notice, identifier: "synveil.mutation.activity.notice")
                }
            } header: {
                Text(library.name)
            }

            activityContents
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Pending Changes")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button("Done") { dismiss() }
                    .accessibilityIdentifier("synveil.mutation.activity.done")
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task { await viewModel.load() }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .disabled(
                    viewModel.state == .loading || viewModel.isSending
                        || viewModel.isReconciling || viewModel.isRestoring
                        || viewModel.isPreparingChanges
                )
                .accessibilityLabel("Refresh Pending Changes")
                .accessibilityHint("Reads durable queue status. It does not send changes.")
                .accessibilityIdentifier("synveil.mutation.activity.refresh")
            }
        }
        .refreshable { await viewModel.load() }
        .task { await viewModel.load() }
        .onChange(of: sessionController.state) { _, _ in viewModel.sessionDidChange() }
        .onChange(of: sessionController.lifecycleRevision) { _, _ in viewModel.sessionDidChange() }
        .onChange(of: viewModel.dataRefreshGeneration) { _, _ in onConfirmedMutation() }
        .onDisappear { viewModel.invalidate() }
        .confirmationDialog(
            "Check / Retry Original Operation",
            isPresented: Binding(
                get: { unknownConfirmationId != nil },
                set: { if !$0 { unknownConfirmationId = nil } }),
            titleVisibility: .visible
        ) {
            Button("Retry Original Operation", role: .destructive) {
                guard let id = unknownConfirmationId else { return }
                unknownConfirmationId = nil
                Task { await viewModel.reconcileUnknown(mutationId: id) }
            }
            .accessibilityIdentifier("synveil.mutation.unknown.confirm")
            Button("Cancel", role: .cancel) { unknownConfirmationId = nil }
                .accessibilityIdentifier("synveil.mutation.unknown.cancel")
        } message: {
            Text(
                "The server may already have processed this change. Synveil will check the original operation with the same ID and unchanged request. No replacement change will be created."
            )
        }
        .confirmationDialog(
            "Restore this item?",
            isPresented: Binding(
                get: { restoreConfirmationId != nil },
                set: { if !$0 { restoreConfirmationId = nil } }),
            titleVisibility: .visible
        ) {
            Button("Queue Restore", role: .destructive) {
                guard let id = restoreConfirmationId else { return }
                restoreConfirmationId = nil
                Task { await viewModel.enqueueRestore(operationId: id) }
            }
            .accessibilityIdentifier("synveil.mutation.restore.confirm")
            Button("Cancel", role: .cancel) { restoreConfirmationId = nil }
                .accessibilityIdentifier("synveil.mutation.restore.cancel")
        } message: {
            Text(
                "Restore uses the previously applied trash result and verifies the original active parent’s current revision. The item changes only after you send this queued operation and the server confirms it."
            )
        }
        .accessibilityIdentifier("synveil.mutation.activity")
    }

    @ViewBuilder
    private var activityContents: some View {
        switch viewModel.state {
        case .idle, .loading:
            Section {
                ProgressView("Loading durable change status…")
                    .accessibilityLabel("Loading Pending Changes")
                    .accessibilityIdentifier("synveil.mutation.activity.loading")
            }
        case .loaded(let items):
            if items.isEmpty {
                Section {
                    Label("No saved changes", systemImage: "tray")
                        .accessibilityIdentifier("synveil.mutation.activity.empty")
                }
            } else {
                Section("Operations") {
                    ForEach(items) { item in
                        MutationActivityRow(
                            item: item,
                            libraryIsWritable: library.status == .active,
                            isRetrying: viewModel.isReconciling || viewModel.isSending
                                || viewModel.isPreparingChanges,
                            isCheckingRestore: viewModel.isRestoring,
                            restoreReady: viewModel.restoreReadyIDs.contains(item.id),
                            checkUnknown: { unknownConfirmationId = item.id },
                            checkRestore: {
                                Task { _ = await viewModel.checkRestore(operationId: item.id) }
                            },
                            confirmRestore: { restoreConfirmationId = item.id }
                        )
                    }
                }
            }
        case .failed(let failure):
            Section {
                MutationNoticeView(
                    notice: MetadataMutationNotice(
                        title: "Pending Changes unavailable",
                        message: MetadataMutationViewModel.message(for: failure),
                        symbol: "externaldrive.badge.exclamationmark", isError: true),
                    identifier: "synveil.mutation.activity.error")
                Button("Try Again") { Task { await viewModel.load() } }
                    .accessibilityIdentifier("synveil.mutation.activity.retry-load")
            }
        case .invalidated:
            EmptyView()
        }
    }
}

private struct MutationActivityRow: View {
    let item: MetadataMutationActivityItem
    let libraryIsWritable: Bool
    let isRetrying: Bool
    let isCheckingRestore: Bool
    let restoreReady: Bool
    let checkUnknown: () -> Void
    let checkRestore: () -> Void
    let confirmRestore: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Label(stateLabel, systemImage: stateSymbol)
                .font(.headline)
                .accessibilityIdentifier("synveil.mutation.state.\(item.id)")
            Text(kindLabel)
                .font(.subheadline.weight(.semibold))
            Text(item.targetLabel)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
                .accessibilityLabel("Target: \(item.targetLabel)")
            Text("Queued \(item.enqueuedAt.formatted(date: .abbreviated, time: .shortened))")
                .font(.footnote)
                .foregroundStyle(.secondary)
            if let attempted = item.lastAttemptAt {
                Text("Last attempt \(attempted.formatted(date: .abbreviated, time: .shortened))")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            if item.state == .conflict {
                Text(MutationConflictPresentation.message(for: item.conflictReason))
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.mutation.conflict.\(item.id)")
            }
            if item.state == .outcomeUnknown {
                Text(
                    "The server may already have processed this change, but its result could not be confirmed."
                )
                .fixedSize(horizontal: false, vertical: true)
                if let count = item.recoveryAttemptCount,
                    count < MutationQueuePolicy.maximumRecoveryAttempts,
                    libraryIsWritable
                {
                    Button("Check Unknown Outcome", action: checkUnknown)
                        .disabled(isRetrying || isCheckingRestore)
                        .accessibilityHint(
                            "After confirmation, checks the original operation with its same ID and request."
                        )
                        .accessibilityIdentifier("synveil.mutation.unknown.check.\(item.id)")
                } else if let count = item.recoveryAttemptCount {
                    Text(
                        "Recovery attempts used: \(count) of \(MutationQueuePolicy.maximumRecoveryAttempts). Further retries are disabled."
                    )
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.mutation.unknown.limit.\(item.id)")
                } else {
                    Text("Retry is disabled until Synveil can verify the recovery limit.")
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if item.state == .blockedRebaseline {
                Text(
                    "Synchronization state must be reconciled before changes can be sent. The original operation is preserved."
                )
                .fixedSize(horizontal: false, vertical: true)
            }
            if item.state == .failedPermanent {
                Text(
                    "This terminal rejection cannot be retried unconditionally. The operation history is preserved."
                )
                .fixedSize(horizontal: false, vertical: true)
            }
            if item.mayCheckRestore && libraryIsWritable {
                if restoreReady {
                    Button("Restore", action: confirmRestore)
                        .disabled(isCheckingRestore || isRetrying)
                        .accessibilityHint(
                            "Queues a restore after checking the original active parent."
                        )
                        .accessibilityIdentifier("synveil.mutation.restore.\(item.id)")
                } else {
                    Button("Check Restore Preconditions", action: checkRestore)
                        .disabled(isCheckingRestore || isRetrying)
                        .accessibilityHint(
                            "Reads the original parent metadata. This does not queue or send a change."
                        )
                        .accessibilityIdentifier("synveil.mutation.restore.check.\(item.id)")
                }
            }
        }
        .padding(.vertical, 6)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("synveil.mutation.row.\(item.id)")
    }

    private var kindLabel: String {
        switch item.kind {
        case .createDirectory: "Create Folder"
        case .renameNode: "Rename"
        case .moveNode: "Move"
        case .trashNode: "Move to Trash"
        case .restoreNode: "Restore"
        }
    }

    private var stateLabel: String {
        MutationQueueStatePresentation.label(for: item.state)
    }

    private var stateSymbol: String {
        MutationQueueStatePresentation.symbol(for: item.state)
    }
}

struct MetadataNameEditorView: View {
    @Environment(\.dismiss) private var dismiss
    @FocusState private var nameFieldFocused: Bool
    @State private var name: String
    @State private var saving = false
    @State private var saveStatusMessage: String?
    let title: String
    let fieldLabel: String
    let helpText: String
    let onSave: (String) async -> Bool

    init(
        title: String,
        initialName: String,
        fieldLabel: String,
        helpText: String,
        onSave: @escaping (String) async -> Bool
    ) {
        self.title = title
        self.fieldLabel = fieldLabel
        self.helpText = helpText
        self.onSave = onSave
        _name = State(initialValue: initialName)
    }

    var body: some View {
        Form {
            Section {
                TextField(fieldLabel, text: $name, axis: .vertical)
                    .lineLimit(1...4)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .focused($nameFieldFocused)
                    .accessibilityIdentifier("synveil.mutation.name-input")
                Text(helpText)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if let saveStatusMessage {
                    Label(saveStatusMessage, systemImage: "exclamationmark.triangle")
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("synveil.mutation.name-save-status")
                }
            }
        }
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
                    .disabled(saving)
                    .accessibilityIdentifier("synveil.mutation.name-cancel")
            }
            ToolbarItem(placement: .confirmationAction) {
                Button {
                    guard !saving, !name.isEmpty else { return }
                    saving = true
                    Task {
                        if await onSave(name) {
                            dismiss()
                        } else {
                            saveStatusMessage =
                                "The save did not return a verified durable status. Close this form to review Pending Changes before trying again."
                        }
                        saving = false
                    }
                } label: {
                    if saving { ProgressView() } else { Text("Save") }
                }
                .disabled(saving || name.isEmpty)
                .accessibilityLabel(saving ? "Saving change" : "Save change")
                .accessibilityIdentifier("synveil.mutation.name-save")
            }
        }
        .task { nameFieldFocused = true }
        .interactiveDismissDisabled(saving)
        .accessibilityIdentifier("synveil.mutation.name-editor")
    }
}

private struct MovePickerRoute: Hashable {
    let directoryId: NodeId
    let title: String
    let ancestry: [NodeId]
}

private enum MovePickerLoadState: Equatable {
    case loading
    case loaded([Node])
    case failed(String)
}

struct MoveDestinationPickerView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var path: [MovePickerRoute]
    @State private var loadStates: [NodeId: MovePickerLoadState] = [:]
    @State private var verifiedNodes: [NodeId: Node] = [:]
    private let repository: (any NodeRepositoryProtocol)?
    private let sessionController: SessionController
    private let library: MetadataMutationLibraryContext
    private let source: Node
    private let sessionRevision: UInt64
    private let initialRoute: MovePickerRoute
    private let onSelect: (Node, [NodeId]) -> Void

    init(
        repository: (any NodeRepositoryProtocol)?,
        sessionController: SessionController,
        library: MetadataMutationLibraryContext,
        source: Node,
        initialDirectoryId: NodeId,
        initialDirectoryTitle: String,
        initialAncestry: [NodeId],
        initialParentSnapshot: Node?,
        initialChildren: [Node],
        onSelect: @escaping (Node, [NodeId]) -> Void
    ) {
        self.repository = repository
        self.sessionController = sessionController
        self.library = library
        self.source = source
        sessionRevision = sessionController.lifecycleRevision
        let route = MovePickerRoute(
            directoryId: initialDirectoryId, title: initialDirectoryTitle,
            ancestry: initialAncestry)
        initialRoute = route
        self.onSelect = onSelect
        _path = State(initialValue: [])
        var nodes: [NodeId: Node] = [:]
        if let initialParentSnapshot { nodes[initialParentSnapshot.id] = initialParentSnapshot }
        for node in initialChildren { nodes[node.id] = node }
        _verifiedNodes = State(initialValue: nodes)
        _loadStates = State(initialValue: [initialDirectoryId: .loaded(initialChildren)])
    }

    var body: some View {
        NavigationStack(path: $path) {
            directoryList(for: initialRoute)
                .navigationDestination(for: MovePickerRoute.self) { route in
                    directoryList(for: route)
                }
        }
        .navigationTitle("Move")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
                    .accessibilityIdentifier("synveil.move.cancel")
            }
        }
        .onChange(of: sessionController.state) { _, _ in invalidateIfNeeded() }
        .onChange(of: sessionController.lifecycleRevision) { _, _ in invalidateIfNeeded() }
        .accessibilityIdentifier("synveil.move.destination-picker")
    }

    @ViewBuilder
    private func directoryList(for route: MovePickerRoute) -> some View {
        List {
            Section {
                if route.directoryId == library.rootNodeId,
                    let root = verifiedNodes[library.rootNodeId]
                {
                    destinationButton(root, ancestry: route.ancestry)
                } else if route.directoryId == library.rootNodeId {
                    Button("Check Library root revision") {
                        Task { await loadRootMetadata() }
                    }
                    .accessibilityHint(
                        "Reads authoritative root metadata before it can be selected as a destination."
                    )
                    .accessibilityIdentifier("synveil.move.check-root-revision")
                }
                if route.directoryId != library.rootNodeId {
                    NavigationLink(
                        value: MovePickerRoute(
                            directoryId: library.rootNodeId, title: "Library root",
                            ancestry: [library.rootNodeId])
                    ) {
                        Label("Browse Library root", systemImage: "arrow.up.to.line")
                    }
                    .accessibilityIdentifier("synveil.move.browse-root")
                }
            } header: {
                Text("Choose this folder as the destination")
            }
            switch loadStates[route.directoryId] {
            case .loading, nil:
                Section {
                    ProgressView("Loading folders…")
                        .accessibilityLabel("Loading destination folders")
                        .accessibilityIdentifier("synveil.move.loading")
                }
            case .loaded(let nodes):
                let directories = nodes.filter { $0.kind == .directory }
                if directories.isEmpty {
                    Section { Text("No subfolders") }
                } else {
                    Section("Folders") {
                        ForEach(directories, id: \.id) { node in
                            HStack(spacing: 12) {
                                destinationButton(node, ancestry: route.ancestry + [node.id])
                                NavigationLink(
                                    value: MovePickerRoute(
                                        directoryId: node.id, title: node.name,
                                        ancestry: route.ancestry + [node.id])
                                ) {
                                    Image(systemName: "chevron.right")
                                        .accessibilityLabel("Browse \(node.name)")
                                }
                                .accessibilityIdentifier("synveil.move.browse.\(node.id.rawValue)")
                            }
                        }
                    }
                }
            case .failed(let message):
                Section {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(message).fixedSize(horizontal: false, vertical: true)
                        Button("Try Again") { Task { await load(route, force: true) } }
                    }
                    .accessibilityIdentifier("synveil.move.error")
                }
            }
        }
        .navigationTitle(route.title)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: route.directoryId) { await load(route) }
    }

    private func destinationButton(_ node: Node, ancestry: [NodeId]) -> some View {
        let blocked =
            node.id == source.id || ancestry.contains(source.id)
            || source.parentId == node.id
        return Button {
            guard !blocked, node.libraryId == library.id, node.kind == .directory,
                node.state == .active
            else { return }
            onSelect(node, ancestry)
            dismiss()
        } label: {
            VStack(alignment: .leading, spacing: 3) {
                Label(
                    node.id == library.rootNodeId ? "Library root" : node.name,
                    systemImage: "folder"
                )
                .fixedSize(horizontal: false, vertical: true)
                if blocked {
                    Text("This folder is the source, its known descendant, or the current parent.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .disabled(blocked)
        .accessibilityHint(
            blocked
                ? "This destination would create a no-op or known folder cycle."
                : "Selects this verified directory and its current revision."
        )
        .accessibilityIdentifier("synveil.move.select.\(node.id.rawValue)")
    }

    private func load(_ route: MovePickerRoute, force: Bool = false) async {
        guard sessionIsCurrent, library.status == .active else { return }
        if !force, loadStates[route.directoryId] != nil { return }
        loadStates[route.directoryId] = .loading
        guard let repository else {
            loadStates[route.directoryId] = .failed("Secure folder access is unavailable.")
            return
        }
        if route.directoryId == library.rootNodeId {
            guard await loadRootMetadata() else {
                loadStates[route.directoryId] = .failed(
                    "The authoritative Library root revision is unavailable.")
                return
            }
        }
        let parent: NodeParentScope =
            route.directoryId == library.rootNodeId
            ? .libraryRoot(rootNodeId: library.rootNodeId)
            : .directory(route.directoryId)
        switch await repository.listChildren(libraryId: library.id, parent: parent) {
        case .failed:
            guard sessionIsCurrent else { return }
            loadStates[route.directoryId] = .failed(
                "Destination folders could not be verified. Retry this read-only folder request.")
        case .loaded(let nodes):
            guard sessionIsCurrent else { return }
            let isValid = nodes.allSatisfy {
                $0.libraryId == library.id && $0.parentId == route.directoryId
                    && $0.id != route.directoryId && $0.state == .active
            }
            guard isValid else {
                loadStates[route.directoryId] = .failed(
                    "The destination response did not match this Library and folder.")
                return
            }
            for node in nodes { verifiedNodes[node.id] = node }
            loadStates[route.directoryId] = .loaded(nodes)
        }
    }

    @discardableResult
    private func loadRootMetadata() async -> Bool {
        if verifiedNodes[library.rootNodeId] != nil { return true }
        guard sessionIsCurrent, let repository else { return false }
        let result = await repository.getNodeMetadata(
            libraryId: library.id, nodeId: library.rootNodeId)
        guard sessionIsCurrent, case .loaded(let root) = result,
            root.id == library.rootNodeId, root.libraryId == library.id,
            root.kind == .directory, root.state == .active, root.trashedAt == nil,
            root.restoreDeadline == nil, !root.purgeEligible
        else { return false }
        verifiedNodes[root.id] = root
        return true
    }

    private var sessionIsCurrent: Bool {
        sessionController.state == .authenticated
            && sessionController.lifecycleRevision == sessionRevision
    }

    private func invalidateIfNeeded() {
        if !sessionIsCurrent {
            loadStates.removeAll()
            verifiedNodes.removeAll()
        }
    }
}

struct MutationNoticeView: View {
    let notice: MetadataMutationNotice
    let identifier: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(notice.title, systemImage: notice.symbol)
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            Text(notice.message)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)
        }
        .foregroundStyle(notice.isError ? Color.red : Color.primary)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(notice.title). \(notice.message)")
        .accessibilityAddTraits(.updatesFrequently)
        .accessibilityIdentifier(identifier)
    }
}

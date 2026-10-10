import SwiftUI

/// Native, read-only browser for one authenticated Library directory.
struct NodeBrowserView: View {
    private let sessionController: SessionController
    private let route: NodeBrowserRoute
    private let nodeRepository: (any NodeRepositoryProtocol)?
    private let metadataMutationFeature: (any MetadataMutationFeatureProtocol)?

    @State private var viewModel: NodeBrowserViewModel
    @State private var mutationViewModel: MetadataMutationViewModel
    @State private var presentedSheet: NodeBrowserSheet?
    @State private var pendingTrashNode: Node?

    init(
        repository: (any NodeRepositoryProtocol)?,
        sessionController: SessionController,
        route: NodeBrowserRoute,
        metadataMutationFeature: (any MetadataMutationFeatureProtocol)? = nil
    ) {
        self.sessionController = sessionController
        self.route = route
        self.nodeRepository = repository
        self.metadataMutationFeature = metadataMutationFeature
        _viewModel = State(
            initialValue: NodeBrowserViewModel(
                repository: repository,
                sessionController: sessionController,
                route: route
            )
        )
        _mutationViewModel = State(
            initialValue: MetadataMutationViewModel(
                route: route, feature: metadataMutationFeature,
                sessionController: sessionController))
    }

    var body: some View {
        List {
            mutationControls
            directoryContents
        }
        .listStyle(.insetGrouped)
        .navigationTitle(route.directoryTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if mutationViewModel.canCreateFolder {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        presentedSheet = .newFolder
                    } label: {
                        Label("New Folder", systemImage: "folder.badge.plus")
                    }
                    .accessibilityIdentifier("synveil.node.new-folder")
                }
            }
            if metadataMutationFeature != nil {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        presentedSheet = .activity
                    } label: {
                        Label("Pending Changes", systemImage: "tray.full")
                    }
                    .accessibilityHint(
                        "Shows saved mutation queue states. Opening it does not send changes."
                    )
                    .accessibilityIdentifier("synveil.node.pending-changes")
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task {
                        await viewModel.refresh()
                        await mutationViewModel.refreshAvailability(forceParentRefresh: true)
                    }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .disabled(!viewModel.canRefresh || viewModel.isRequestInProgress)
                .accessibilityLabel("Refresh folder")
                .accessibilityHint("Reloads this folder from the selected Library.")
                .accessibilityIdentifier("synveil.node.refresh")
            }
        }
        .refreshable {
            await viewModel.refresh()
            await mutationViewModel.refreshAvailability(forceParentRefresh: true)
        }
        .task {
            await viewModel.loadIfNeeded()
            await mutationViewModel.refreshAvailability()
        }
        .onChange(of: sessionController.state) { _, _ in
            viewModel.sessionDidChange()
            mutationViewModel.sessionDidChange()
            if sessionController.state != .authenticated {
                presentedSheet = nil
                pendingTrashNode = nil
            }
        }
        .onChange(of: sessionController.lifecycleRevision) { _, _ in
            mutationViewModel.sessionDidChange()
            presentedSheet = nil
            pendingTrashNode = nil
        }
        .sheet(item: $presentedSheet) { sheet in sheetContents(for: sheet) }
        .confirmationDialog(
            "Move \(pendingTrashNode?.name ?? "item") to Trash?",
            isPresented: Binding(
                get: { pendingTrashNode != nil },
                set: { if !$0 { pendingTrashNode = nil } }),
            titleVisibility: .visible
        ) {
            Button("Move to Trash", role: .destructive) {
                guard let node = pendingTrashNode else { return }
                pendingTrashNode = nil
                Task { _ = await mutationViewModel.enqueueTrash(node: node, confirmed: true) }
            }
            .accessibilityIdentifier("synveil.node.trash.confirm")
            Button("Cancel", role: .cancel) { pendingTrashNode = nil }
                .accessibilityIdentifier("synveil.node.trash.cancel")
        } message: {
            Text(
                "This is a logical Trash operation, not immediate physical deletion. A nonempty folder may be rejected; descendants are not trashed automatically."
            )
        }
        .accessibilityIdentifier(directoryAccessibilityIdentifier)
    }

    @ViewBuilder
    private var mutationControls: some View {
        if metadataMutationFeature == nil {
            Section("Changes") {
                Label(
                    "Metadata changes are unavailable because the durable queue is not ready. Browsing remains available.",
                    systemImage: "lock"
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.node.mutations.unavailable")
            }
        } else if route.library.status != .active {
            Section("Changes unavailable") {
                Label(
                    route.library.status == .readOnly
                        ? "This Library is read-only. You can continue browsing."
                        : "This Library is quarantined. You can continue browsing.",
                    systemImage: route.library.status == .readOnly
                        ? "lock" : "exclamationmark.shield"
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.node.mutations.library-status")
            }
        } else if mutationViewModel.isCheckingAvailability {
            Section("Changes") {
                ProgressView("Checking saved synchronization state…")
                    .accessibilityLabel("Checking synchronization state")
                    .accessibilityIdentifier("synveil.node.mutations.checking")
            }
        } else if mutationViewModel.availability == .setupRequired {
            Section("Changes not enabled") {
                Text(
                    "Enable Changes connects to the server to establish synchronization state. It does not send a metadata change."
                )
                .fixedSize(horizontal: false, vertical: true)
                Button {
                    Task { await mutationViewModel.enableChanges() }
                } label: {
                    if mutationViewModel.isPreparingChanges {
                        ProgressView("Preparing synchronization…")
                    } else {
                        Label(
                            "Enable Changes for This Library",
                            systemImage: "arrow.triangle.2.circlepath")
                    }
                }
                .disabled(mutationViewModel.isPreparingChanges)
                .accessibilityIdentifier("synveil.node.mutations.enable-changes")
            }
        } else if case .unavailable(let reason) = mutationViewModel.availability {
            Section("Changes unavailable") {
                Label(
                    MetadataMutationViewModel.message(for: reason),
                    systemImage: "exclamationmark.triangle"
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.node.mutations.blocked")
            }
        }
        if let notice = mutationViewModel.notice {
            Section("Change status") {
                MutationNoticeView(notice: notice, identifier: "synveil.node.mutations.notice")
            }
        }
        if route.library.status == .active, mutationViewModel.availability == .ready,
            !mutationViewModel.isCheckingFolderParent
        {
            folderParentControls
        }
    }

    @ViewBuilder
    private var folderParentControls: some View {
        switch mutationViewModel.folderParentAvailability {
        case .ready:
            EmptyView()
        case .setupRequired:
            Section("New Folder unavailable") {
                Text("The verified synchronization base is no longer available.")
                    .fixedSize(horizontal: false, vertical: true)
                Button("Enable Changes for This Library") {
                    Task { await mutationViewModel.enableChanges() }
                }
                .accessibilityHint(
                    "Establishes synchronization state. It does not queue or send a metadata change."
                )
                .accessibilityIdentifier("synveil.node.mutations.enable-changes")
            }
        case .unavailable(.invalidMetadata):
            Section("New Folder unavailable") {
                Text(
                    "The current folder revision could not be verified. Refresh its metadata before creating a folder."
                )
                .fixedSize(horizontal: false, vertical: true)
                Button("Check Folder Metadata") {
                    Task {
                        await mutationViewModel.refreshAvailability(forceParentRefresh: true)
                    }
                }
                .accessibilityHint(
                    "Reads the current folder metadata. It does not queue or send a change."
                )
                .accessibilityIdentifier("synveil.node.mutations.check-parent")
            }
        case .unavailable(let reason):
            Section("New Folder unavailable") {
                Text(MetadataMutationViewModel.message(for: reason))
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.node.mutations.parent-unavailable")
            }
        }
    }

    @ViewBuilder
    private func sheetContents(for sheet: NodeBrowserSheet) -> some View {
        switch sheet {
        case .newFolder:
            NavigationStack {
                MetadataNameEditorView(
                    title: "New Folder", initialName: "", fieldLabel: "Folder name",
                    helpText:
                        "Enter the exact logical name. Synveil preserves Unicode spelling and does not change the server until you send this queued operation."
                ) { name in
                    await mutationViewModel.enqueueFolder(name: name)
                }
            }
        case .rename(let node):
            NavigationStack {
                MetadataNameEditorView(
                    title: "Rename", initialName: node.name, fieldLabel: "New name",
                    helpText:
                        "The current validated name is prefilled. The server-backed list changes only after an Applied result."
                ) { name in
                    await mutationViewModel.enqueueRename(node: node, newName: name)
                }
            }
        case .move(let node):
            MoveDestinationPickerView(
                repository: nodeRepository,
                sessionController: sessionController,
                library: route.library.mutationContext,
                source: node,
                initialDirectoryId: route.parentScope.expectedParentId,
                initialDirectoryTitle: route.directoryTitle,
                initialAncestry: route.ancestry,
                initialParentSnapshot: route.parentNodeSnapshot,
                initialChildren: viewModel.visibleNodes ?? []
            ) { destination, destinationAncestry in
                presentedSheet = nil
                Task {
                    _ = await mutationViewModel.enqueueMove(
                        node: node, destination: destination,
                        destinationAncestry: destinationAncestry)
                }
            }
        case .activity:
            if let metadataMutationFeature {
                NavigationStack {
                    MutationActivityView(
                        library: route.library.mutationContext,
                        feature: metadataMutationFeature,
                        sessionController: sessionController,
                        onConfirmedMutation: {
                            Task {
                                await viewModel.refresh()
                                await mutationViewModel.refreshAvailability(
                                    forceParentRefresh: true)
                            }
                        })
                }
            } else {
                ContentUnavailableView(
                    "Pending Changes unavailable",
                    systemImage: "tray",
                    description: Text("The durable mutation queue is not ready."))
            }
        }
    }

    @ViewBuilder
    private var directoryContents: some View {
        switch viewModel.state {
        case .idle, .loading:
            Section {
                ProgressView("Loading folder…")
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("Loading folder contents")
                    .accessibilityIdentifier("synveil.node.loading")
            } header: {
                folderHeader
            }

        case .loaded(let nodes):
            nodeRows(nodes)

        case .empty:
            Section {
                emptyDirectory
            } header: {
                folderHeader
            }

        case .refreshing(let nodes):
            Section {
                ProgressView("Refreshing folder…")
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("Refreshing folder contents")
                    .accessibilityIdentifier("synveil.node.refresh-progress")
            } header: {
                folderHeader
            }
            if nodes.isEmpty {
                previouslyEmptyDirectory
            } else {
                nodeRows(nodes)
            }

        case .refreshFailed(let nodes, let failure):
            Section {
                NodeBrowserFeedbackView(
                    title: failure.title,
                    message:
                        "\(failure.message) Showing previously loaded items; they have not been "
                        + "verified by this refresh.",
                    symbol: "exclamationmark.triangle.fill",
                    style: .error,
                    actionTitle: failure.canRetry ? "Try Again" : nil,
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.node.refresh-error"
                )
            } header: {
                Text("Previous Results")
            }
            if nodes.isEmpty {
                previouslyEmptyDirectory
            } else {
                nodeRows(nodes)
            }

        case .refreshCancelled(let nodes):
            Section {
                NodeBrowserFeedbackView(
                    title: "Refresh cancelled",
                    message: "Showing previously loaded items. Refresh to check this folder again.",
                    symbol: "arrow.clockwise",
                    style: .notice,
                    actionTitle: "Refresh",
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.node.refresh-cancelled"
                )
            } header: {
                Text("Previous Results")
            }
            if nodes.isEmpty {
                previouslyEmptyDirectory
            } else {
                nodeRows(nodes)
            }

        case .failed(let failure):
            Section {
                NodeBrowserFeedbackView(
                    title: failure.title,
                    message: failure.message,
                    symbol: failure.canRetry ? "wifi.exclamationmark" : "exclamationmark.shield",
                    style: .error,
                    actionTitle: failure.canRetry ? "Retry" : nil,
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.node.error"
                )
            } header: {
                folderHeader
            }

        case .cancelled:
            Section {
                NodeBrowserFeedbackView(
                    title: "Folder request cancelled",
                    message: "No folder contents were loaded. Retry when ready.",
                    symbol: "arrow.clockwise",
                    style: .notice,
                    actionTitle: "Retry",
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.node.cancelled"
                )
            } header: {
                folderHeader
            }

        case .invalidated:
            EmptyView()
        }
    }

    @ViewBuilder
    private var folderHeader: some View {
        switch route.parentScope {
        case .libraryRoot:
            Text("Root folder · \(route.library.name)")
        case .directory:
            Text("Folder contents · \(route.library.name)")
        }
    }

    private var directoryAccessibilityIdentifier: String {
        let scopeName: String
        switch route.parentScope {
        case .libraryRoot:
            scopeName = "root"
        case .directory:
            scopeName = "directory"
        }
        return "synveil.node.browser.\(route.library.id.rawValue).\(scopeName)."
            + route.parentScope.expectedParentId.rawValue
    }

    private var emptyDirectory: some View {
        VStack(spacing: 14) {
            Image(systemName: "folder")
                .font(.largeTitle)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)

            Text("This folder is empty.")
                .font(.title3.weight(.semibold))
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("synveil.node.empty.title")

            Button {
                Task { await viewModel.refresh() }
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.bordered)
            .accessibilityIdentifier("synveil.node.empty.refresh")
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 32)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("synveil.node.empty")
    }

    private var previouslyEmptyDirectory: some View {
        Text("The previously loaded result was an empty folder.")
            .font(.body)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityIdentifier("synveil.node.previous-empty")
    }

    @ViewBuilder
    private func nodeRows(_ nodes: [Node]) -> some View {
        Section("Items") {
            ForEach(nodes, id: \.id) { node in
                nodeRow(node)
            }
        }
    }

    private func nodeRow(_ node: Node) -> some View {
        let row: AnyView
        if let destination = viewModel.route(into: node) {
            row = AnyView(
                NavigationLink(value: destination) { NodeBrowserRow(node: node) }
                    .accessibilityHint("Opens this folder's direct contents."))
        } else if let destination = viewModel.details(for: node) {
            row = AnyView(
                NavigationLink(value: destination) { NodeBrowserRow(node: node) }
                    .accessibilityHint(
                        "Opens read-only file information. File content is not available here."))
        } else {
            row = AnyView(
                NodeBrowserRow(node: node)
                    .accessibilityHint(
                        "This folder links to an ancestor and cannot be opened again."))
        }
        return
            row
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(NodeBrowserRow.accessibilityDescription(for: node))
            .accessibilityIdentifier("synveil.node.row.\(node.id.rawValue)")
            .contextMenu {
                if mutationViewModel.canEdit && node.state == .active {
                    Button("Rename", systemImage: "pencil") {
                        presentedSheet = .rename(node)
                    }
                    .accessibilityIdentifier("synveil.node.rename.\(node.id.rawValue)")
                    Button("Move", systemImage: "folder") {
                        presentedSheet = .move(node)
                    }
                    .accessibilityIdentifier("synveil.node.move.\(node.id.rawValue)")
                    Button(role: .destructive) {
                        pendingTrashNode = node
                    } label: {
                        Label("Move to Trash", systemImage: "trash")
                    }
                    .accessibilityIdentifier("synveil.node.trash.\(node.id.rawValue)")
                }
            }
    }
}

private enum NodeBrowserSheet: Identifiable {
    case newFolder
    case rename(Node)
    case move(Node)
    case activity

    var id: String {
        switch self {
        case .newFolder: "new-folder"
        case .rename(let node): "rename-\(node.id.rawValue)"
        case .move(let node): "move-\(node.id.rawValue)"
        case .activity: "activity"
        }
    }
}

private struct NodeBrowserRow: View {
    let node: Node

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: node.kind == .directory ? "folder" : "doc")
                .font(.title2)
                .foregroundStyle(.tint)
                .frame(minWidth: 28, minHeight: 44)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 5) {
                Text(node.name)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .fixedSize(horizontal: false, vertical: true)

                Text("Updated \(node.updatedAt.formatted(date: .abbreviated, time: .shortened))")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }

    static func accessibilityDescription(for node: Node) -> String {
        let kind = node.kind == .directory ? "Folder" : "File"
        let updated = node.updatedAt.formatted(date: .abbreviated, time: .shortened)
        return "\(kind): \(node.name), updated \(updated)"
    }
}

private struct NodeBrowserFeedbackView: View {
    enum Style: Equatable {
        case error
        case notice
    }

    let title: String
    let message: String
    let symbol: String
    let style: Style
    let actionTitle: String?
    let action: () -> Void
    let identifier: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(title, systemImage: symbol)
                .font(.headline)
                .foregroundStyle(style == .error ? Color.red : Color.secondary)
                .accessibilityAddTraits(.isHeader)

            Text(message)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)

            if let actionTitle {
                Button(actionTitle, action: action)
                    .buttonStyle(.bordered)
                    .accessibilityIdentifier("\(identifier).action")
            }
        }
        .padding(.vertical, 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("\(title). \(message)")
        .accessibilityAddTraits(.updatesFrequently)
        .accessibilityIdentifier(identifier)
    }
}

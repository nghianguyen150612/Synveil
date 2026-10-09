import SwiftUI

/// Native, read-only browser for one authenticated Library directory.
struct NodeBrowserView: View {
    private let sessionController: SessionController
    private let route: NodeBrowserRoute

    @State private var viewModel: NodeBrowserViewModel

    init(
        repository: (any NodeRepositoryProtocol)?,
        sessionController: SessionController,
        route: NodeBrowserRoute
    ) {
        self.sessionController = sessionController
        self.route = route
        _viewModel = State(
            initialValue: NodeBrowserViewModel(
                repository: repository,
                sessionController: sessionController,
                route: route
            )
        )
    }

    var body: some View {
        List {
            directoryContents
        }
        .listStyle(.insetGrouped)
        .navigationTitle(route.directoryTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task { await viewModel.refresh() }
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
        }
        .task {
            await viewModel.loadIfNeeded()
        }
        .onChange(of: sessionController.state) { _, _ in
            viewModel.sessionDidChange()
        }
        .accessibilityIdentifier(directoryAccessibilityIdentifier)
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
                if let destination = viewModel.route(into: node) {
                    NavigationLink(value: destination) {
                        NodeBrowserRow(node: node)
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(NodeBrowserRow.accessibilityDescription(for: node))
                    .accessibilityHint("Opens this folder's direct contents.")
                    .accessibilityIdentifier("synveil.node.row.\(node.id.rawValue)")
                } else if let destination = viewModel.details(for: node) {
                    NavigationLink(value: destination) {
                        NodeBrowserRow(node: node)
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(NodeBrowserRow.accessibilityDescription(for: node))
                    .accessibilityHint(
                        "Opens read-only file information. File content is not available here."
                    )
                    .accessibilityIdentifier("synveil.node.row.\(node.id.rawValue)")
                } else {
                    NodeBrowserRow(node: node)
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel(NodeBrowserRow.accessibilityDescription(for: node))
                        .accessibilityHint(
                            "This folder links to an ancestor and cannot be opened again."
                        )
                        .accessibilityIdentifier("synveil.node.row.\(node.id.rawValue)")
                }
            }
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

import SwiftUI

/// State-owned details remain in the containing Library's native navigation hierarchy.
struct NodeFileDetailsView: View {
    private let sessionController: SessionController
    @State private var viewModel: NodeFileDetailsViewModel

    init(
        repository: (any NodeRepositoryProtocol)?, sessionController: SessionController,
        offlineDetailsService: (any OfflineNodeBrowserServiceProtocol)? = nil,
        route: NodeFileDetailsRoute
    ) {
        self.sessionController = sessionController
        _viewModel = State(
            initialValue: NodeFileDetailsViewModel(
                repository: repository, sessionController: sessionController,
                offlineDetailsService: offlineDetailsService, route: route))
    }

    var body: some View {
        List {
            detailsContents
            if viewModel.presentationState != .invalidated {
                Section {
                    Label(
                        "File content is not available in this version. This screen shows read-only metadata.",
                        systemImage: "info.circle"
                    )
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.node.details.read-only-note")
                }
            }
        }
        .navigationTitle("File Details")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task { await viewModel.refresh() }
                } label: {
                    Label(
                        viewModel.contentSource == .cached ? "Refresh from Server" : "Refresh",
                        systemImage: "arrow.clockwise")
                }
                .disabled(!viewModel.canRefresh)
                .accessibilityLabel(
                    viewModel.contentSource == .cached
                        ? "Refresh from Server" : "Refresh file metadata from server"
                )
                .accessibilityHint("Checks the selected file's current server metadata.")
                .accessibilityIdentifier("synveil.node.details.refresh")
            }
        }
        .refreshable { await viewModel.refresh() }
        .task { await viewModel.loadIfNeeded() }
        .onChange(of: sessionController.state) { _, _ in viewModel.sessionDidChange() }
        .onChange(of: sessionController.lifecycleRevision) { _, _ in viewModel.sessionDidChange() }
        .onDisappear { viewModel.invalidate() }
        .accessibilityIdentifier("synveil.node.details")
    }

    @ViewBuilder
    private var detailsContents: some View {
        switch viewModel.presentationState {
        case .idle, .loading:
            Section {
                ProgressView(
                    viewModel.route.contentSource == .cached
                        ? "Loading saved file metadata…" : "Loading current file metadata…"
                )
                .accessibilityIdentifier("synveil.node.details.loading")
            }
        case .loaded(let node):
            metadata(node)
        case .refreshing(let node):
            Section {
                ProgressView("Refreshing file metadata…")
                    .accessibilityIdentifier("synveil.node.details.refresh-progress")
                Text("Showing previously loaded metadata while checking the server.")
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("synveil.node.details.previous")
            }
            metadata(node)
        case .refreshFailed(let node, let failure):
            feedback(
                title: failure.title,
                message: failure.message
                    + " Showing previously loaded metadata; it has not been verified by this refresh.",
                identifier: "synveil.node.details.refresh-error")
            metadata(node)
        case .unavailable:
            feedback(
                title: "File unavailable",
                message:
                    "This file is no longer available at its previous location. Return to the folder or refresh to check again.",
                identifier: "synveil.node.details.unavailable")
        case .savedUnavailable(let failure):
            feedback(
                title: failure == .synchronizationRecoveryRequired
                    ? "Synchronization recovery required" : "Saved metadata unavailable",
                message: failure == .synchronizationRecoveryRequired
                    ? "Saved file metadata needs synchronization recovery before it can be shown."
                    : "No complete, validated saved file record is available for this folder.",
                identifier: "synveil.node.details.saved-unavailable")
        case .failed(let failure):
            feedback(
                title: failure.title, message: failure.message,
                identifier: "synveil.node.details.error")
        case .cancelled:
            feedback(
                title: "File metadata request cancelled",
                message: "Refresh to check this file again.",
                identifier: "synveil.node.details.cancelled")
        case .invalidated:
            EmptyView()
        }
    }

    private func feedback(title: String, message: String, identifier: String) -> some View {
        Section {
            VStack(alignment: .leading, spacing: 10) {
                Label(title, systemImage: "info.circle")
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
                Text(message).fixedSize(horizontal: false, vertical: true)
                if viewModel.canRefresh {
                    Button(
                        viewModel.contentSource == .cached ? "Refresh from Server" : "Refresh"
                    ) {
                        Task { await viewModel.refresh() }
                    }
                    .accessibilityIdentifier("\(identifier).action")
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityAddTraits(.updatesFrequently)
            .accessibilityIdentifier(identifier)
        }
    }

    @ViewBuilder
    private func metadata(_ node: Node) -> some View {
        if viewModel.contentSource == .cached {
            Section {
                Label(
                    viewModel.savedProjectionState?.completeness == .rebaselineRequired
                        ? "Synchronization recovery required. Saved metadata is not verified with the server."
                        : "Saved metadata — current server state not verified.",
                    systemImage:
                        viewModel.savedProjectionState?.completeness == .rebaselineRequired
                        ? "exclamationmark.triangle" : "tray.full"
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.node.details.saved-source")
            }
        } else if viewModel.contentSource == .previouslyLoaded {
            Section {
                Label(
                    "Previously loaded metadata — not verified by this refresh.",
                    systemImage: "clock.arrow.circlepath"
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.node.details.previous-source")
            }
        } else {
            Section {
                Label(
                    "Current metadata retrieved from server.",
                    systemImage: "network"
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.node.details.live-source")
            }
        }
        Section("File") {
            Text(node.name)
                .font(.headline)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("synveil.node.details.name")
            LabeledContent("Kind", value: "File")
            LabeledContent("Created") {
                Text(node.createdAt.formatted(date: .abbreviated, time: .shortened))
                    .multilineTextAlignment(.trailing)
            }
            LabeledContent("Updated") {
                Text(node.updatedAt.formatted(date: .abbreviated, time: .shortened))
                    .multilineTextAlignment(.trailing)
            }
            metadataRow(title: "Revision", value: node.revision.rawValue)
        }
        Section("Identity") {
            metadataRow(title: "Node ID", value: node.id.rawValue)
            metadataRow(title: "Library", value: viewModel.route.library.name)
            metadataRow(title: "Library ID", value: node.libraryId.rawValue)
            metadataRow(title: "Folder", value: viewModel.route.parentDirectoryTitle)
            if let parentId = node.parentId {
                metadataRow(title: "Parent ID", value: parentId.rawValue)
            }
            if let versionId = node.currentVersionId {
                metadataRow(title: "Current version ID", value: versionId.rawValue)
            }
        }
    }

    private func metadataRow(title: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.subheadline).foregroundStyle(.secondary)
            Text(value)
                .font(.footnote.monospaced())
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

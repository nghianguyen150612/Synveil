import SwiftUI

/// Authenticated, read-only library catalog. A new state-owned model is made for each session.
struct LibraryCatalogView: View {
    private let sessionController: SessionController
    private let nodeRepository: (any NodeRepositoryProtocol)?

    @State private var viewModel: LibraryCatalogViewModel
    @State private var isShowingLogoutConfirmation = false

    init(
        repository: (any LibraryCatalogRepositoryProtocol)?,
        nodeRepository: (any NodeRepositoryProtocol)? = nil,
        sessionController: SessionController
    ) {
        self.sessionController = sessionController
        self.nodeRepository = nodeRepository
        _viewModel = State(
            initialValue: LibraryCatalogViewModel(
                repository: repository,
                sessionController: sessionController
            )
        )
    }

    var body: some View {
        Group {
            if sessionController.state == .authenticated {
                NavigationStack {
                    List {
                        catalogSections
                    }
                    .listStyle(.insetGrouped)
                    .navigationTitle("Libraries")
                    .navigationBarTitleDisplayMode(.large)
                    .navigationDestination(for: LibraryId.self) { id in
                        if let library = viewModel.library(with: id) {
                            NodeBrowserView(
                                repository: nodeRepository,
                                sessionController: sessionController,
                                route: .root(for: library)
                            )
                            .id(NodeBrowserRoute.root(for: library))
                        } else {
                            LibrarySelectionUnavailableView()
                        }
                    }
                    .navigationDestination(for: NodeBrowserRoute.self) { route in
                        NodeBrowserView(
                            repository: nodeRepository,
                            sessionController: sessionController,
                            route: route
                        )
                        .id(route)
                    }
                    .navigationDestination(for: NodeFileDetailsRoute.self) { route in
                        NodeFileDetailsView(route: route)
                    }
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button {
                                Task { await viewModel.refresh() }
                            } label: {
                                Image(systemName: "arrow.clockwise")
                            }
                            .disabled(!viewModel.canRefresh || viewModel.isRequestInProgress)
                            .accessibilityLabel("Refresh libraries")
                            .accessibilityHint("Loads the current library catalog from the server.")
                            .accessibilityIdentifier("synveil.library.refresh")
                        }

                        ToolbarItem(placement: .topBarTrailing) {
                            Button(role: .destructive) {
                                isShowingLogoutConfirmation = true
                            } label: {
                                Image(systemName: "rectangle.portrait.and.arrow.right")
                            }
                            .accessibilityLabel("Log Out / Forget Session")
                            .accessibilityHint(
                                "Deletes this device's saved credential after confirmation. It does not revoke server access."
                            )
                            .accessibilityIdentifier(SessionLogoutAccessibility.logoutButton)
                        }
                    }
                    .refreshable {
                        await viewModel.refresh()
                    }
                    .task {
                        await viewModel.loadIfNeeded()
                    }
                    .confirmationDialog(
                        "Forget this device session?",
                        isPresented: $isShowingLogoutConfirmation,
                        titleVisibility: .visible
                    ) {
                        Button("Log Out and Forget Session", role: .destructive) {
                            Task { await sessionController.requestLogout() }
                        }
                        .accessibilityIdentifier(SessionLogoutAccessibility.logoutConfirmation)

                        Button("Cancel", role: .cancel) {}
                            .accessibilityIdentifier(SessionLogoutAccessibility.logoutCancellation)
                    } message: {
                        Text(
                            "This deletes the saved credential from this device. It does not revoke "
                                + "the credential on the server."
                        )
                    }
                    .accessibilityIdentifier("synveil.root.authenticated")
                }
            } else {
                // Root routing owns recovery. Never leave catalog details visible during transition.
                EmptyView()
            }
        }
        .onChange(of: sessionController.state) { _, _ in
            viewModel.sessionDidChange()
        }
    }

    @ViewBuilder
    private var catalogSections: some View {
        switch viewModel.state {
        case .idle, .loading:
            Section {
                ProgressView("Loading libraries…")
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("Loading libraries")
                    .accessibilityIdentifier("synveil.library.loading")
            }

        case .loaded(let libraries):
            libraryRows(libraries)

        case .empty:
            Section {
                emptyCatalog
            }

        case .refreshing(let libraries):
            Section {
                ProgressView("Refreshing libraries…")
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("Refreshing libraries")
                    .accessibilityIdentifier("synveil.library.refresh-progress")
            }
            libraryRows(libraries)

        case .refreshFailed(let libraries, let failure):
            Section {
                CatalogFeedbackView(
                    title: failure.title,
                    message:
                        "\(failure.message) Showing previously loaded results; they have not been verified by this refresh.",
                    symbol: "exclamationmark.triangle.fill",
                    style: .error,
                    actionTitle: failure.canRetry ? "Try Again" : nil,
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.library.refresh-error"
                )
            } header: {
                Text("Previous Results")
            }
            libraryRows(libraries)

        case .refreshCancelled(let libraries):
            Section {
                CatalogFeedbackView(
                    title: "Refresh cancelled",
                    message:
                        "Showing previously loaded results. Refresh to check for current libraries.",
                    symbol: "arrow.clockwise",
                    style: .notice,
                    actionTitle: "Refresh",
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.library.refresh-cancelled"
                )
            }
            libraryRows(libraries)

        case .failed(let failure):
            Section {
                CatalogFeedbackView(
                    title: failure.title,
                    message: failure.message,
                    symbol: failure.canRetry ? "wifi.exclamationmark" : "exclamationmark.shield",
                    style: .error,
                    actionTitle: failure.canRetry ? "Retry" : nil,
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.library.error"
                )
            }

        case .cancelled:
            Section {
                CatalogFeedbackView(
                    title: "Library request cancelled",
                    message: "No catalog result was loaded. Retry when ready.",
                    symbol: "arrow.clockwise",
                    style: .notice,
                    actionTitle: "Retry",
                    action: { Task { await viewModel.refresh() } },
                    identifier: "synveil.library.cancelled"
                )
            }

        case .invalidated:
            EmptyView()
        }
    }

    private var emptyCatalog: some View {
        VStack(spacing: 14) {
            Image(systemName: "books.vertical")
                .font(.largeTitle)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)

            Text("No libraries available")
                .font(.title3.weight(.semibold))
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("synveil.library.empty.title")

            Text("This authenticated server currently has no libraries visible to this device.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("synveil.library.empty.message")

            Button {
                Task { await viewModel.refresh() }
            } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.bordered)
            .accessibilityIdentifier("synveil.library.empty.refresh")
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 32)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("synveil.library.empty")
    }

    @ViewBuilder
    private func libraryRows(_ libraries: [Library]) -> some View {
        Section("Libraries") {
            ForEach(libraries, id: \.id) { library in
                NavigationLink(value: library.id) {
                    LibraryCatalogRow(library: library)
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(LibraryCatalogRow.accessibilityDescription(for: library))
                .accessibilityHint(
                    "Opens this Library's root folder contents."
                )
                .accessibilityIdentifier("synveil.library.row.\(library.id.rawValue)")
            }
        }
    }
}

private struct LibraryCatalogRow: View {
    let library: Library

    private var status: LibraryStatusPresentation {
        LibraryStatusPresentation.make(for: library.status)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            Image(systemName: "folder")
                .font(.title2)
                .foregroundStyle(.tint)
                .frame(minWidth: 28)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 6) {
                Text(library.name)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .fixedSize(horizontal: false, vertical: true)

                Label(status.label, systemImage: status.symbol)
                    .font(.subheadline)
                    .foregroundStyle(statusColor)
                    .accessibilityLabel("Status: \(status.label)")
                    .accessibilityIdentifier("synveil.library.status.\(library.id.rawValue)")

                Text("Updated \(library.updatedAt.formatted(date: .abbreviated, time: .shortened))")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
        .accessibilityIdentifier("synveil.library.row-content.\(library.id.rawValue)")
    }

    static func accessibilityDescription(for library: Library) -> String {
        let status = LibraryStatusPresentation.make(for: library.status)
        let updated = library.updatedAt.formatted(date: .abbreviated, time: .shortened)
        return "\(library.name), \(status.label), updated \(updated)"
    }

    private var statusColor: Color {
        switch library.status {
        case .active: .green
        case .readOnly: .orange
        case .quarantined: .red
        }
    }
}

private struct CatalogFeedbackView: View {
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

private struct LibrarySelectionUnavailableView: View {
    var body: some View {
        ContentUnavailableView(
            "Library contents unavailable",
            systemImage: "folder.badge.questionmark",
            description: Text("Return to the catalog and refresh the current session.")
        )
        .navigationTitle("Library")
        .accessibilityIdentifier("synveil.library.contents.unavailable")
    }
}

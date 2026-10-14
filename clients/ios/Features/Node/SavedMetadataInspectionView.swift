import Observation
import SwiftUI

@Observable
@MainActor
final class SavedMetadataInspectionViewModel {
    private(set) var libraries: [Library] = []
    private(set) var directory: NodeCachedDirectoryPresentation?
    private(set) var isLoading = false
    private(set) var unavailable = false
    private let service: any SavedMetadataInspectionProtocol
    private let controller: SessionController
    private let revision: UInt64

    init(service: any SavedMetadataInspectionProtocol, controller: SessionController) {
        self.service = service
        self.controller = controller
        revision = controller.lifecycleRevision
    }

    var isCurrent: Bool {
        controller.permitsSavedMetadataInspection && revision == controller.lifecycleRevision
    }

    func load(library: Library? = nil, parent: NodeParentScope? = nil) async {
        guard isCurrent, !isLoading else { return }
        isLoading = true
        unavailable = false
        defer { isLoading = false }
        if let library, let parent {
            let result = await service.browse(library: library, parent: parent)
            guard isCurrent else {
                clear()
                return
            }
            if case .saved(let saved) = result {
                directory = saved
            } else {
                directory = nil
                unavailable = true
            }
        } else {
            do {
                let saved = try await service.libraries()
                guard isCurrent else {
                    clear()
                    return
                }
                libraries = saved
            } catch {
                clear()
                if isCurrent { unavailable = true }
            }
        }
    }

    func clear() {
        libraries = []
        directory = nil
    }
}

struct SavedMetadataInspectionView: View {
    @Environment(\.dismiss) private var dismiss
    let service: any SavedMetadataInspectionProtocol
    let controller: SessionController
    @State private var model: SavedMetadataInspectionViewModel

    init(service: any SavedMetadataInspectionProtocol, controller: SessionController) {
        self.service = service
        self.controller = controller
        _model = State(
            initialValue: SavedMetadataInspectionViewModel(service: service, controller: controller)
        )
    }

    var body: some View {
        NavigationStack {
            List {
                if model.isCurrent {
                    Section {
                        Label("Saved metadata — not verified current", systemImage: "externaldrive")
                        Text(
                            controller.state == .authenticated
                                ? "Only eligible previously saved Libraries are listed; other Libraries and items may exist. Saved inspection is read-only. Current server data has not been verified."
                                : "Server authorization is awaiting verification. Only previously saved Libraries are listed; other Libraries and items may exist. Changes and file-content actions require verified authorization."
                        )
                        .fixedSize(horizontal: false, vertical: true)
                    }
                    if model.isLoading {
                        ProgressView("Reading saved metadata…")
                    } else if model.unavailable || model.libraries.isEmpty {
                        Text(
                            "Eligible saved metadata is unavailable. This does not mean your Libraries are empty."
                        )
                        .accessibilityIdentifier("synveil.saved-inspection.unavailable")
                    } else {
                        ForEach(model.libraries, id: \.id) { library in
                            NavigationLink(library.name) {
                                SavedMetadataDirectoryView(
                                    service: service, controller: controller,
                                    library: library,
                                    parent: .libraryRoot(rootNodeId: library.rootNodeId),
                                    title: library.name, ancestry: [library.rootNodeId])
                            }
                        }
                    }
                    Button("Reload Saved Status") { Task { await model.load() } }
                        .disabled(model.isLoading)
                        .accessibilityIdentifier("synveil.saved-inspection.reload")
                } else {
                    Text("Saved access is unavailable. Return to session verification.")
                }
            }
            .navigationTitle("Saved Metadata")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                        .accessibilityIdentifier("synveil.saved-inspection.done")
                }
            }
            .task { await model.load() }
            .onChange(of: controller.lifecycleRevision) { _, _ in model.clear() }
            .onChange(of: controller.permitsSavedMetadataInspection) { _, valid in
                if !valid { model.clear() }
            }
            .accessibilityIdentifier("synveil.saved-inspection.screen")
        }
    }
}

private struct SavedMetadataDirectoryView: View {
    let service: any SavedMetadataInspectionProtocol
    let controller: SessionController
    let library: Library
    let parent: NodeParentScope
    let title: String
    let ancestry: [NodeId]
    @State private var model: SavedMetadataInspectionViewModel

    init(
        service: any SavedMetadataInspectionProtocol, controller: SessionController,
        library: Library, parent: NodeParentScope, title: String, ancestry: [NodeId]
    ) {
        self.service = service
        self.controller = controller
        self.library = library
        self.parent = parent
        self.title = title
        self.ancestry = ancestry
        _model = State(
            initialValue: SavedMetadataInspectionViewModel(service: service, controller: controller)
        )
    }

    var body: some View {
        List {
            if model.isCurrent {
                if model.isLoading {
                    ProgressView("Reading saved folder…")
                } else if let saved = model.directory {
                    ClientRecoveryNotice(presentation: ClientRecoveryPresentation.saved(saved))
                    ForEach(saved.orderedItems) { item in
                        if item.kind == .directory, saved.knowledge != .staleKnown,
                            !ancestry.contains(item.id), ancestry.count < 128
                        {
                            NavigationLink(item.name) {
                                SavedMetadataDirectoryView(
                                    service: service, controller: controller,
                                    library: library, parent: .directory(item.id), title: item.name,
                                    ancestry: ancestry + [item.id])
                            }
                        } else {
                            Label(
                                item.name, systemImage: item.kind == .directory ? "folder" : "doc")
                        }
                    }
                    if saved.orderedItems.isEmpty {
                        Text(
                            saved.knowledge == .complete
                                ? "Saved folder was empty at its snapshot cut."
                                : "No saved items are known here. Other server items may exist.")
                    }
                } else {
                    Text("Saved folder metadata is unavailable. No absence has been inferred.")
                }
            } else {
                Text("Saved access is unavailable.")
            }
        }
        .navigationTitle(title)
        .task { await model.load(library: library, parent: parent) }
        .onChange(of: controller.lifecycleRevision) { _, _ in model.clear() }
        .onChange(of: controller.permitsSavedMetadataInspection) { _, valid in
            if !valid { model.clear() }
        }
    }
}

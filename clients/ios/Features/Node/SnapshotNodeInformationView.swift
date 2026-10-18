import SwiftUI

struct SnapshotNodeInformationView: View {
    private let node: RebaselineSnapshotNode
    private let sessionController: SessionController
    private let revision: UInt64
    private let library: LibraryId?
    private let downloadCoordinator: FileDownloadCoordinator?
    private let fileExportCoordinator: VerifiedFileExportCoordinator?
    init(
        node: RebaselineSnapshotNode, sessionController: SessionController,
        library: LibraryId? = nil, downloadCoordinator: FileDownloadCoordinator? = nil,
        fileExportCoordinator: VerifiedFileExportCoordinator? = nil
    ) {
        self.node = node
        self.sessionController = sessionController
        self.library = library
        self.downloadCoordinator = downloadCoordinator
        self.fileExportCoordinator = fileExportCoordinator
        revision = sessionController.lifecycleRevision
    }
    var body: some View {
        if sessionController.state == .authenticated
            && sessionController.lifecycleRevision == revision
        {
            List {
                if node.kind == .file, node.state == .active, let version = node.currentVersionId,
                    let library, downloadCoordinator != nil
                {
                    FileDownloadControls(
                        coordinator: downloadCoordinator, controller: sessionController,
                        library: library, node: node.id, version: version,
                        displayName: node.name, exportCoordinator: fileExportCoordinator)
                }
                Section("Saved Snapshot Metadata") {
                    Text(node.name).fixedSize(horizontal: false, vertical: true)
                    LabeledContent("Kind", value: node.kind == .file ? "File" : "Folder")
                    LabeledContent("Revision", value: node.revision.rawValue)
                    Text(node.id.rawValue).textSelection(.enabled)
                    if let version = node.currentVersionId {
                        Text("Current version: \(version.rawValue)").textSelection(.enabled)
                    }
                    if let content = node.content {
                        Text("Content length: \(content.byteLength.rawValue) bytes")
                        Text("SHA-256: \(content.sha256)").textSelection(.enabled)
                    }
                }
                Section {
                    Text(
                        "Live timestamps and restoration metadata are unavailable in this snapshot. Download requires online authorization."
                    )
                    .fixedSize(horizontal: false, vertical: true)
                }
            }
            .navigationTitle("Snapshot Information")
            .accessibilityIdentifier("synveil.snapshot.information")
        } else {
            Text("The session changed. Return to the Library catalog.")
        }
    }
}

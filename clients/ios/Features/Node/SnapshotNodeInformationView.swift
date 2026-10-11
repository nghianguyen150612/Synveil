import SwiftUI

struct SnapshotNodeInformationView: View {
    private let node: RebaselineSnapshotNode
    private let sessionController: SessionController
    private let revision: UInt64
    init(node: RebaselineSnapshotNode, sessionController: SessionController) {
        self.node = node
        self.sessionController = sessionController
        revision = sessionController.lifecycleRevision
    }
    var body: some View {
        if sessionController.state == .authenticated
            && sessionController.lifecycleRevision == revision
        {
            List {
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
                        "Live timestamps and restoration metadata are unavailable in this snapshot. File content is not stored on this device."
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

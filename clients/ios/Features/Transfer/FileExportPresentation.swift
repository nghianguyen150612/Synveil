import Foundation
import Observation
import QuickLook
import SwiftUI
import UIKit

enum NativeFilePresentationResult {
    case previewDismissed, shareCompleted, shareCancelled, saveReportedCompleted, saveCancelled

    var message: String {
        switch self {
        case .previewDismissed: "Preview dismissed"
        case .shareCompleted: "Share activity completed. Recipient import is not confirmed."
        case .shareCancelled: "Share activity was cancelled."
        case .saveReportedCompleted: "Files export flow reported completion. Provider sync is not confirmed."
        case .saveCancelled: "Save to Files was cancelled."
        }
    }
}

struct NativeFilePresentation: Identifiable {
    let lease: VerifiedFileExportLease
    let url: URL
    var id: String { lease.id }
}

@Observable
@MainActor
final class FileExportViewModel {
    private(set) var isPreparing = false
    private(set) var isAvailable = false
    private(set) var stateText = ""
    private(set) var failureText: String?
    private(set) var presentation: NativeFilePresentation?
    private let coordinator: VerifiedFileExportCoordinator?
    private let controller: SessionController
    private let library: LibraryId
    private let node: NodeId
    private let version: FileVersionId
    private let displayName: String
    private let coordinatorAvailable: Bool
    private let revision: UInt64
    @ObservationIgnored private var invalidated = false
    @ObservationIgnored private var preparation: Task<Void, Never>?

    init(
        coordinator: VerifiedFileExportCoordinator?, controller: SessionController,
        library: LibraryId, node: NodeId, version: FileVersionId, displayName: String
    ) {
        self.coordinator = coordinator
        coordinatorAvailable = coordinator != nil
        self.controller = controller
        self.library = library
        self.node = node
        self.version = version
        self.displayName = displayName
        revision = controller.lifecycleRevision
    }

    var isCurrent: Bool {
        !invalidated && controller.state == .authenticated && controller.lifecycleRevision == revision
    }
    var canPresent: Bool { isCurrent && isAvailable && !isPreparing && presentation == nil }

    func setVerified(_ value: Bool) { isAvailable = value && isCurrent && coordinatorAvailable }

    func present(_ purpose: VerifiedFileExportPurpose) {
        guard canPresent, let coordinator else { return }
        isPreparing = true
        failureText = nil
        stateText = "Preparing verified copy…"
        preparation = Task { [weak self] in
            guard let self else { return }
            let result = await coordinator.prepare(
                library: library, node: node, version: version, purpose: purpose,
                displayName: displayName
            ) { [weak self] state in
                guard let self, isCurrent else { return }
                stateText = switch state {
                case .preparing: "Checking verified download…"
                case .verifyingCopy: "Copying and verifying temporary file…"
                case .ready: "Verified copy ready"
                case .presented: "Presenting native file action…"
                case .revoking: "Cleaning up temporary copy…"
                case .released: "Temporary copy released"
                case .failed: "File preparation failed"
                }
            }
            guard isCurrent, !Task.isCancelled else {
                if let lease = result.lease { await coordinator.finish(lease) }
                isPreparing = false
                return
            }
            guard let lease = result.lease else {
                stateText = ""
                isPreparing = false
                failureText = Self.message(result.failure)
                return
            }
            do {
                let url = try await coordinator.beginPresentation(lease)
                guard isCurrent else {
                    await coordinator.finish(lease)
                    isPreparing = false
                    return
                }
                presentation = NativeFilePresentation(lease: lease, url: url)
                stateText = "Ready for native presentation"
            } catch {
                await coordinator.finish(lease)
                failureText = Self.message(.session)
            }
            isPreparing = false
        }
    }

    func cancel() {
        preparation?.cancel()
        preparation = nil
        isPreparing = false
        stateText = "Preparation cancelled"
    }

    func finishPresentation(result: NativeFilePresentationResult? = nil) {
        guard let current = presentation else { return }
        presentation = nil
        stateText = "Cleaning up temporary copy…"
        Task {
            let released = await coordinator?.finish(current.lease) ?? false
            if isCurrent {
                stateText = released
                    ? (result?.message ?? "Presentation dismissed; temporary copy released")
                    : "Cleanup pending. Temporary data remains charged to private storage."
            }
        }
    }

    func invalidate() {
        invalidated = true
        preparation?.cancel()
        preparation = nil
        presentation = nil
        isPreparing = false
        isAvailable = false
        failureText = nil
        stateText = "Session changed"
    }

    private static func message(_ failure: VerifiedFileExportFailure?) -> String {
        switch failure {
        case .missing: "Verified content is missing. Download this version again."
        case .integrity: "The downloaded file failed integrity verification and cannot be exported."
        case .storage: "There is not enough private storage to prepare a safe copy."
        case .unsupportedPreview: "Quick Look does not support this file type. Share or save it instead."
        case .session: "The authenticated session changed. File presentation was stopped."
        case .preparation, .none: "The verified file could not be prepared safely."
        }
    }
}

struct FileExportControls: View {
    @Environment(\.scenePhase) private var scenePhase
    @State private var model: FileExportViewModel

    init(
        coordinator: VerifiedFileExportCoordinator?, controller: SessionController,
        library: LibraryId, node: NodeId, version: FileVersionId, displayName: String,
        verified: Bool
    ) {
        _model = State(initialValue: FileExportViewModel(
            coordinator: coordinator, controller: controller, library: library, node: node,
            version: version, displayName: displayName))
        initialVerified = verified
    }

    private let initialVerified: Bool

    var body: some View {
        Group {
            if model.isCurrent && model.isAvailable {
                Section("Verified downloaded version") {
                Text("These actions use the immutable downloaded version. They do not check whether a newer server version exists.")
                    .fixedSize(horizontal: false, vertical: true)
                if model.isPreparing { ProgressView(model.stateText).accessibilityIdentifier("synveil.export.preparing") }
                else if !model.stateText.isEmpty { Text(model.stateText).accessibilityAddTraits(.updatesFrequently) }
                if let failure = model.failureText { Text(failure).fixedSize(horizontal: false, vertical: true) }
                Button("Preview") { model.present(.preview) }
                    .disabled(!model.canPresent)
                    .accessibilityLabel("Preview verified downloaded version")
                    .accessibilityHint("Prepares and verifies a private copy, then opens it in Quick Look when the file type is supported.")
                    .accessibilityIdentifier("synveil.export.preview")
                Button("Share") { model.present(.share) }
                    .disabled(!model.canPresent)
                    .accessibilityLabel("Share verified downloaded version")
                    .accessibilityHint("Prepares a verified copy and opens the iOS Share Sheet.")
                    .accessibilityIdentifier("synveil.export.share")
                Button("Save to Files") { model.present(.saveToFiles) }
                    .disabled(!model.canPresent)
                    .accessibilityLabel("Save verified downloaded version to Files")
                    .accessibilityHint("Prepares a verified copy and lets you choose a Files destination.")
                    .accessibilityIdentifier("synveil.export.save-to-files")
                if model.isPreparing {
                    Button("Cancel preparation", role: .cancel) { model.cancel() }
                        .accessibilityIdentifier("synveil.export.cancel")
                }
                Text("An app you choose to receive a shared file may keep its own copy. Synveil cannot revoke that copy.")
                    .font(.footnote).fixedSize(horizontal: false, vertical: true)
                    .onChange(of: model.presentation?.id) { _, current in
                        if current == nil { model.finishPresentation() }
                    }
                    .onChange(of: model.isCurrent) { _, current in if !current { model.invalidate() } }
                    .onDisappear {
                        model.cancel()
                        model.finishPresentation()
                    }
            }
        }
        .task { model.setVerified(initialVerified) }
        .onChange(of: initialVerified) { _, current in model.setVerified(current) }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active, model.presentation?.lease.purpose == .preview {
                model.finishPresentation(result: .previewDismissed)
            }
        }
        .sheet(
            item: Binding(get: { model.presentation }, set: { _ in }),
            onDismiss: { model.finishPresentation() }
        ) { item in
            NativeFilePresenter(presentation: item) { result in model.finishPresentation(result: result) }
                .ignoresSafeArea()
        }
    }
}

private struct NativeFilePresenter: View {
    let presentation: NativeFilePresentation
    let onFinished: @MainActor (NativeFilePresentationResult) -> Void
    var body: some View {
        Group {
            switch presentation.lease.purpose {
            case .preview:
                QuickLookNativePreview(url: presentation.url) { onFinished(.previewDismissed) }
            case .share:
                ShareSheetNativePresenter(url: presentation.url, onFinished: onFinished)
            case .saveToFiles:
                FilesExportNativePresenter(url: presentation.url, onFinished: onFinished)
            }
        }
    }
}

private struct QuickLookNativePreview: UIViewControllerRepresentable {
    let url: URL
    let onFinished: @MainActor () -> Void
    func makeCoordinator() -> Coordinator { Coordinator(url: url, onFinished: onFinished) }
    func makeUIViewController(context: Context) -> QLPreviewController {
        let controller = QLPreviewController()
        controller.dataSource = context.coordinator
        controller.delegate = context.coordinator
        return controller
    }
    func updateUIViewController(_ controller: QLPreviewController, context: Context) {}
    final class Coordinator: NSObject, QLPreviewControllerDataSource, QLPreviewControllerDelegate {
        let url: URL
        let onFinished: @MainActor () -> Void
        init(url: URL, onFinished: @escaping @MainActor () -> Void) {
            self.url = url
            self.onFinished = onFinished
        }
        func numberOfPreviewItems(in controller: QLPreviewController) -> Int { 1 }
        func previewController(_ controller: QLPreviewController, previewItemAt index: Int) -> QLPreviewItem {
            url as NSURL
        }
        func previewControllerDidDismiss(_ controller: QLPreviewController) { onFinished() }
    }
}

private struct ShareSheetNativePresenter: UIViewControllerRepresentable {
    let url: URL
    let onFinished: @MainActor (NativeFilePresentationResult) -> Void
    func makeUIViewController(context: Context) -> UIActivityViewController {
        let controller = UIActivityViewController(activityItems: [url], applicationActivities: nil)
        if let popover = controller.popoverPresentationController {
            popover.sourceView = controller.view
            popover.sourceRect = CGRect(x: 0, y: 0, width: 1, height: 1)
            popover.permittedArrowDirections = []
        }
        controller.completionWithItemsHandler = { _, completed, _, _ in
            Task { @MainActor in onFinished(completed ? .shareCompleted : .shareCancelled) }
        }
        return controller
    }
    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}

private struct FilesExportNativePresenter: UIViewControllerRepresentable {
    let url: URL
    let onFinished: @MainActor (NativeFilePresentationResult) -> Void
    func makeCoordinator() -> Coordinator { Coordinator(onFinished: onFinished) }
    func makeUIViewController(context: Context) -> UIDocumentPickerViewController {
        let picker = UIDocumentPickerViewController(forExporting: [url], asCopy: true)
        picker.delegate = context.coordinator
        return picker
    }
    func updateUIViewController(_ controller: UIDocumentPickerViewController, context: Context) {}
    final class Coordinator: NSObject, UIDocumentPickerDelegate {
        let onFinished: @MainActor (NativeFilePresentationResult) -> Void
        init(onFinished: @escaping @MainActor (NativeFilePresentationResult) -> Void) { self.onFinished = onFinished }
        func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
            onFinished(urls.isEmpty ? .saveCancelled : .saveReportedCompleted)
        }
        func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { onFinished(.saveCancelled) }
    }
}

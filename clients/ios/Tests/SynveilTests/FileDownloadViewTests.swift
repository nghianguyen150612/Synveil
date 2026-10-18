import SwiftUI
import UIKit
import XCTest

@testable import Synveil

@MainActor
final class FileDownloadViewTests: XCTestCase {
    func testBackgroundDownloadFailsClosedAndForegroundDownloadRemainsAvailable() async throws {
        let f = try await downloadFixture(self)
        let model = FileDownloadViewModel(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)

        XCTAssertFalse(BackgroundDownloadCapability.current.isAvailable)
        XCTAssertEqual(
            BackgroundDownloadCapability.current,
            .unavailable(.deploymentRedirectPolicyUnverified))
        XCTAssertTrue(model.canStart)
        XCTAssertTrue(
            BackgroundDownloadCapability.current.userMessage.contains("cannot prove"))
        let requests = await f.content.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    func testUnavailableDownloadStorageIsNotPresentedAsEmptyHistory() async throws {
        let f = try await downloadFixture(self)
        let model = FileDownloadViewModel(
            coordinator: nil, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)

        await model.loadSaved()

        XCTAssertFalse(model.canStart)
        XCTAssertTrue(model.savedStatusUnavailableMessage?.contains("unavailable") == true)
    }

    func testDownloadControlsRenderAtStandardSize() async throws {
        try await render(size: .large)
    }
    func testDownloadControlsRenderAtLargestAccessibilitySize() async throws {
        try await render(size: .accessibility5)
    }
    private func render(size: DynamicTypeSize) async throws {
        let f = try await downloadFixture(self)
        let controls = FileDownloadControls(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
        let host = UIHostingController(
            rootView: List { controls }.environment(\.dynamicTypeSize, size))
        host.loadViewIfNeeded()
        host.view.frame = CGRect(x: 0, y: 0, width: 320, height: 1000)
        host.view.layoutIfNeeded()
        let image = UIGraphicsImageRenderer(size: host.view.bounds.size).image { context in
            host.view.layer.render(in: context.cgContext)
        }
        XCTAssertEqual(image.size.width, 320)
        XCTAssertEqual(image.size.height, 1000)
        let requests = await f.content.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testDownloadedStatusRendersAfterVerification() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let controls = FileDownloadControls(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
        let host = UIHostingController(
            rootView: List { controls }.environment(\.dynamicTypeSize, .accessibility5))
        host.loadViewIfNeeded()
        host.view.frame = CGRect(x: 0, y: 0, width: 320, height: 1000)
        host.view.layoutIfNeeded()
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.state, .verified)
        XCTAssertEqual(record.state.label, "Downloaded and Verified")
    }

    func testRestoredDownloadingRecordShowsInterruptedAndDoesNotReplay() async throws {
        let body = Data(repeating: 97, count: DownloadPolicy.bufferSize * 2 + 3)
        let f = try await downloadFixture(self, body: body)
        await f.content.configure(failAfterChunks: 1)
        guard case .stopped(_, .timeout) = await downloadRun(f) else {
            return XCTFail("The deterministic fixture must stop after one committed chunk")
        }
        let partial = try await downloadRecord(f)
        try await f.queue.database.updateDownload(partial, state: .downloading)
        let requestsBeforeRestore = await f.content.requests().count

        let model = FileDownloadViewModel(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
        await model.loadSaved()

        XCTAssertEqual(model.progress?.state, .downloading)
        XCTAssertEqual(model.label, "Download interrupted — Resume available")
        XCTAssertTrue(model.canResume)
        let recovery = try XCTUnwrap(
            ClientRecoveryPresentation.download(model.progress!, isActive: false))
        XCTAssertEqual(recovery.classification, .transferInterrupted)
        XCTAssertTrue(recovery.message.contains("not available offline"))
        XCTAssertTrue(recovery.actions.contains(.resumeDownload))
        let requestsAfterRestore = await f.content.requests()
        XCTAssertEqual(requestsAfterRestore.count, requestsBeforeRestore)
    }

    func testLifecycleSuspensionCancelsForegroundDownloadAndReloadsDurableState() async throws {
        let body = Data(repeating: 97, count: DownloadPolicy.bufferSize * 2 + 3)
        let f = try await downloadFixture(self, body: body)
        let gate = QueueGate()
        await f.content.configure(gate: gate)
        let model = FileDownloadViewModel(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, node: f.node.id, version: f.version)
        let operation = Task { await model.download() }
        await gate.wait()

        f.coordinator.suspendForeground(lifecycleGeneration: 1)
        f.coordinator.updateLifecycle(lifecycleGeneration: 2, isActive: true)
        await gate.release()
        await operation.value

        let saved = try await downloadRecord(f)
        XCTAssertEqual(saved.state, .cancelled)
        XCTAssertEqual(model.progress?.state, .cancelled)
        XCTAssertEqual(model.label, "Download interrupted — Resume available")
        XCTAssertFalse(model.isRunning)
        let requests = await f.content.requests()
        XCTAssertEqual(requests.count, 1)
    }
}

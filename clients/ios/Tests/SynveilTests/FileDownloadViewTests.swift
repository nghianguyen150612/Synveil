import SwiftUI
import UIKit
import XCTest

@testable import Synveil

@MainActor
final class FileDownloadViewTests: XCTestCase {
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
}

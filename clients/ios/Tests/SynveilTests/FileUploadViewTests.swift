import SwiftUI
import UIKit
import XCTest

@testable import Synveil

@MainActor
final class FileUploadViewTests: XCTestCase {
    func testUploadControlsRenderAtStandardAndAccessibilityDynamicType() async throws {
        let fixture = try await queueFixture(self)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        try await render(
            FileUploadControls(
                coordinator: nil, controller: fixture.controller,
                library: fixture.scope.libraryId, libraryStatus: .active,
                createParent: parent),
            size: .large)
        try await render(
            FileUploadControls(
                coordinator: nil, controller: fixture.controller,
                library: fixture.scope.libraryId, libraryStatus: .readOnly,
                createParent: parent),
            size: .accessibility5)
    }

    func testReplaceControlsCarryTheLiveNodeRevisionInTheirConfirmation() async throws {
        let fixture = try await queueFixture(self)
        let nodeId = try await NodeId.validated(queueUUID(3), using: QueueValidator())
        let parentId = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let node = Node(
            id: nodeId, libraryId: fixture.scope.libraryId, parentId: parentId,
            currentVersionId: nil, revision: try NodeRevision(validating: "17"),
            name: "long Unicode report 📄.txt", kind: .file, state: .active,
            createdAt: Date(timeIntervalSince1970: 1), updatedAt: Date(timeIntervalSince1970: 2),
            trashedAt: nil, restoreDeadline: nil, purgeEligible: false)
        let controls = FileUploadControls(
            coordinator: nil, controller: fixture.controller,
            library: fixture.scope.libraryId, libraryStatus: .active, replaceNode: node)
        try await render(controls, size: .accessibility5)
        XCTAssertEqual(node.revision.rawValue, "17")
        XCTAssertEqual(node.name, "long Unicode report 📄.txt")
    }

    private func render(_ controls: FileUploadControls, size: DynamicTypeSize) async throws {
        let host = UIHostingController(
            rootView: List { controls }.environment(\.dynamicTypeSize, size))
        host.loadViewIfNeeded()
        host.view.frame = CGRect(x: 0, y: 0, width: 320, height: 1000)
        host.view.layoutIfNeeded()
        XCTAssertEqual(host.view.bounds.width, 320)
        XCTAssertEqual(host.view.bounds.height, 1000)
    }
}

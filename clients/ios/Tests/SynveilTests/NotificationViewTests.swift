import SwiftUI
import UIKit
import XCTest

@testable import Synveil

@MainActor
final class NotificationViewTests: XCTestCase {
    func testSettingsRenderOnSmallScreen() async throws {
        try await render(.large, status: .authorized)
    }
    func testSettingsRenderWithLargestDynamicType() async throws {
        try await render(.accessibility5, status: .authorized)
    }
    func testDeniedPermissionSettingsRender() async throws {
        try await render(.accessibility5, status: .denied)
    }
    func testDisabledCategoriesRemainOffDuringRendering() async throws {
        try await render(.large, status: .notDetermined)
    }
    private func render(_ size: DynamicTypeSize, status: NotificationAuthorization) async throws {
        let f = try notificationFixture(self, enabled: false)
        f.center.status = status
        await f.coordinator.refreshStatus()
        let host = UIHostingController(
            rootView: NavigationStack {
                NotificationSettingsView(coordinator: f.coordinator).environment(
                    \.dynamicTypeSize, size)
            })
        host.loadViewIfNeeded()
        host.view.frame = CGRect(x: 0, y: 0, width: 320, height: 568)
        host.view.layoutIfNeeded()
        let image = UIGraphicsImageRenderer(size: host.view.bounds.size).image { context in
            host.view.layer.render(in: context.cgContext)
        }
        XCTAssertEqual(image.size.width, 320)
        XCTAssertEqual(f.coordinator.authorization, status)
        XCTAssertEqual(f.center.prompts, 0)
        XCTAssertTrue(f.coordinator.policy.categories.isEmpty)
    }
}

import SwiftUI
import XCTest

@testable import Synveil

@MainActor
final class SynveilBootstrapTests: XCTestCase {
    func testBootstrapViewCanBeInstantiated() {
        let view = BootstrapView()
        XCTAssertNotNil(view.body)
    }

    func testDelegateAndSceneLifecycleEventsShareOneCallbackFence() {
        let coordinator = AppLifecycleRecoveryCoordinator()
        var transitions: [String] = []
        let observe: @MainActor (UInt64, Bool) -> Void = {
            transitions.append("\($0):\($1)")
        }

        coordinator.receive(.active, lifecycleChanged: observe)
        coordinator.receive(.inactive, lifecycleChanged: observe)
        coordinator.receive(.inactive, lifecycleChanged: observe)
        coordinator.receive(.background, lifecycleChanged: observe)

        XCTAssertEqual(transitions, ["1:true", "2:false", "3:false"])
        XCTAssertEqual(coordinator.generation, 3)
        XCTAssertEqual(coordinator.phase, .background)

        coordinator.receive(.active, lifecycleChanged: observe)
        coordinator.receive(.inactive, lifecycleChanged: observe)
        XCTAssertEqual(transitions, ["1:true", "2:false", "3:false", "4:true", "5:false"])
        XCTAssertEqual(coordinator.generation, 5)
    }
}

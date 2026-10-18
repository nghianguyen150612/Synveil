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
        let delivery = AppLifecycleEventDelivery()
        var transitions: [String] = []
        let observe: @MainActor (UInt64, Bool) -> Void = {
            transitions.append("\($0):\($1)")
        }

        delivery.receive(.active)  // UIApplicationDelegate can arrive before dependency install.
        delivery.install { phase in coordinator.receive(phase, lifecycleChanged: observe) }
        delivery.receive(.inactive)  // Scene transition.
        delivery.receive(.inactive)  // Duplicate delegate/scene delivery.
        delivery.receive(.background)

        XCTAssertEqual(transitions, ["1:true", "2:false", "3:false"])
        XCTAssertEqual(coordinator.generation, 3)
        XCTAssertEqual(coordinator.phase, .background)

        delivery.receive(.active)
        delivery.receive(.inactive)
        XCTAssertEqual(transitions, ["1:true", "2:false", "3:false", "4:true", "5:false"])
        XCTAssertEqual(coordinator.generation, 5)
    }
}

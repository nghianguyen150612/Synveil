import SwiftUI
import XCTest
@testable import Synveil

final class RootViewTests: XCTestCase {
    @MainActor
    func testRootViewCanBeInstantiatedForAllStates() {
        let states: [AppStartupState] = [
            .initializing,
            .needsServerProfile,
            .readyForServerValidation,
            .needsEnrollment,
            .authenticated,
            .recoveryRequired(.configuration),
            .recoveryRequired(.authentication),
            .recoveryRequired(.deviceRevoked),
            .recoveryRequired(.secureStore),
            .recoveryRequired(.enrollmentAmbiguous),
            .recoveryRequired(.transport),
        ]

        for state in states {
            let controller = SessionController()
            configure(controller, for: state)

            let view = RootView(sessionController: controller)
            XCTAssertNotNil(view.body, "Failed to render RootView for state: \(state)")
        }
    }

    @MainActor
    private func configure(_ controller: SessionController, for state: AppStartupState) {
        switch state {
        case .initializing:
            break
        case .needsServerProfile:
            controller.showServerProfileSetup()
        case .readyForServerValidation:
            controller.showServerProfileSetup()
            controller.markServerReadyForValidation()
        case .needsEnrollment:
            controller.showServerProfileSetup()
            controller.markServerReadyForValidation()
            controller.requireEnrollment()
        case .authenticated:
            controller.showServerProfileSetup()
            controller.markServerReadyForValidation()
            controller.requireEnrollment()
            controller.markAuthenticated()
        case .recoveryRequired(let reason):
            controller.requireRecovery(reason)
        }
    }
    }
}

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
            .recoveryRequired(.credential),
            .recoveryRequired(.scopeMismatch),
            .recoveryRequired(.tls),
            .recoveryRequired(.protocolFailure),
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
    func testRestorationVerificationPendingSurfaceCanBeInstantiated() async {
        let controller = SessionController(
            restorationService: ImmediateRestorationService(result: .cancelled)
        )

        await controller.start()

        XCTAssertEqual(controller.state, .restorationVerificationPending)
        XCTAssertNotNil(RootView(sessionController: controller).body)
        XCTAssertNotNil(RestorationVerificationPendingView(sessionController: controller).body)
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
        case .restorationVerificationPending:
            controller.requireRecovery(.transport)
        case .authenticated:
            controller.showServerProfileSetup()
            let endpoint = try! ServerEndpoint(validating: "https://root.synveil.example")
            controller.configureServerEndpoint(endpoint)
            controller.requireEnrollment()
            controller.markAuthenticated(after: makeReceipt(for: endpoint))
        case .logoutInProgress, .logoutCleanupRequired:
            break
        case .recoveryRequired(let reason):
            controller.requireRecovery(reason)
        }
    }

    @MainActor
    private func makeReceipt(for endpoint: ServerEndpoint) -> SecureCredentialPersistenceReceipt {
        let record = try! DeviceCredentialRecord(
            ownerUserId: "11111111-2222-3333-4444-555555555555",
            deviceId: "66666666-7777-8888-9999-000000000000",
            credentialId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            credential: DeviceCredential(
                validatedRawValue:
                    "svd1_abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
            ),
            createdAt: "2026-10-07T12:00:00Z"
        )
        return SecureCredentialPersistenceReceipt(
            session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)
        )
    }
}

private struct ImmediateRestorationService: SessionRestorationServiceProtocol {
    let result: SessionRestorationResult

    func restore(configuredServerEndpoint: ServerEndpoint?) async -> SessionRestorationResult {
        result
    }

    func retryVerification(
        of session: DeviceCredentialSession,
        expectedServerEndpoint: ServerEndpoint
    ) async -> SessionRestorationResult {
        result
    }
}

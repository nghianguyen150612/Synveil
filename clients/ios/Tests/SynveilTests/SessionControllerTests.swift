import XCTest
@testable import Synveil

final class SessionControllerTests: XCTestCase {
    @MainActor
    func testInitialStateIsInitializing() {
        let controller = SessionController()
        XCTAssertEqual(controller.state, .initializing)
    }

    @MainActor
    func testStartupWithoutServerEndpointResolvesToNeedsServerProfile() async {
        let configuration = AppConfiguration(serverEndpoint: nil)
        let controller = SessionController(configuration: configuration)

        await controller.start()

        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testStartupWithServerEndpointResolvesToReadyForServerValidation() async {
        let endpoint = try! ServerEndpoint(validating: "https://sync.synveil.internal:8443")
        let configuration = AppConfiguration(serverEndpoint: endpoint)
        let controller = SessionController(configuration: configuration)

        await controller.start()

        XCTAssertEqual(controller.state, .readyForServerValidation)
    }

    @MainActor
    func testStartupIsIdempotent() async {
        let configuration = AppConfiguration(serverEndpoint: nil)
        let controller = SessionController(configuration: configuration)

        await controller.start()
        XCTAssertEqual(controller.state, .needsServerProfile)

        // Move through the valid lifecycle path before testing repeated startup.
        controller.markServerReadyForValidation()
        controller.requireEnrollment()
        XCTAssertEqual(controller.state, .needsEnrollment)

        // Second startup call should be no-op and not overwrite state.
        await controller.start()
        XCTAssertEqual(controller.state, .needsEnrollment)
    }

    @MainActor
    func testProductionStartupDoesNotFabricateAuthenticatedState() async {
        let controller = SessionController(configuration: AppConfiguration(serverEndpoint: nil))
        await controller.start()
        XCTAssertNotEqual(controller.state, .authenticated)
    }

    @MainActor
    func testConfigureServerEndpointSetsEndpointAndTransitionsState() throws {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let endpoint = try ServerEndpoint(validating: "https://synveil.example.com")
        controller.configureServerEndpoint(endpoint)

        XCTAssertEqual(controller.serverEndpoint, endpoint)
        XCTAssertEqual(controller.state, .readyForServerValidation)
    }

    @MainActor
    func testControlledTransitions() {
        let controller = SessionController()

        controller.showServerProfileSetup()
        XCTAssertEqual(controller.state, .needsServerProfile)

        controller.markServerReadyForValidation()
        XCTAssertEqual(controller.state, .readyForServerValidation)

        controller.requireEnrollment()
        XCTAssertEqual(controller.state, .needsEnrollment)

        controller.markAuthenticated()
        XCTAssertEqual(controller.state, .authenticated)

        controller.requireRecovery(.transport)
        XCTAssertEqual(controller.state, .recoveryRequired(.transport))
    }

    @MainActor
    func testInvalidTransitionsCannotBypassLifecycleGates() {
        let controller = SessionController()

        controller.markAuthenticated()
        XCTAssertEqual(controller.state, .initializing)

        controller.showServerProfileSetup()
        controller.markAuthenticated()
        XCTAssertEqual(controller.state, .needsServerProfile)

        controller.markServerReadyForValidation()
        controller.requireRecovery(.authentication)
        controller.markAuthenticated()
        XCTAssertEqual(controller.state, .recoveryRequired(.authentication))
    }
}

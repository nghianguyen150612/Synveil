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

        let endpoint = try! ServerEndpoint(validating: "https://session.synveil.example")
        controller.configureServerEndpoint(endpoint)
        XCTAssertEqual(controller.state, .readyForServerValidation)

        controller.requireEnrollment()
        XCTAssertEqual(controller.state, .needsEnrollment)

        controller.markAuthenticated(after: makeReceipt(for: endpoint))
        XCTAssertEqual(controller.state, .authenticated)

        controller.requireRecovery(.transport)
        XCTAssertEqual(controller.state, .recoveryRequired(.transport))
    }

    @MainActor
    func testInvalidTransitionsCannotBypassLifecycleGates() {
        let controller = SessionController()

        let endpoint = try! ServerEndpoint(validating: "https://session.synveil.example")
        let receipt = makeReceipt(for: endpoint)
        controller.markAuthenticated(after: receipt)
        XCTAssertEqual(controller.state, .initializing)

        controller.showServerProfileSetup()
        controller.markAuthenticated(after: receipt)
        XCTAssertEqual(controller.state, .needsServerProfile)

        controller.configureServerEndpoint(endpoint)
        controller.markAuthenticated(after: makeReceipt(
            for: try! ServerEndpoint(validating: "https://wrong.synveil.example")
        ))
        XCTAssertEqual(controller.state, .readyForServerValidation)
        controller.requireEnrollment()
        controller.requireRecovery(.authentication)
        controller.markAuthenticated(after: receipt)
        XCTAssertEqual(controller.state, .recoveryRequired(.authentication))
    }

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

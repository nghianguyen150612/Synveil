import XCTest

@testable import Synveil

final class ServerSetupViewModelTests: XCTestCase {

    @MainActor
    func testEmptyInputValidationFails() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(sessionController: controller, initialInput: "")
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(viewModel.validationError, "Please enter a server address.")
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testWhitespaceInputValidationFails() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "   \n\t "
        )
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(
            viewModel.validationError,
            "Server address must not contain whitespace or control characters."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testMissingSchemeValidationFails() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "synveil.example.com"
        )
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(
            viewModel.validationError,
            "Server address requires a URL scheme (e.g. https://synveil.example.com)."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testUnsupportedSchemeValidationFails() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "ftp://synveil.example.com"
        )
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(
            viewModel.validationError,
            "Unsupported URL scheme 'ftp'. Synveil requires HTTPS."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testUserinfoCredentialsValidationFails() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "https://user:password@synveil.example.com"
        )
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(
            viewModel.validationError,
            "Server address must not contain username or password credentials."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testQueryParametersValidationFails() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "https://synveil.example.com?token=123"
        )
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(
            viewModel.validationError,
            "Server address must not contain query parameters."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testURLFragmentValidationFails() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "https://synveil.example.com#section"
        )
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(
            viewModel.validationError,
            "Server address must not contain URL fragments."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testValidHTTPSInputSucceedsAndTransitionsState() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "https://synveil.example.com"
        )
        let result = viewModel.submit()

        XCTAssertTrue(result)
        XCTAssertNil(viewModel.validationError)
        XCTAssertEqual(controller.serverEndpoint?.urlString, "https://synveil.example.com/")
        XCTAssertEqual(controller.state, .readyForServerValidation)

        // Verify accepting server address DOES NOT mark user as authenticated
        XCTAssertNotEqual(controller.state, .authenticated)
    }

    @MainActor
    func testValidHTTPLoopbackInputSucceedsInDebugPolicy() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "http://127.0.0.1:8443",
            allowLoopbackTestHTTP: true
        )
        let result = viewModel.submit()

        XCTAssertTrue(result)
        XCTAssertNil(viewModel.validationError)
        XCTAssertEqual(controller.serverEndpoint?.urlString, "http://127.0.0.1:8443/")
        XCTAssertEqual(controller.state, .readyForServerValidation)
    }

    @MainActor
    func testLANHTTPInputIsRejectedEvenWithDebugPolicy() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "http://192.168.1.100:8443",
            allowLoopbackTestHTTP: true
        )
        XCTAssertFalse(viewModel.submit())
        XCTAssertEqual(
            viewModel.validationError,
            "Cleartext HTTP is limited to 127.0.0.1 or ::1 in debug builds. "
                + "Use HTTPS for other servers."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testWhitespaceAroundServerAddressIsRejected() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(
            sessionController: controller,
            initialInput: "   https://synveil.example.com/  \n"
        )
        let result = viewModel.submit()

        XCTAssertFalse(result)
        XCTAssertEqual(
            viewModel.validationError,
            "Server address must not contain whitespace or control characters."
        )
        XCTAssertEqual(controller.state, .needsServerProfile)
    }

    @MainActor
    func testClearsValidationErrorWhenInputChanges() {
        let controller = SessionController()
        controller.showServerProfileSetup()

        let viewModel = ServerSetupViewModel(sessionController: controller, initialInput: "")
        viewModel.submit()

        XCTAssertNotNil(viewModel.validationError)

        viewModel.serverAddressInput = "h"
        XCTAssertNil(viewModel.validationError)
    }
}

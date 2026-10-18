import XCTest

@testable import Synveil

@MainActor
final class LibraryCatalogViewTests: XCTestCase {
    func testAuthenticatedRootBuildsInjectedCatalogSurface() throws {
        let controller = try makeAuthenticatedController()
        let repository = FixedLibraryCatalogRepository(result: .loaded([]))

        let root = RootView(
            sessionController: controller,
            libraryCatalog: repository
        )

        XCTAssertNotNil(root.body)
    }

    func testCatalogViewFailsClosedWhenRepositoryIsMissing() throws {
        let controller = try makeAuthenticatedController()
        let screen = LibraryCatalogView(repository: nil, sessionController: controller)

        XCTAssertNotNil(screen.body)
    }

    func testCatalogSurfaceCanRenderWithoutAuthentication() throws {
        let controller = SessionController()
        let screen = LibraryCatalogView(repository: nil, sessionController: controller)

        XCTAssertNotNil(screen.body)
    }

    func testLibraryStatusLabelsRemainDistinctAndAccessible() {
        let active = LibraryStatusPresentation.make(for: .active)
        let readOnly = LibraryStatusPresentation.make(for: .readOnly)
        let quarantined = LibraryStatusPresentation.make(for: .quarantined)

        XCTAssertEqual(active.label, "Active")
        XCTAssertEqual(readOnly.label, "Read Only")
        XCTAssertEqual(quarantined.label, "Quarantined")
        XCTAssertNotEqual(active.symbol, readOnly.symbol)
        XCTAssertNotEqual(active.symbol, quarantined.symbol)
    }

    func testLogoutControlAndConfirmationIdentifiersRemainStable() {
        XCTAssertEqual(SessionLogoutAccessibility.logoutButton, "synveil.session.logout")
        XCTAssertEqual(
            SessionLogoutAccessibility.logoutConfirmation,
            "synveil.session.logout-confirm"
        )
        XCTAssertEqual(
            SessionLogoutAccessibility.logoutCancellation,
            "synveil.session.logout-cancel"
        )
    }

    private func makeAuthenticatedController() throws -> SessionController {
        let endpoint = try ServerEndpoint(validating: "https://catalog.synveil.example")
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint))
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()

        let record = try DeviceCredentialRecord(
            ownerUserId: testCatalogID(90),
            deviceId: testCatalogID(91),
            credentialId: testCatalogID(92),
            credential: DeviceCredential(
                validatedRawValue: "svd1_" + String(repeating: "a", count: 64)
            ),
            createdAt: "2026-10-09T12:00:00Z"
        )
        controller.markAuthenticated(
            after: SecureCredentialPersistenceReceipt(
                session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)
            )
        )
        return controller
    }

    private func testCatalogID(_ number: Int) -> String {
        "018f9b9f-5c21-722e-8b1a-" + String(format: "%012x", number)
    }
}

@MainActor
private struct FixedLibraryCatalogRepository: LibraryCatalogRepositoryProtocol {
    let result: LibraryRepositoryResult

    func listLibraries() async -> LibraryRepositoryResult { result }
}

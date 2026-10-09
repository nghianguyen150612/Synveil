import Foundation
import XCTest

@testable import Synveil

@MainActor
final class NodeBrowserViewTests: XCTestCase {
    func testLibrarySelectionRouteUsesLibraryRootScope() async throws {
        let library = try await makeLibrary(1)
        let route = NodeBrowserRoute.root(for: library)

        XCTAssertEqual(route.library.id, library.id)
        XCTAssertEqual(route.directoryTitle, library.name)
        XCTAssertEqual(route.parentScope, .libraryRoot(rootNodeId: library.rootNodeId))
        XCTAssertEqual(route.ancestry, [library.rootNodeId])
    }

    func testRootAndDirectoryScopesAreDifferentNavigationIdentities() async throws {
        let library = try await makeLibrary(1)
        let root = NodeBrowserRoute.root(for: library)
        let directory = NodeBrowserRoute(
            library: NodeBrowserLibraryContext(library),
            parentScope: .directory(library.rootNodeId),
            directoryTitle: library.name,
            ancestry: [library.rootNodeId]
        )

        XCTAssertNotEqual(root.parentScope, directory.parentScope)
        XCTAssertNotEqual(root, directory)
    }

    func testDirectorySelectionCreatesDirectDirectoryScopeAndKeepsLibrary() async throws {
        let library = try await makeLibrary(1)
        let route = NodeBrowserRoute.root(for: library)
        let model = makeViewModel(route: route)
        let folder = try await makeNode(
            20,
            library: library,
            parent: library.rootNodeId,
            kind: .directory,
            name: "Reports"
        )

        let destination = try XCTUnwrap(model.route(into: folder))

        XCTAssertEqual(destination.library.id, library.id)
        XCTAssertEqual(destination.parentScope, .directory(folder.id))
        XCTAssertEqual(destination.directoryTitle, "Reports")
        XCTAssertEqual(destination.ancestry, [library.rootNodeId, folder.id])
    }

    func testNestedNavigationUsesEachActualDirectoryID() async throws {
        let library = try await makeLibrary(1)
        let root = NodeBrowserRoute.root(for: library)
        let rootModel = makeViewModel(route: root)
        let first = try await makeNode(
            20,
            library: library,
            parent: library.rootNodeId,
            kind: .directory,
            name: "Archive"
        )
        let firstRoute = try XCTUnwrap(rootModel.route(into: first))
        let firstModel = makeViewModel(route: firstRoute)
        let second = try await makeNode(
            21,
            library: library,
            parent: first.id,
            kind: .directory,
            name: "Archive"
        )
        let secondRoute = try XCTUnwrap(firstModel.route(into: second))

        XCTAssertEqual(firstRoute.parentScope, .directory(first.id))
        XCTAssertEqual(secondRoute.parentScope, .directory(second.id))
        XCTAssertEqual(secondRoute.ancestry, [library.rootNodeId, first.id, second.id])
        XCTAssertNotEqual(firstRoute, secondRoute)
    }

    func testSameNamedFoldersRemainDistinctByNodeID() async throws {
        let library = try await makeLibrary(1)
        let model = makeViewModel(route: .root(for: library))
        let first = try await makeNode(
            30,
            library: library,
            parent: library.rootNodeId,
            kind: .directory,
            name: "Shared"
        )
        let second = try await makeNode(
            31,
            library: library,
            parent: library.rootNodeId,
            kind: .directory,
            name: "Shared"
        )

        let firstRoute = try XCTUnwrap(model.route(into: first))
        let secondRoute = try XCTUnwrap(model.route(into: second))

        XCTAssertEqual(firstRoute.directoryTitle, secondRoute.directoryTitle)
        XCTAssertNotEqual(firstRoute.parentScope, secondRoute.parentScope)
        XCTAssertNotEqual(firstRoute, secondRoute)
    }

    func testKnownAncestorCycleDoesNotCreateAnotherNavigationRoute() async throws {
        let library = try await makeLibrary(1)
        let rootRoute = NodeBrowserRoute.root(for: library)
        let directoryId = try await makeNodeId(20)
        let nestedRoute = NodeBrowserRoute(
            library: rootRoute.library,
            parentScope: .directory(directoryId),
            directoryTitle: "Nested",
            ancestry: [library.rootNodeId, directoryId]
        )
        let model = makeViewModel(route: nestedRoute)
        let cycle = try await makeNode(
            20,
            library: library,
            parent: directoryId,
            kind: .directory,
            name: "Root"
        )

        XCTAssertNil(model.route(into: cycle))
    }

    func testFileSelectionCarriesOnlyValidatedReadOnlyMetadata() async throws {
        let library = try await makeLibrary(1)
        let model = makeViewModel(route: .root(for: library))
        let file = try await makeNode(
            40,
            library: library,
            parent: library.rootNodeId,
            kind: .file,
            name: "notes.txt"
        )

        let details = try XCTUnwrap(model.details(for: file))

        XCTAssertEqual(details.library.id, library.id)
        XCTAssertEqual(details.parentScope, .libraryRoot(rootNodeId: library.rootNodeId))
        XCTAssertEqual(details.ancestry, [library.rootNodeId])
        XCTAssertEqual(details.parentDirectoryTitle, library.name)
        XCTAssertEqual(details.nodeId, file.id)
        XCTAssertEqual(details.name, file.name)
        XCTAssertEqual(details.revision, file.revision)
        XCTAssertEqual(details.createdAt, file.createdAt)
        XCTAssertEqual(details.updatedAt, file.updatedAt)
    }

    func testFileCannotBeUsedAsDirectoryDestination() async throws {
        let library = try await makeLibrary(1)
        let model = makeViewModel(route: .root(for: library))
        let file = try await makeNode(40, library: library, parent: library.rootNodeId)
        XCTAssertNil(model.route(into: file))
    }

    func testNodeFromAnotherLibraryCannotCreateRouteOrDetails() async throws {
        let firstLibrary = try await makeLibrary(1)
        let secondLibrary = try await makeLibrary(2)
        let model = makeViewModel(route: .root(for: firstLibrary))
        let folder = try await makeNode(
            30,
            library: secondLibrary,
            parent: secondLibrary.rootNodeId,
            kind: .directory,
            name: "Other"
        )
        let file = try await makeNode(
            31,
            library: secondLibrary,
            parent: secondLibrary.rootNodeId,
            name: "Other file"
        )

        XCTAssertNil(model.route(into: folder))
        XCTAssertNil(model.details(for: file))
    }

    func testNativeBrowserAndFileDetailsSurfacesCanBeConstructed() async throws {
        let library = try await makeLibrary(1)
        let controller = try makeAuthenticatedController()
        let browser = NodeBrowserView(
            repository: nil,
            sessionController: controller,
            route: .root(for: library)
        )
        let file = try await makeNode(40, library: library, parent: library.rootNodeId)
        let model = makeViewModel(route: .root(for: library))
        let details = NodeFileDetailsView(
            repository: nil, sessionController: controller,
            route: try XCTUnwrap(model.details(for: file)))

        XCTAssertNotNil(browser.body)
        XCTAssertNotNil(details.body)
    }

    private func makeViewModel(route: NodeBrowserRoute) -> NodeBrowserViewModel {
        NodeBrowserViewModel(
            repository: nil,
            sessionController: SessionController(),
            route: route
        )
    }

    private func makeAuthenticatedController() throws -> SessionController {
        let endpoint = try ServerEndpoint(validating: "https://folders.synveil.example")
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint)
        )
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()
        let record = try DeviceCredentialRecord(
            ownerUserId: testID(90),
            deviceId: testID(91),
            credentialId: testID(92),
            credential: DeviceCredential(
                validatedRawValue: "svd1_" + String(repeating: "c", count: 64)
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

    private func makeLibrary(_ number: Int) async throws -> Library {
        let validator = BrowserViewTestValidator()
        return Library(
            id: try await LibraryId.validated(testID(number), using: validator),
            revision: try LibraryRevision(validating: "1"),
            name: "Library \(number)",
            rootNodeId: try await NodeId.validated(testID(number + 10_000), using: validator),
            status: .active,
            createdAt: Date(timeIntervalSince1970: 1_760_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_760_000_100)
        )
    }

    private func makeNode(
        _ number: Int,
        library: Library,
        parent: NodeId,
        kind: NodeKind = .file,
        name: String = "item.bin"
    ) async throws -> Node {
        let validator = BrowserViewTestValidator()
        return Node(
            id: try await NodeId.validated(testID(number), using: validator),
            libraryId: library.id,
            parentId: parent,
            currentVersionId: nil,
            revision: try NodeRevision(validating: "2"),
            name: name,
            kind: kind,
            state: .active,
            createdAt: Date(timeIntervalSince1970: 1_760_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_760_000_100),
            trashedAt: nil,
            restoreDeadline: nil,
            purgeEligible: false
        )
    }

    private func makeNodeId(_ number: Int) async throws -> NodeId {
        try await NodeId.validated(testID(number), using: BrowserViewTestValidator())
    }

    private func testID(_ number: Int) -> String {
        "018f9b9f-5c21-722e-8b1a-" + String(format: "%012x", number)
    }
}

private struct BrowserViewTestValidator: RustBridgeProtocol {
    func parseSHA256(_ canonical: String) async throws -> Data { Data() }
    func formatSHA256(_ digest: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ token: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ token: String) async throws -> Bool { false }
    func validateLibraryID(_ value: String) async throws -> Bool { isCanonicalID(value) }
    func validateNodeID(_ value: String) async throws -> Bool { isCanonicalID(value) }
    func validateLogicalName(_ value: String) async throws -> Bool {
        !value.isEmpty && value.utf8.count <= 1024
    }

    private func isCanonicalID(_ value: String) -> Bool {
        LibraryWireValidation.matches(
            value,
            pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
        )
    }
}

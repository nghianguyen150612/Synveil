import Foundation
import XCTest

@testable import Synveil

@MainActor
final class NodeBrowserViewModelTests: XCTestCase {
    func testInitialStateIsIdle() async throws {
        let fixture = try await makeFixture()
        XCTAssertEqual(fixture.viewModel.state, .idle)
        XCTAssertNil(fixture.viewModel.visibleNodes)
    }

    func testAuthenticatedDirectoryLoadUsesInjectedRepositoryAndScope() async throws {
        let library = try await makeLibrary(1)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(library: library, results: [.loaded([node])])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.repository.requestCount, 1)
        XCTAssertEqual(fixture.repository.requests.first?.libraryId, library.id)
        XCTAssertEqual(
            fixture.repository.requests.first?.parent,
            .libraryRoot(rootNodeId: library.rootNodeId)
        )
        XCTAssertEqual(fixture.viewModel.state, .loaded([node]))
    }

    func testSuspendedInitialRequestPublishesLoadingState() async throws {
        let fixture = try await makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        XCTAssertEqual(fixture.viewModel.state, .loading)

        fixture.repository.complete(request: 1, with: .loaded([]))
        await load.value
        XCTAssertEqual(fixture.viewModel.state, .empty)
    }

    func testSuccessfulEmptyResponseHasDistinctEmptyState() async throws {
        let fixture = try await makeFixture(results: [.loaded([])])
        await fixture.viewModel.loadIfNeeded()
        XCTAssertEqual(fixture.viewModel.state, .empty)
        XCTAssertEqual(fixture.viewModel.visibleNodes, [])
    }

    func testInitialNetworkFailureIsTypedAndRetryable() async throws {
        let fixture = try await makeFixture(results: [.failed(.offline)])
        await fixture.viewModel.loadIfNeeded()
        XCTAssertEqual(fixture.viewModel.state, .failed(.node(.offline)))
        XCTAssertTrue(fixture.viewModel.canRefresh)
    }

    func testDuplicateInitialLoadsAreSuppressed() async throws {
        let fixture = try await makeFixture(suspendedRequests: [1])
        let first = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)
        await fixture.viewModel.loadIfNeeded()
        XCTAssertEqual(fixture.repository.requestCount, 1)

        fixture.repository.complete(request: 1, with: .loaded([]))
        await first.value
        XCTAssertEqual(fixture.viewModel.state, .empty)
    }

    func testRefreshRequestsOnlyTheCurrentLibraryAndParentScope() async throws {
        let library = try await makeLibrary(1)
        let initial = try await makeNode(11, library: library, parent: library.rootNodeId)
        let refreshed = try await makeNode(12, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(
            library: library,
            results: [.loaded([initial]), .loaded([refreshed])]
        )

        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertEqual(fixture.repository.requestCount, 2)
        XCTAssertEqual(fixture.repository.requests[1].libraryId, library.id)
        XCTAssertEqual(
            fixture.repository.requests[1].parent,
            .libraryRoot(rootNodeId: library.rootNodeId)
        )
        XCTAssertEqual(fixture.viewModel.state, .loaded([refreshed]))
    }

    func testDuplicateRefreshIsSuppressed() async throws {
        let fixture = try await makeFixture(
            results: [.loaded([])], suspendedRequests: [2]
        )
        await fixture.viewModel.loadIfNeeded()
        let refresh = Task { await fixture.viewModel.refresh() }
        await fixture.repository.waitForRequestCount(2)
        await fixture.viewModel.refresh()
        XCTAssertEqual(fixture.repository.requestCount, 2)

        fixture.repository.complete(request: 2, with: .loaded([]))
        await refresh.value
        XCTAssertEqual(fixture.viewModel.state, .empty)
    }

    func testSuccessfulRefreshReplacesPreviouslyLoadedNodes() async throws {
        let library = try await makeLibrary(1)
        let oldNode = try await makeNode(11, library: library, parent: library.rootNodeId)
        let newNode = try await makeNode(12, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(
            library: library,
            results: [.loaded([oldNode]), .loaded([newNode])]
        )

        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertEqual(fixture.viewModel.state, .loaded([newNode]))
    }

    func testTransientRefreshFailureRetainsPriorNodesAsUnverified() async throws {
        let library = try await makeLibrary(1)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(
            library: library,
            results: [.loaded([node]), .failed(.timeout)]
        )

        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertEqual(fixture.viewModel.state, .refreshFailed([node], .node(.timeout)))
        XCTAssertEqual(fixture.viewModel.visibleNodes, [node])
        XCTAssertFalse(
            String(describing: fixture.viewModel.state).localizedCaseInsensitiveContains("cache")
        )
    }

    func testTransientFailureRetainsPreviouslyEmptyDirectoryState() async throws {
        let fixture = try await makeFixture(results: [.loaded([]), .failed(.serverUnavailable)])
        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()
        XCTAssertEqual(fixture.viewModel.state, .refreshFailed([], .node(.serverUnavailable)))
    }

    func testNonTransientRefreshFailureClearsPriorNodes() async throws {
        let library = try await makeLibrary(1)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(
            library: library,
            results: [.loaded([node]), .failed(.protocolFailure)]
        )

        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertEqual(fixture.viewModel.state, .failed(.node(.protocolFailure)))
        XCTAssertNil(fixture.viewModel.visibleNodes)
    }

    func testHTTP503CanRetryAndRetainsSameSessionResults() async throws {
        let library = try await makeLibrary(1)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(
            library: library,
            results: [.loaded([node]), .failed(.httpFailure(statusCode: 503))]
        )

        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()

        XCTAssertEqual(
            fixture.viewModel.state,
            .refreshFailed([node], .node(.httpFailure(statusCode: 503)))
        )
        XCTAssertTrue(fixture.viewModel.canRefresh)
        XCTAssertEqual(fixture.sessionController.state, .authenticated)
    }

    func testCancellationIsNotPresentedAsAnEmptyDirectory() async throws {
        let fixture = try await makeFixture(results: [.failed(.cancelled)])
        await fixture.viewModel.loadIfNeeded()
        XCTAssertEqual(fixture.viewModel.state, .cancelled)
        XCTAssertNil(fixture.viewModel.visibleNodes)
    }

    func testCancelledRefreshRetainsDataWithCancellationState() async throws {
        let library = try await makeLibrary(1)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(
            library: library,
            results: [.loaded([node]), .failed(.cancelled)]
        )
        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()
        XCTAssertEqual(fixture.viewModel.state, .refreshCancelled([node]))
    }

    func testLogoutInvalidatesDirectoryModelAndCancelsPendingRepositoryWork() async throws {
        let fixture = try await makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        await fixture.sessionController.requestLogout()
        fixture.viewModel.sessionDidChange()
        XCTAssertEqual(fixture.viewModel.state, .invalidated)

        fixture.repository.complete(request: 1, with: .loaded([]))
        await load.value
        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        XCTAssertNil(fixture.viewModel.visibleNodes)
        XCTAssertEqual(fixture.repository.wasCancelled(request: 1), true)
    }

    func testLateSuccessAfterLogoutCannotRestoreNodeData() async throws {
        let library = try await makeLibrary(1)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        await fixture.sessionController.requestLogout()
        fixture.viewModel.sessionDidChange()
        fixture.repository.complete(request: 1, with: .loaded([node]))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
        XCTAssertNil(fixture.viewModel.visibleNodes)
    }

    func testLateFailureAfterLogoutCannotReplaceInvalidatedState() async throws {
        let fixture = try await makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        await fixture.sessionController.requestLogout()
        fixture.viewModel.sessionDidChange()
        fixture.repository.complete(request: 1, with: .failed(.offline))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
    }

    func testSessionRecoveryInvalidatesPendingRequest() async throws {
        let fixture = try await makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)

        fixture.sessionController.requireRecovery(.scopeMismatch)
        fixture.viewModel.sessionDidChange()
        fixture.repository.complete(request: 1, with: .loaded([]))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .invalidated)
    }

    func testOfflineFailureDoesNotChangeAuthenticatedSession() async throws {
        let fixture = try await makeFixture(results: [.failed(.offline)])
        await fixture.viewModel.loadIfNeeded()
        XCTAssertEqual(fixture.sessionController.state, .authenticated)
        XCTAssertEqual(fixture.viewModel.state, .failed(.node(.offline)))
    }

    func testAuthenticationFailureCopyUsesAuthoritativeRecoveryLanguage() async throws {
        let presentation = NodeBrowserUIFailure.node(.authenticationRejected)
        XCTAssertTrue(presentation.message.contains("recovery flow"))
        XCTAssertFalse(presentation.canRetry)
    }

    func testMissingRepositoryFailsClosedWithoutSyntheticNodes() async throws {
        let controller = try makeAuthenticatedController()
        let library = try await makeLibrary(1)
        let viewModel = NodeBrowserViewModel(
            repository: nil,
            sessionController: controller,
            route: .root(for: library)
        )

        await viewModel.loadIfNeeded()

        XCTAssertEqual(viewModel.state, .failed(.repositoryUnavailable))
        XCTAssertNil(viewModel.visibleNodes)
        XCTAssertFalse(viewModel.canRefresh)
    }

    func testUIStateAndErrorCopyContainNoBearerOrRawResponseText() async throws {
        let fixture = try await makeFixture(results: [.failed(.httpFailure(statusCode: 503))])
        await fixture.viewModel.loadIfNeeded()
        let stateText = String(describing: fixture.viewModel.state)
        let copy = NodeBrowserUIFailure.node(.httpFailure(statusCode: 503)).message

        XCTAssertFalse(stateText.contains("svd1_"))
        XCTAssertFalse(copy.contains("svd1_"))
        XCTAssertFalse(copy.contains("Authorization"))
        XCTAssertFalse(copy.localizedCaseInsensitiveContains("response body"))
    }

    func testPresentationOrderingPutsFoldersFirstAndSortsNumericNames() async throws {
        let library = try await makeLibrary(1)
        let file10 = try await makeNode(
            10, library: library, parent: library.rootNodeId, name: "File 10"
        )
        let folder2 = try await makeNode(
            2, library: library, parent: library.rootNodeId, kind: .directory, name: "Folder 2"
        )
        let file2 = try await makeNode(
            3, library: library, parent: library.rootNodeId, name: "file 2"
        )
        let folder1 = try await makeNode(
            4, library: library, parent: library.rootNodeId, kind: .directory, name: "folder 1"
        )

        let ordered = NodeBrowserViewModel.presentationOrder([file10, folder2, file2, folder1])

        XCTAssertEqual(ordered.map(\.id), [folder1.id, folder2.id, file2.id, file10.id])
    }

    func testDuplicateNamesRemainDistinctByCanonicalNodeID() async throws {
        let library = try await makeLibrary(1)
        let first = try await makeNode(
            11, library: library, parent: library.rootNodeId, name: "Shared"
        )
        let second = try await makeNode(
            12, library: library, parent: library.rootNodeId, name: "Shared"
        )
        let ordered = NodeBrowserViewModel.presentationOrder([second, first])
        XCTAssertEqual(ordered.map(\.id), [first.id, second.id])
        XCTAssertNotEqual(ordered[0].id, ordered[1].id)
    }

    func testLongUnicodeNameRemainsExactInVisibleData() async throws {
        let library = try await makeLibrary(1)
        let name = String(repeating: "档案🚀", count: 95)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId, name: name)
        let fixture = try await makeFixture(library: library, results: [.loaded([node])])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.viewModel.visibleNodes?.first?.name, name)
        XCTAssertEqual(fixture.viewModel.visibleNodes?.first?.id, node.id)
    }

    func testWrongLibraryResultFailsClosedAsProtocolError() async throws {
        let library = try await makeLibrary(1)
        let otherLibrary = try await makeLibrary(2)
        let node = try await makeNode(
            11,
            library: otherLibrary,
            parent: library.rootNodeId
        )
        let fixture = try await makeFixture(library: library, results: [.loaded([node])])

        await fixture.viewModel.loadIfNeeded()

        XCTAssertEqual(fixture.viewModel.state, .failed(.node(.protocolFailure)))
        XCTAssertNil(fixture.viewModel.visibleNodes)
    }

    func testDistinctLibrariesAndParentScopesUseIndependentModels() async throws {
        let firstLibrary = try await makeLibrary(1)
        let secondLibrary = try await makeLibrary(2)
        let controller = try makeAuthenticatedController()
        let repository = ControlledNodeRepository(
            results: [.loaded([]), .loaded([])],
            suspendedRequests: []
        )
        let firstModel = NodeBrowserViewModel(
            repository: repository,
            sessionController: controller,
            route: .root(for: firstLibrary)
        )
        let secondRoute = NodeBrowserRoute(
            library: NodeBrowserLibraryContext(secondLibrary),
            parentScope: .directory(secondLibrary.rootNodeId),
            directoryTitle: "Other folder",
            ancestry: [secondLibrary.rootNodeId]
        )
        let secondModel = NodeBrowserViewModel(
            repository: repository,
            sessionController: controller,
            route: secondRoute
        )

        await firstModel.loadIfNeeded()
        await secondModel.loadIfNeeded()

        XCTAssertEqual(repository.requestCount, 2)
        XCTAssertEqual(repository.requests[0].libraryId, firstLibrary.id)
        XCTAssertEqual(
            repository.requests[0].parent,
            .libraryRoot(rootNodeId: firstLibrary.rootNodeId)
        )
        XCTAssertEqual(repository.requests[1].libraryId, secondLibrary.id)
        XCTAssertEqual(repository.requests[1].parent, .directory(secondLibrary.rootNodeId))
        XCTAssertEqual(firstModel.state, .empty)
        XCTAssertEqual(secondModel.state, .empty)
    }

    func testLoadTaskCancellationDoesNotBecomeSuccessfulEmptyResult() async throws {
        let fixture = try await makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)
        load.cancel()
        fixture.repository.complete(request: 1, with: .loaded([]))
        await load.value

        XCTAssertEqual(fixture.viewModel.state, .cancelled)
    }

    func testManualInvalidationDiscardsLateResult() async throws {
        let fixture = try await makeFixture(suspendedRequests: [1])
        let load = Task { await fixture.viewModel.loadIfNeeded() }
        await fixture.repository.waitForRequestCount(1)
        fixture.viewModel.invalidate()
        fixture.repository.complete(request: 1, with: .loaded([]))
        await load.value
        XCTAssertEqual(fixture.viewModel.state, .invalidated)
    }

    func testPermanentCredentialFailureDoesNotOfferRetryOrRetainData() async throws {
        let library = try await makeLibrary(1)
        let node = try await makeNode(11, library: library, parent: library.rootNodeId)
        let fixture = try await makeFixture(
            library: library,
            results: [.loaded([node]), .failed(.staleSession)]
        )
        await fixture.viewModel.loadIfNeeded()
        await fixture.viewModel.refresh()
        XCTAssertEqual(fixture.viewModel.state, .failed(.node(.staleSession)))
        XCTAssertNil(fixture.viewModel.visibleNodes)
        XCTAssertFalse(fixture.viewModel.canRefresh)
    }

    private func makeFixture(
        library: Library? = nil,
        results: [NodeRepositoryResult] = [],
        suspendedRequests: Set<Int> = []
    ) async throws -> NodeFixture {
        let chosenLibrary: Library
        if let library {
            chosenLibrary = library
        } else {
            chosenLibrary = try await makeLibrary(1)
        }
        let controller = try makeAuthenticatedController()
        let repository = ControlledNodeRepository(
            results: results,
            suspendedRequests: suspendedRequests
        )
        let viewModel = NodeBrowserViewModel(
            repository: repository,
            sessionController: controller,
            route: .root(for: chosenLibrary)
        )
        return NodeFixture(
            sessionController: controller,
            repository: repository,
            viewModel: viewModel
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
                validatedRawValue: "svd1_" + String(repeating: "b", count: 64)
            ),
            createdAt: "2026-10-09T12:00:00Z"
        )
        controller.markAuthenticated(
            after: SecureCredentialPersistenceReceipt(
                session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)
            )
        )
        XCTAssertEqual(controller.state, .authenticated)
        return controller
    }

    private func makeLibrary(_ number: Int) async throws -> Library {
        let validator = BrowserTestValidator()
        return Library(
            id: try await LibraryId.validated(testID(number), using: validator),
            revision: try LibraryRevision(validating: "42"),
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
        name: String? = nil
    ) async throws -> Node {
        let validator = BrowserTestValidator()
        return Node(
            id: try await NodeId.validated(testID(number), using: validator),
            libraryId: library.id,
            parentId: parent,
            currentVersionId: nil,
            revision: try NodeRevision(validating: "7"),
            name: name ?? "Item \(number)",
            kind: kind,
            state: .active,
            createdAt: Date(timeIntervalSince1970: 1_760_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_760_000_100),
            trashedAt: nil,
            restoreDeadline: nil,
            purgeEligible: false
        )
    }

    private func testID(_ number: Int) -> String {
        "018f9b9f-5c21-722e-8b1a-" + String(format: "%012x", number)
    }
}

@MainActor
private struct NodeFixture {
    let sessionController: SessionController
    let repository: ControlledNodeRepository
    let viewModel: NodeBrowserViewModel
}

@MainActor
private final class ControlledNodeRepository: NodeRepositoryProtocol {
    struct Request {
        let libraryId: LibraryId
        let parent: NodeParentScope
    }

    private var queuedResults: [NodeRepositoryResult]
    private var suspendedRequests: Set<Int>
    private var responseContinuations: [Int: CheckedContinuation<NodeRepositoryResult, Never>] = [:]
    private var requestWaiters: [(Int, CheckedContinuation<Void, Never>)] = []
    private var cancellationStates: [Int: Bool] = [:]
    private(set) var requests: [Request] = []

    var requestCount: Int { requests.count }

    init(results: [NodeRepositoryResult], suspendedRequests: Set<Int>) {
        queuedResults = results
        self.suspendedRequests = suspendedRequests
    }

    func listChildren(libraryId: LibraryId, parent: NodeParentScope) async -> NodeRepositoryResult {
        requests.append(Request(libraryId: libraryId, parent: parent))
        let request = requests.count
        resumeRequestWaiters()

        if suspendedRequests.remove(request) != nil {
            let result = await withCheckedContinuation {
                responseContinuations[request] = $0
            }
            cancellationStates[request] = Task.isCancelled
            return result
        }
        if !queuedResults.isEmpty { return queuedResults.removeFirst() }
        return .failed(.offline)
    }

    func waitForRequestCount(_ count: Int) async {
        if requestCount >= count { return }
        await withCheckedContinuation { continuation in
            requestWaiters.append((count, continuation))
        }
    }

    func complete(request: Int, with result: NodeRepositoryResult) {
        responseContinuations.removeValue(forKey: request)?.resume(returning: result)
    }

    func wasCancelled(request: Int) -> Bool? { cancellationStates[request] }

    private func resumeRequestWaiters() {
        let ready = requestWaiters.filter { $0.0 <= requestCount }
        requestWaiters.removeAll { $0.0 <= requestCount }
        for (_, continuation) in ready { continuation.resume() }
    }
}

private struct BrowserTestValidator: RustBridgeProtocol {
    func parseSHA256(_ canonical: String) async throws -> Data { Data() }
    func formatSHA256(_ digest: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ token: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ token: String) async throws -> Bool {
        DeviceCredential.isValid(token)
    }
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

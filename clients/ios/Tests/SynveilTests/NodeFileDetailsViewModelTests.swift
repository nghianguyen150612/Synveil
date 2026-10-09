import Foundation
import SwiftUI
import XCTest

@testable import Synveil

@MainActor
final class NodeFileDetailsViewModelTests: XCTestCase {
    func testIdleHasNoVerifiedSnapshotMetadata() async throws {
        let fixture = try await makeFixture()
        XCTAssertEqual(fixture.model.state, .idle)
        XCTAssertNil(fixture.model.visibleNode)
        XCTAssertTrue(fixture.model.canRefresh)
    }

    func testInitialLoadFetchesOnlySelectedIdentityAndScopeOnce() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        await fixture.model.loadIfNeeded()
        XCTAssertEqual(fixture.repository.requests.count, 1)
        let request = try XCTUnwrap(fixture.repository.requests.first)
        XCTAssertEqual(request.libraryId, fixture.route.library.id)
        XCTAssertEqual(request.nodeId, fixture.route.nodeId)
        XCTAssertEqual(request.parent, fixture.route.parentScope)
        XCTAssertEqual(fixture.model.state, .loaded(fixture.node))
    }

    func testLoadingHidesSnapshotAndIsNotRemotelyVerified() async throws {
        let fixture = try await makeFixture(suspended: [1])
        let load = Task { await fixture.model.loadIfNeeded() }
        await fixture.repository.waitForRequest(1)
        XCTAssertEqual(fixture.model.state, .loading)
        XCTAssertNil(fixture.model.visibleNode)
        XCTAssertFalse(fixture.model.canRefresh)
        fixture.repository.complete(1, .loaded(fixture.node))
        await load.value
        XCTAssertFalse(fixture.model.isRequestInProgress)
    }

    func testAuthoritativeNameRevisionVersionAndTimesReplaceSnapshot() async throws {
        let fixture = try await makeFixture()
        let current = try await makeNode(
            40, library: fixture.library, parent: fixture.library.rootNodeId,
            name: "report-new.txt", revision: "18446744073709551616000", version: 50)
        fixture.repository.results = [.loaded(current)]
        await fixture.model.loadIfNeeded()
        XCTAssertEqual(fixture.model.visibleNode, current)
        XCTAssertNotEqual(fixture.model.visibleNode?.name, fixture.route.name)
        XCTAssertNotEqual(fixture.model.visibleNode?.revision, fixture.route.revision)
        XCTAssertEqual(fixture.model.visibleNode?.currentVersionId?.rawValue, testID(50))
        XCTAssertEqual(fixture.route.name, "report-old.txt")
    }

    func testUnavailableIsDistinctFromNetworkFailure() async throws {
        let fixture = try await makeFixture()
        fixture.repository.results = [.unavailable]
        await fixture.model.loadIfNeeded()
        XCTAssertEqual(fixture.model.state, .unavailable)
        XCTAssertNil(fixture.model.visibleNode)
        XCTAssertTrue(fixture.model.canRefresh)
        fixture.repository.results = [.failed(.offline)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .failed(.node(.offline)))
    }

    func testRefreshUsesSameSingleNodeOperationAndSuppressesDuplicates() async throws {
        let fixture = try await makeFixture(suspended: [2])
        await fixture.model.loadIfNeeded()
        let refresh = Task { await fixture.model.refresh() }
        await fixture.repository.waitForRequest(2)
        XCTAssertEqual(fixture.model.state, .refreshing(fixture.node))
        await fixture.model.refresh()
        XCTAssertEqual(fixture.repository.requests.count, 2)
        XCTAssertEqual(fixture.repository.requests[0].nodeId, fixture.repository.requests[1].nodeId)
        fixture.repository.complete(2, .loaded(fixture.node))
        await refresh.value
        XCTAssertEqual(fixture.model.state, .loaded(fixture.node))
        XCTAssertTrue(fixture.model.canRefresh)
        XCTAssertFalse(fixture.model.isRequestInProgress)
    }

    func testInitialDuplicateLoadSuppressedWhileSuspended() async throws {
        let fixture = try await makeFixture(suspended: [1])
        let load = Task { await fixture.model.loadIfNeeded() }
        await fixture.repository.waitForRequest(1)
        await fixture.model.loadIfNeeded()
        await fixture.model.refresh()
        XCTAssertEqual(fixture.repository.requests.count, 1)
        fixture.repository.complete(1, .loaded(fixture.node))
        await load.value
    }

    func testTransientRefreshFailureRetainsOnlyPreviouslyLoadedSameSessionData() async throws {
        for failure in [
            NodeFailure.offline, .dnsFailure, .timeout, .serverUnavailable,
            .httpFailure(statusCode: 500),
        ] {
            let fixture = try await makeFixture()
            await fixture.model.loadIfNeeded()
            fixture.repository.results = [.failed(failure)]
            await fixture.model.refresh()
            XCTAssertEqual(fixture.model.state, .refreshFailed(fixture.node, .node(failure)))
            XCTAssertEqual(fixture.model.visibleNode, fixture.node)
            XCTAssertTrue(fixture.model.canRefresh)
            XCTAssertFalse(fixture.model.isRequestInProgress)
            XCTAssertEqual(fixture.controller.state, .authenticated)
        }
    }

    func testSecurityFailuresClearPreviouslyVerifiedDetails() async throws {
        for failure in [
            NodeFailure.protocolFailure, .originMismatch, .tlsFailure, .redirectRejected,
            .resourceLimit, .unexpectedContentType, .credentialUnavailable, .authenticationRejected,
            .deviceRevoked, .staleSession,
        ] {
            let fixture = try await makeFixture()
            await fixture.model.loadIfNeeded()
            fixture.repository.results = [.failed(failure)]
            await fixture.model.refresh()
            XCTAssertEqual(fixture.model.state, .failed(.node(failure)))
            XCTAssertNil(fixture.model.visibleNode)
            XCTAssertFalse(fixture.model.canRefresh)
        }
    }

    func testUnavailableAndScopeChangesClearLoadedMetadata() async throws {
        for result in [NodeDetailsRepositoryResult.unavailable, .inconsistent] {
            let fixture = try await makeFixture()
            await fixture.model.loadIfNeeded()
            fixture.repository.results = [result]
            await fixture.model.refresh()
            XCTAssertEqual(fixture.model.state, .unavailable)
            XCTAssertNil(fixture.model.visibleNode)
        }
    }

    func testMissingRepositoryFailsClosedWithoutSnapshot() async throws {
        let fixture = try await makeFixture()
        let model = NodeFileDetailsViewModel(
            repository: nil, sessionController: fixture.controller, route: fixture.route)
        await model.loadIfNeeded()
        XCTAssertEqual(model.state, .failed(.repositoryUnavailable))
        XCTAssertNil(model.visibleNode)
        XCTAssertFalse(model.canRefresh)
    }

    func testLogoutInvalidatesAndCancelsLateSuccess() async throws {
        let fixture = try await makeFixture(suspended: [1])
        let load = Task { await fixture.model.loadIfNeeded() }
        await fixture.repository.waitForRequest(1)
        await fixture.controller.requestLogout()
        fixture.model.sessionDidChange()
        XCTAssertEqual(fixture.model.state, .invalidated)
        fixture.repository.complete(1, .loaded(fixture.node))
        await load.value
        XCTAssertEqual(fixture.model.state, .invalidated)
        XCTAssertNil(fixture.model.visibleNode)
        XCTAssertEqual(fixture.repository.cancelled[1], true)
    }

    func testLogoutHidesLoadedMetadataBeforeLifecycleObserver() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        await fixture.controller.requestLogout()
        XCTAssertEqual(fixture.model.presentationState, .invalidated)
        XCTAssertNil(fixture.model.visibleNode)
        fixture.model.sessionDidChange()
        XCTAssertEqual(fixture.model.state, .invalidated)
    }

    func testCredentialSessionReplacementInvalidatesEvenWhenAuthenticated() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        let endpoint = try XCTUnwrap(fixture.controller.serverEndpoint)
        let record = try DeviceCredentialRecord(
            ownerUserId: testID(90), deviceId: testID(91), credentialId: testID(93),
            credential: DeviceCredential(
                validatedRawValue: "svd1_" + String(repeating: "d", count: 64)),
            createdAt: "2026-10-09T12:00:00Z")
        await fixture.controller.requestLogout()
        fixture.controller.requireEnrollment()
        fixture.controller.markAuthenticated(
            after: SecureCredentialPersistenceReceipt(
                session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)))
        XCTAssertEqual(fixture.controller.state, .authenticated)
        XCTAssertNil(fixture.model.visibleNode)
        fixture.model.sessionDidChange()
        XCTAssertEqual(fixture.model.state, .invalidated)
    }

    func testRecoveryInvalidatesLateFailure() async throws {
        let fixture = try await makeFixture(suspended: [1])
        let load = Task { await fixture.model.loadIfNeeded() }
        await fixture.repository.waitForRequest(1)
        fixture.controller.requireRecovery(.scopeMismatch)
        fixture.model.sessionDidChange()
        fixture.repository.complete(1, .failed(.offline))
        await load.value
        XCTAssertEqual(fixture.model.state, .invalidated)
    }

    func testScopeRecoveryInvalidatesLateSuccess() async throws {
        let fixture = try await makeFixture(suspended: [1])
        let load = Task { await fixture.model.loadIfNeeded() }
        await fixture.repository.waitForRequest(1)
        fixture.controller.requireRecovery(.scopeMismatch)
        fixture.repository.complete(1, .loaded(fixture.node))
        await load.value
        XCTAssertEqual(fixture.model.state, .invalidated)
        XCTAssertNil(fixture.model.visibleNode)
    }

    func testCallerCancellationRejectsLateSuccessAndClearsProgress() async throws {
        let fixture = try await makeFixture(suspended: [1])
        let load = Task { await fixture.model.loadIfNeeded() }
        await fixture.repository.waitForRequest(1)
        load.cancel()
        fixture.repository.complete(1, .loaded(fixture.node))
        await load.value
        XCTAssertEqual(fixture.model.state, .cancelled)
        XCTAssertNil(fixture.model.visibleNode)
        XCTAssertFalse(fixture.model.isRequestInProgress)
        XCTAssertEqual(fixture.repository.cancelled[1], true)
    }

    func testCancelledRefreshClearsVerifiedDataAndCanRetry() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        fixture.repository.results = [.failed(.cancelled)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .cancelled)
        XCTAssertNil(fixture.model.visibleNode)
        XCTAssertTrue(fixture.model.canRefresh)
    }

    func testFileALateResponseCannotOverwriteFileBWithSameName() async throws {
        let fixture = try await makeFixture(suspended: [1])
        let loadA = Task { await fixture.model.loadIfNeeded() }
        await fixture.repository.waitForRequest(1)
        fixture.model.invalidate()
        let fileB = try await makeNode(
            41, library: fixture.library, parent: fixture.library.rootNodeId,
            name: fixture.node.name)
        let routeB = NodeFileDetailsRoute(
            node: fileB, library: fixture.route.library, parentScope: fixture.route.parentScope,
            ancestry: fixture.route.ancestry,
            parentDirectoryTitle: fixture.route.parentDirectoryTitle)
        let repositoryB = DetailsRepository(results: [.loaded(fileB)], suspended: [])
        let modelB = NodeFileDetailsViewModel(
            repository: repositoryB, sessionController: fixture.controller, route: routeB)
        await modelB.loadIfNeeded()
        fixture.repository.complete(1, .loaded(fixture.node))
        await loadA.value
        XCTAssertEqual(modelB.visibleNode, fileB)
        XCTAssertEqual(fixture.model.state, .invalidated)
        XCTAssertNotEqual(fixture.route.nodeId, routeB.nodeId)
    }

    func testLongUnicodeNameIsPreservedAndNativeViewConstructs() async throws {
        let fixture = try await makeFixture()
        let name = String(repeating: "資料🚀e\u{301}", count: 60)
        let file = try await makeNode(
            40, library: fixture.library, parent: fixture.library.rootNodeId, name: name)
        fixture.repository.results = [.loaded(file)]
        await fixture.model.loadIfNeeded()
        XCTAssertEqual(fixture.model.visibleNode?.name, name)
        let view = NodeFileDetailsView(
            repository: fixture.repository, sessionController: fixture.controller,
            route: fixture.route)
        XCTAssertNotNil(view.body)
    }

    func testMovedInjectedNodeCannotRemainVerified() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        let wrong = try await makeNode(
            40, library: fixture.library,
            parent: try await NodeId.validated(testID(10002), using: DetailsTestValidator()),
            kind: .file, state: .active)
        fixture.repository.results = [.loaded(wrong)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .unavailable)
        XCTAssertNil(fixture.model.visibleNode)
    }

    func testCrossLibraryInjectedNodeCannotRemainVerified() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        let wrong = try await makeNode(
            40, library: try await makeLibrary(2), parent: fixture.library.rootNodeId, kind: .file,
            state: .active)
        fixture.repository.results = [.loaded(wrong)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .unavailable)
        XCTAssertNil(fixture.model.visibleNode)
    }

    func testWrongIdentityInjectedNodeCannotRemainVerified() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        let wrong = try await makeNode(
            41, library: fixture.library, parent: fixture.library.rootNodeId, kind: .file,
            state: .active)
        fixture.repository.results = [.loaded(wrong)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .unavailable)
        XCTAssertNil(fixture.model.visibleNode)
    }

    func testDirectoryInjectedNodeCannotRemainVerified() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        let wrong = try await makeNode(
            40, library: fixture.library, parent: fixture.library.rootNodeId, kind: .directory,
            state: .active)
        fixture.repository.results = [.loaded(wrong)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .unavailable)
        XCTAssertNil(fixture.model.visibleNode)
    }

    func testTrashedInjectedNodeCannotRemainVerified() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        let wrong = try await makeNode(
            40, library: fixture.library, parent: fixture.library.rootNodeId, kind: .file,
            state: .trashed)
        fixture.repository.results = [.loaded(wrong)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .unavailable)
        XCTAssertNil(fixture.model.visibleNode)
    }

    func testPurgingInjectedNodeCannotRemainVerified() async throws {
        let fixture = try await makeFixture()
        await fixture.model.loadIfNeeded()
        let wrong = try await makeNode(
            40, library: fixture.library, parent: fixture.library.rootNodeId, kind: .file,
            state: .purging)
        fixture.repository.results = [.loaded(wrong)]
        await fixture.model.refresh()
        XCTAssertEqual(fixture.model.state, .unavailable)
        XCTAssertNil(fixture.model.visibleNode)
    }

    private struct Fixture {
        let library: Library
        let node: Node
        let route: NodeFileDetailsRoute
        let controller: SessionController
        let repository: DetailsRepository
        let model: NodeFileDetailsViewModel
    }

    private func makeFixture(suspended: Set<Int> = []) async throws -> Fixture {
        let library = try await makeLibrary(1)
        let node = try await makeNode(
            40, library: library, parent: library.rootNodeId, name: "report-old.txt")
        let route = NodeFileDetailsRoute(
            node: node, library: NodeBrowserLibraryContext(library),
            parentScope: .libraryRoot(rootNodeId: library.rootNodeId),
            ancestry: [library.rootNodeId], parentDirectoryTitle: library.name)
        let controller = try makeAuthenticatedController()
        let repository = DetailsRepository(results: [.loaded(node)], suspended: suspended)
        return Fixture(
            library: library, node: node, route: route, controller: controller,
            repository: repository,
            model: NodeFileDetailsViewModel(
                repository: repository, sessionController: controller, route: route))
    }

    private func makeAuthenticatedController() throws -> SessionController {
        let endpoint = try ServerEndpoint(validating: "https://folders.synveil.example")
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint),
            logoutService: DetailsLogoutService()
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
        let validator = DetailsTestValidator()
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
        name: String? = nil,
        revision: String = "7",
        state: NodeState = .active,
        version: Int? = nil
    ) async throws -> Node {
        let validator = DetailsTestValidator()
        var versionId: FileVersionId?
        if let version {
            versionId = try await FileVersionId.validated(testID(version), using: validator)
        }
        return Node(
            id: try await NodeId.validated(testID(number), using: validator),
            libraryId: library.id,
            parentId: parent,
            currentVersionId: versionId,
            revision: try NodeRevision(validating: revision),
            name: name ?? "Item \(number)",
            kind: kind,
            state: state,
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
private final class DetailsRepository: NodeRepositoryProtocol {
    struct Request {
        let libraryId: LibraryId
        let nodeId: NodeId
        let parent: NodeParentScope
    }
    var results: [NodeDetailsRepositoryResult]
    private var suspended: Set<Int>
    private(set) var requests: [Request] = []
    private(set) var cancelled: [Int: Bool] = [:]
    private var completions: [Int: CheckedContinuation<NodeDetailsRepositoryResult, Never>] = [:]
    private var waiters: [(Int, CheckedContinuation<Void, Never>)] = []

    init(results: [NodeDetailsRepositoryResult], suspended: Set<Int>) {
        self.results = results
        self.suspended = suspended
    }
    func listChildren(libraryId: LibraryId, parent: NodeParentScope) async -> NodeRepositoryResult {
        XCTFail("File details must never refetch the collection")
        return .failed(.protocolFailure)
    }
    func getNode(libraryId: LibraryId, nodeId: NodeId, expectedParent: NodeParentScope) async
        -> NodeDetailsRepositoryResult
    {
        requests.append(Request(libraryId: libraryId, nodeId: nodeId, parent: expectedParent))
        let request = requests.count
        let ready = waiters.filter { $0.0 <= request }
        waiters.removeAll { $0.0 <= request }
        for (_, continuation) in ready { continuation.resume() }
        if suspended.remove(request) != nil {
            let result = await withCheckedContinuation { completions[request] = $0 }
            cancelled[request] = Task.isCancelled
            return result
        }
        return results.isEmpty ? .failed(.offline) : results.removeFirst()
    }
    func waitForRequest(_ number: Int) async {
        if requests.count >= number { return }
        await withCheckedContinuation { waiters.append((number, $0)) }
    }
    func complete(_ number: Int, _ result: NodeDetailsRepositoryResult) {
        completions.removeValue(forKey: number)?.resume(returning: result)
    }
}

private struct DetailsTestValidator: RustBridgeProtocol {
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

private struct DetailsLogoutService: SessionLogoutServiceProtocol {
    func logoutLocally() async -> SessionLogoutResult { .credentialAbsent }
}

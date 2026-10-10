import Foundation
import XCTest

@testable import Synveil

@MainActor
final class OfflineNodeBrowserServiceTests: XCTestCase {
    func testVerifiedLiveEmptyListingWinsWithoutReadingTheProjection() async throws {
        let f = try await fixture(live: .loaded([]))

        let result = await f.service.browse(
            libraryId: f.library.id, parent: .libraryRoot(rootNodeId: f.library.rootNodeId),
            request: .liveFirst)

        XCTAssertEqual(result, .live([]))
        XCTAssertEqual(f.live.listCount, 1)
        XCTAssertEqual(f.projection.childrenCount, 0)
        XCTAssertNil(f.scopeProvider.capturedLibrary)
    }

    func testVerifiedLiveNodesRemainAuthoritativeAndKeepRootRequestIdentity() async throws {
        let f = try await fixture(live: .loaded([]))
        let file = try await f.node(parent: f.library.rootNodeId)
        f.live.results = [.loaded([file])]

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

        XCTAssertEqual(result, .live([file]))
        XCTAssertEqual(f.live.requestedParent, .libraryRoot(rootNodeId: f.library.rootNodeId))
        XCTAssertEqual(f.projection.childrenCount, 0)
        XCTAssertNil(f.scopeProvider.capturedLibrary)
    }

    func testOfflineFallbackUsesValidatedRootIDAndBoundedPartialListing() async throws {
        let f = try await fixture(live: .failed(.offline))
        let file = try await f.node(parent: f.library.rootNodeId)
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [file], knowledge: .partial, hasMore: true,
                projection: f.partialProjection))

        let result = await f.service.browse(
            libraryId: f.library.id, parent: .libraryRoot(rootNodeId: f.library.rootNodeId),
            request: .liveFirst)

        guard case .saved(let presentation) = result else {
            return XCTFail("Expected the scoped saved listing")
        }
        XCTAssertEqual(presentation.nodes, [file])
        XCTAssertEqual(presentation.knowledge, .partial)
        XCTAssertTrue(presentation.hasMore)
        XCTAssertEqual(presentation.liveFailure, .offline)
        XCTAssertEqual(f.scopeProvider.capturedLibrary, f.library.id)
        XCTAssertEqual(f.projection.requestedParent, f.library.rootNodeId)
        XCTAssertEqual(f.projection.requestedLimit, NodeProjectionPolicy.maximumChildren)
    }

    func testEligibleTransientFailuresPermitProjectionFallback() async throws {
        let failures: [NodeFailure] = [
            .offline, .dnsFailure, .timeout, .serverUnavailable,
            .httpFailure(statusCode: 500), .httpFailure(statusCode: 503),
            .httpFailure(statusCode: 599),
        ]

        for failure in failures {
            let f = try await fixture(live: .failed(failure))
            f.projection.childrenResult = .loaded(
                CachedChildren(
                    nodes: [], knowledge: .partial, hasMore: false,
                    projection: f.partialProjection))

            let result = await f.service.browse(
                libraryId: f.library.id,
                parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

            guard case .saved(let presentation) = result else {
                return XCTFail("Expected fallback for \(failure)")
            }
            XCTAssertEqual(presentation.liveFailure, failure)
            XCTAssertEqual(f.projection.childrenCount, 1)
        }
    }

    func testSecurityAndProtocolFailuresNeverReadProjection() async throws {
        let failures: [NodeFailure] = [
            .unauthenticated, .credentialUnavailable, .invalidCredential, .originMismatch,
            .staleSession, .authenticationRejected, .deviceRevoked, .tlsFailure,
            .redirectRejected, .unexpectedContentType, .protocolFailure, .resourceLimit,
            .repeatedCursor, .cancelled, .httpFailure(statusCode: 403),
            .httpFailure(statusCode: 600),
        ]

        for failure in failures {
            let f = try await fixture(live: .failed(failure))
            let result = await f.service.browse(
                libraryId: f.library.id,
                parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

            XCTAssertEqual(result, .failed(failure))
            XCTAssertEqual(f.projection.childrenCount, 0, "Unexpected cache read for \(failure)")
            XCTAssertNil(f.scopeProvider.capturedLibrary)
        }
    }

    func testScopeMismatchCannotSelectCachedLibraryRows() async throws {
        let f = try await fixture(live: .failed(.offline))
        f.scopeProvider.scopeValue = try await queueScope(library: 2)

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

        XCTAssertEqual(result, .failed(.offline))
        XCTAssertEqual(f.projection.childrenCount, 0)
    }

    func testSavedItemsActionPerformsLocalReadWithoutLiveRequest() async throws {
        let f = try await fixture(live: .failed(.offline))
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [], knowledge: .missing, hasMore: false,
                projection: f.partialProjection))
        let model = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))

        await model.viewSavedItems()

        XCTAssertEqual(f.live.listCount, 0)
        XCTAssertEqual(f.projection.childrenCount, 1)
        guard case .saved(let presentation) = model.state else {
            return XCTFail("Expected an explicit saved-items presentation")
        }
        XCTAssertEqual(presentation.knowledge, .missing)
        XCTAssertEqual(model.contentSource, .cached)
        XCTAssertFalse(model.canPrepareMutationsFromVisibleResults)
    }

    func testServerOnlyRefreshNeverFallsBackToProjection() async throws {
        let f = try await fixture(live: .failed(.timeout))

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .serverOnly)

        XCTAssertEqual(result, .failed(.timeout))
        XCTAssertEqual(f.projection.childrenCount, 0)
        XCTAssertNil(f.scopeProvider.capturedLibrary)
    }

    func testSavedReadFailureIsTypedAndDoesNotCallLiveRepository() async throws {
        let f = try await fixture(live: .loaded([]))
        f.projection.childrenResult = .unavailable(.malformedMetadata)

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .savedOnly)

        XCTAssertEqual(result, .savedUnavailable(.unavailable))
        XCTAssertEqual(f.live.listCount, 0)
        XCTAssertEqual(f.projection.childrenCount, 1)
    }

    func testCachedFolderAndFileDestinationsPreserveCanonicalMetadata() async throws {
        let f = try await fixture(live: .failed(.offline))
        let folder = try await f.node(parent: f.library.rootNodeId, kind: .directory)
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [folder], knowledge: .partial, hasMore: false,
                projection: f.partialProjection))
        let rootModel = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))
        await rootModel.viewSavedItems()
        let folderRoute = try XCTUnwrap(rootModel.route(into: folder))
        XCTAssertEqual(folderRoute.parentScope, .directory(folder.id))
        XCTAssertEqual(folderRoute.initialSource, .cached)

        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [], knowledge: .missing, hasMore: false,
                projection: f.partialProjection))
        let childModel = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: folderRoute)
        await childModel.loadIfNeeded()
        XCTAssertEqual(f.live.listCount, 0)
        XCTAssertEqual(f.projection.requestedParent, folder.id)

        let file = try await f.node(
            parent: f.library.rootNodeId, version: 12, name: "Exact 文件 🚀.txt")
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [file], knowledge: .partial, hasMore: false,
                projection: f.partialProjection))
        let fileModel = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))
        await fileModel.viewSavedItems()
        let detailsRoute = try XCTUnwrap(fileModel.details(for: file))
        XCTAssertEqual(detailsRoute.contentSource, .cached)
        XCTAssertEqual(detailsRoute.currentVersionId?.rawValue, queueUUID(12))
        let position = SyncJournalPosition(
            epoch: try ClientMutationDecimal(validating: "1"),
            sequence: try ClientMutationDecimal(validating: "0"))
        f.projection.nodeResult = .found(
            CachedNodeRecord(
                scope: f.scope, position: position, id: file.id,
                revision: try ClientMutationDecimal(validating: file.revision.rawValue),
                lifecycle: .active, provenance: .canonical, metadata: file),
            f.partialProjection)

        let details = NodeFileDetailsViewModel(
            repository: f.live, sessionController: f.sessionController,
            offlineDetailsService: f.service, route: detailsRoute)
        await details.loadIfNeeded()
        XCTAssertEqual(f.live.detailsCount, 0)
        XCTAssertEqual(f.projection.nodeCount, 2)
        XCTAssertEqual(details.visibleNode?.name, "Exact 文件 🚀.txt")
        XCTAssertEqual(details.visibleNode?.revision, file.revision)
        XCTAssertEqual(details.visibleNode?.currentVersionId, file.currentVersionId)
        XCTAssertEqual(details.contentSource, .cached)
    }

    func testUnprovenCompleteDirectoryIsDowngradedToPartial() async throws {
        let f = try await fixture(live: .failed(.offline))
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [], knowledge: .complete, hasMore: false,
                projection: f.partialProjection))

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

        guard case .saved(let presentation) = result else {
            return XCTFail("Expected saved metadata")
        }
        XCTAssertEqual(presentation.knowledge, .partial)
    }

    func testRebaselineAndTruncatedProjectionCannotAppearComplete() async throws {
        let f = try await fixture(live: .failed(.offline))
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [], knowledge: .complete, hasMore: true,
                projection: NodeProjectionState(
                    completeness: .rebaselineRequired, incrementalAnchor: nil,
                    locallyApplied: nil, serverConfirmed: nil, library: nil)))

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

        guard case .saved(let presentation) = result else {
            return XCTFail("Expected saved recovery evidence")
        }
        XCTAssertEqual(presentation.knowledge, .staleKnown)
        XCTAssertTrue(presentation.hasMore)
        XCTAssertEqual(presentation.projection.completeness, .rebaselineRequired)
    }

    func testInvalidCachedParentIdentityPreservesOriginalNetworkFailure() async throws {
        let f = try await fixture(live: .failed(.dnsFailure))
        let otherParent = try await NodeId.validated(queueUUID(10002), using: QueueValidator())
        let invalid = try await f.node(parent: otherParent)
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [invalid], knowledge: .partial, hasMore: false,
                projection: f.partialProjection))

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

        XCTAssertEqual(result, .failed(.dnsFailure))
    }

    func testStaleKnownCachedFoldersCannotCreateOrdinaryNavigationRoutes() async throws {
        let f = try await fixture(live: .failed(.offline))
        let folder = try await f.node(parent: f.library.rootNodeId, kind: .directory)
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [folder], knowledge: .staleKnown, hasMore: false,
                projection: f.partialProjection))
        let model = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))

        await model.viewSavedItems()

        XCTAssertEqual(model.contentSource, .cached)
        XCTAssertNil(model.route(into: folder))
    }

    func testCacheReadFailurePreservesOriginalTransientFailure() async throws {
        let f = try await fixture(live: .failed(.timeout))
        f.projection.childrenResult = .unavailable(.reconciliationRequired)

        let result = await f.service.browse(
            libraryId: f.library.id,
            parent: .libraryRoot(rootNodeId: f.library.rootNodeId), request: .liveFirst)

        XCTAssertEqual(result, .failed(.timeout))
    }

    func testSavedFileDetailsRejectParentMismatchAndIncompleteRecords() async throws {
        let f = try await fixture(live: .failed(.offline))
        let file = try await f.node(parent: f.library.rootNodeId)
        let wrongParent = try await NodeId.validated(queueUUID(10001), using: QueueValidator())
        let position = SyncJournalPosition(
            epoch: try ClientMutationDecimal(validating: "1"),
            sequence: try ClientMutationDecimal(validating: "0"))
        let projection = f.partialProjection
        f.projection.nodeResult = .found(
            CachedNodeRecord(
                scope: f.scope, position: position, id: file.id,
                revision: try ClientMutationDecimal(validating: file.revision.rawValue),
                lifecycle: .active, provenance: .canonical, metadata: file),
            projection)

        let mismatch = await f.service.savedNode(
            libraryId: f.library.id, nodeId: file.id,
            expectedParent: .directory(wrongParent), ancestry: [wrongParent])
        XCTAssertEqual(mismatch, .unavailable(.unavailable))

        f.projection.nodeResult = .found(
            CachedNodeRecord(
                scope: f.scope, position: position, id: file.id,
                revision: try ClientMutationDecimal(validating: file.revision.rawValue),
                lifecycle: .active, provenance: .lastKnown, metadata: file),
            projection)
        let incomplete = await f.service.savedNode(
            libraryId: f.library.id, nodeId: file.id,
            expectedParent: .libraryRoot(rootNodeId: f.library.rootNodeId),
            ancestry: [f.library.rootNodeId])
        XCTAssertEqual(incomplete, .unavailable(.unavailable))
        XCTAssertEqual(f.live.listCount, 0)
    }

    func testSavedFileDetailsRejectDirectoryMetadata() async throws {
        let f = try await fixture(live: .failed(.offline))
        let folder = try await f.node(parent: f.library.rootNodeId, kind: .directory)
        let position = SyncJournalPosition(
            epoch: try ClientMutationDecimal(validating: "1"),
            sequence: try ClientMutationDecimal(validating: "0"))
        f.projection.nodeResult = .found(
            CachedNodeRecord(
                scope: f.scope, position: position, id: folder.id,
                revision: try ClientMutationDecimal(validating: folder.revision.rawValue),
                lifecycle: .active, provenance: .canonical, metadata: folder),
            f.partialProjection)

        let result = await f.service.savedNode(
            libraryId: f.library.id, nodeId: folder.id,
            expectedParent: .libraryRoot(rootNodeId: f.library.rootNodeId),
            ancestry: [f.library.rootNodeId])

        XCTAssertEqual(result, .unavailable(.unavailable))
        XCTAssertEqual(f.live.detailsCount, 0)
    }

    func testSavedFileDetailsRejectKnownInactiveAncestor() async throws {
        let f = try await fixture(live: .failed(.offline))
        let folder = try await f.node(parent: f.library.rootNodeId, kind: .directory, id: 11)
        let file = try await f.node(parent: folder.id, id: 12)
        let position = SyncJournalPosition(
            epoch: try ClientMutationDecimal(validating: "1"),
            sequence: try ClientMutationDecimal(validating: "0"))
        f.projection.nodeResult = .found(
            CachedNodeRecord(
                scope: f.scope, position: position, id: file.id,
                revision: try ClientMutationDecimal(validating: file.revision.rawValue),
                lifecycle: .active, provenance: .canonical, metadata: file),
            f.partialProjection)
        f.projection.nodeResults[folder.id] = .found(
            CachedNodeRecord(
                scope: f.scope, position: position, id: folder.id,
                revision: try ClientMutationDecimal(validating: folder.revision.rawValue),
                lifecycle: .trashed, provenance: .canonical, metadata: folder),
            f.partialProjection)

        let result = await f.service.savedNode(
            libraryId: f.library.id, nodeId: file.id,
            expectedParent: .directory(folder.id), ancestry: [f.library.rootNodeId, folder.id])

        XCTAssertEqual(result, .unavailable(.unavailable))
    }

    func testExplicitServerRefreshReplacesCacheOnlyAfterLiveSuccess() async throws {
        let f = try await fixture(live: .failed(.offline))
        let file = try await f.node(parent: f.library.rootNodeId)
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [file], knowledge: .partial, hasMore: false,
                projection: f.partialProjection))
        let model = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))
        await model.viewSavedItems()
        XCTAssertEqual(model.contentSource, .cached)

        let liveFile = try await f.node(parent: f.library.rootNodeId, name: "Server copy.txt")
        f.live.results = [.loaded([liveFile])]
        await model.refresh()

        XCTAssertEqual(model.state, .loaded([liveFile]))
        XCTAssertEqual(model.contentSource, .live)
        XCTAssertEqual(f.projection.childrenCount, 1)
    }

    func testTransientServerRefreshRetainsCachedDataAndSecurityFailureClearsIt() async throws {
        let f = try await fixture(live: .failed(.offline))
        let file = try await f.node(parent: f.library.rootNodeId)
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [file], knowledge: .partial, hasMore: false,
                projection: f.partialProjection))
        let model = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))
        await model.viewSavedItems()

        f.live.results = [.failed(.timeout)]
        await model.refresh()
        XCTAssertEqual(model.contentSource, .cached)
        XCTAssertEqual(model.visibleNodes, [file])
        XCTAssertEqual(model.savedRefreshFailure, .node(.timeout))
        XCTAssertFalse(model.canPrepareMutationsFromVisibleResults)

        f.live.results = [.failed(.tlsFailure)]
        await model.refresh()
        XCTAssertEqual(model.state, .failed(.node(.tlsFailure)))
        XCTAssertNil(model.visibleNodes)
        XCTAssertFalse(model.canPrepareMutationsFromVisibleResults)
        XCTAssertEqual(f.projection.childrenCount, 1)
    }

    func testLateSavedReadCannotReplaceNewerLiveResult() async throws {
        let f = try await fixture(live: .failed(.offline))
        let cachedFile = try await f.node(parent: f.library.rootNodeId, name: "Saved.txt")
        let liveFile = try await f.node(parent: f.library.rootNodeId, name: "Current.txt")
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [cachedFile], knowledge: .partial, hasMore: false,
                projection: f.partialProjection))
        let gate = QueueGate()
        f.projection.childrenGate = gate
        let model = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))

        let savedRead = Task { await model.viewSavedItems() }
        await gate.wait()
        model.cancelCurrentRequest()
        f.live.results = [.loaded([liveFile])]
        await model.refresh()
        XCTAssertEqual(model.state, .loaded([liveFile]))

        await gate.release()
        await savedRead.value
        XCTAssertEqual(model.state, .loaded([liveFile]))
        XCTAssertEqual(model.contentSource, .live)
    }

    func testLogoutDuringSavedReadCannotRepublishCachedRows() async throws {
        let f = try await fixture(live: .failed(.offline))
        let file = try await f.node(parent: f.library.rootNodeId)
        f.projection.childrenResult = .loaded(
            CachedChildren(
                nodes: [file], knowledge: .partial, hasMore: false,
                projection: f.partialProjection))
        let gate = QueueGate()
        f.projection.childrenGate = gate
        let model = NodeBrowserViewModel(
            repository: f.live, offlineBrowserService: f.service,
            sessionController: f.sessionController, route: .root(for: f.library))

        let savedRead = Task { await model.viewSavedItems() }
        await gate.wait()
        await f.sessionController.requestLogout()
        model.sessionDidChange()
        await gate.release()
        await savedRead.value

        XCTAssertEqual(model.state, .invalidated)
        XCTAssertNil(model.visibleNodes)
    }

    private func fixture(live result: NodeRepositoryResult) async throws -> Fixture {
        let scope = try await queueScope(library: 1)
        let root = try await NodeId.validated(queueUUID(10000), using: QueueValidator())
        let library = Library(
            id: scope.libraryId, revision: try LibraryRevision(validating: "3"),
            name: "Offline test", rootNodeId: root, status: .active,
            createdAt: Date(timeIntervalSince1970: 1_760_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_760_000_100))
        let live = OfflineBrowserLiveRepository(result: result)
        let projection = OfflineBrowserProjectionRepository()
        let scopes = OfflineBrowserScopeProvider(scope: scope)
        let controller = try makeAuthenticatedController(endpoint: scope.serverEndpoint)
        let service = OfflineNodeBrowserService(
            liveRepository: live, projection: projection, scopeProvider: scopes)
        return Fixture(
            library: library, scope: scope, live: live, projection: projection,
            scopeProvider: scopes, sessionController: controller, service: service)
    }

    private func makeAuthenticatedController(endpoint: ServerEndpoint) throws -> SessionController {
        let controller = SessionController(configuration: AppConfiguration(serverEndpoint: endpoint))
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()
        let record = try DeviceCredentialRecord(
            ownerUserId: queueUUID(90), deviceId: queueUUID(91), credentialId: queueUUID(92),
            credential: DeviceCredential(validatedRawValue: queueBearer),
            createdAt: "2026-10-09T12:00:00Z")
        controller.markAuthenticated(
            after: SecureCredentialPersistenceReceipt(
                session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)))
        return controller
    }
}

@MainActor
private struct Fixture {
    let library: Library
    let scope: ClientMutationScope
    let live: OfflineBrowserLiveRepository
    let projection: OfflineBrowserProjectionRepository
    let scopeProvider: OfflineBrowserScopeProvider
    let sessionController: SessionController
    let service: OfflineNodeBrowserService

    var partialProjection: NodeProjectionState {
        NodeProjectionState(
            completeness: .partial, incrementalAnchor: nil, locallyApplied: nil,
            serverConfirmed: nil, library: nil)
    }

    func node(
        parent: NodeId, kind: NodeKind = .file, version: Int? = nil, id: Int = 10,
        name: String = "saved-file.txt"
    ) async throws -> Node {
        let versionId: FileVersionId?
        if let version {
            versionId = try await FileVersionId.validated(queueUUID(version), using: QueueValidator())
        } else {
            versionId = nil
        }
        return Node(
            id: try await NodeId.validated(queueUUID(id), using: QueueValidator()),
            libraryId: library.id, parentId: parent, currentVersionId: versionId,
            revision: try NodeRevision(validating: "7"), name: name,
            kind: kind, state: .active, createdAt: Date(timeIntervalSince1970: 1_760_000_000),
            updatedAt: Date(timeIntervalSince1970: 1_760_000_100), trashedAt: nil,
            restoreDeadline: nil, purgeEligible: false)
    }
}

@MainActor
private final class OfflineBrowserLiveRepository: NodeRepositoryProtocol {
    var results: [NodeRepositoryResult]
    private(set) var listCount = 0
    private(set) var detailsCount = 0
    private(set) var requestedParent: NodeParentScope?

    init(result: NodeRepositoryResult) { results = [result] }

    func listChildren(libraryId: LibraryId, parent: NodeParentScope) async -> NodeRepositoryResult {
        listCount += 1
        requestedParent = parent
        return results.isEmpty ? .failed(.offline) : results.removeFirst()
    }

    func getNode(libraryId: LibraryId, nodeId: NodeId, expectedParent: NodeParentScope) async
        -> NodeDetailsRepositoryResult
    {
        detailsCount += 1
        return .unavailable
    }
}

@MainActor
private final class OfflineBrowserProjectionRepository: NodeProjectionRepositoryProtocol {
    var childrenResult: CachedChildrenResult = .unavailable(.missingMaterialization)
    var nodeResult: CachedNodeResult?
    var nodeResults: [NodeId: CachedNodeResult] = [:]
    private(set) var nodeCount = 0
    private(set) var childrenCount = 0
    private(set) var requestedParent: NodeId?
    private(set) var requestedLimit: Int?
    var childrenGate: QueueGate?

    func node(scope: ClientMutationScope, nodeId: NodeId) async -> CachedNodeResult {
        nodeCount += 1
        if let result = nodeResults[nodeId] { return result }
        if case .found(let record, _) = nodeResult, record.id != nodeId {
            return .missing(.init(
                completeness: .uninitialized, incrementalAnchor: nil, locallyApplied: nil,
                serverConfirmed: nil, library: nil))
        }
        return nodeResult ?? .missing(.init(
            completeness: .uninitialized, incrementalAnchor: nil, locallyApplied: nil,
            serverConfirmed: nil, library: nil))
    }

    func children(scope: ClientMutationScope, parentId: NodeId, limit: Int) async
        -> CachedChildrenResult
    {
        childrenCount += 1
        requestedParent = parentId
        requestedLimit = limit
        await childrenGate?.arrive()
        return childrenResult
    }

    func projectionState(scope: ClientMutationScope) async -> NodeProjectionStateResult {
        .unavailable(.missingMaterialization)
    }
}

@MainActor
private final class OfflineBrowserScopeProvider: InboundSyncCoordinatorProtocol {
    var scopeValue: ClientMutationScope
    private(set) var capturedLibrary: LibraryId?

    init(scope: ClientMutationScope) { scopeValue = scope }

    func scope(libraryId: LibraryId) async throws -> ClientMutationScope {
        capturedLibrary = libraryId
        guard libraryId == scopeValue.libraryId else { throw NodeFailure.originMismatch }
        return scopeValue
    }

    func status(scope: ClientMutationScope) async -> InboundSyncStatus { fatalError("Unused") }

    func synchronize(
        scope: ClientMutationScope, configuration: InboundSyncRunConfiguration,
        progress: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult { fatalError("Unused") }

    func recoverUnknownAcknowledgement(
        scope: ClientMutationScope, position: SyncJournalPosition, confirmedByUser: Bool,
        progress: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult { fatalError("Unused") }
}

import Foundation
import XCTest

@testable import Synveil

@MainActor
private final class NotificationCatalog: LibraryCatalogRepositoryProtocol {
    var libraries: [Library]
    init(_ library: Library) { libraries = [library] }
    func listLibraries() async -> LibraryRepositoryResult { .loaded(libraries) }
}

@MainActor
final class NotificationEligibilityTests: XCTestCase {
    func testDownloadHookObservesOnlyDurableVerifiedIntegrity() async throws {
        let f = try await downloadFixture(self)
        var observations: [DownloadRecord] = []
        f.coordinator.notificationObservation = { observations.append($0) }
        _ = try downloadCapability(await downloadRun(f))
        XCTAssertEqual(observations.count, 1)
        let saved = try await downloadRecord(f)
        XCTAssertEqual(observations[0], saved)
        XCTAssertNotNil(
            NotificationEventClassifier.download(saved, scope: String(repeating: "a", count: 64)))
    }
    func testPartialDownloadNeverObservesCompletion() async throws {
        let f = try await downloadFixture(self, body: Data(repeating: 1, count: 128 * 1024))
        await f.content.configure(failAfterChunks: 1)
        var observed = false
        f.coordinator.notificationObservation = { _ in observed = true }
        _ = await downloadRun(f)
        let record = try await downloadRecord(f)
        XCTAssertNil(
            NotificationEventClassifier.download(record, scope: String(repeating: "a", count: 64)))
        XCTAssertFalse(observed)
    }
    func testFailedDigestNeverObservesCompletion() async throws {
        let f = try await downloadFixture(self)
        await f.content.configure(body: Data("wrong".utf8))
        var observed = false
        f.coordinator.notificationObservation = { _ in observed = true }
        _ = await downloadRun(f)
        let record = try await downloadRecord(f)
        XCTAssertEqual(record.state, .integrityFailure)
        XCTAssertNil(
            NotificationEventClassifier.download(record, scope: String(repeating: "a", count: 64)))
        XCTAssertFalse(observed)
    }
    private func authority(_ f: DownloadFixture) async throws -> (
        NotificationAuthority, NotificationCatalog, ProjectionNodes
    ) {
        let library = Library(
            id: f.queue.scope.libraryId, revision: try LibraryRevision(validating: "1"),
            name: "Private Library", rootNodeId: try XCTUnwrap(f.node.parentId), status: .active,
            createdAt: Date(), updatedAt: Date())
        let catalog = NotificationCatalog(library)
        let nodes = ProjectionNodes()
        nodes.values[f.node.id] = .loaded(f.node)
        return (
            NotificationAuthority(
                provider: f.provider, database: f.queue.database,
                bridge: QueueValidator(), downloads: f.coordinator, catalog: catalog, nodes: nodes),
            catalog, nodes
        )
    }
    func testAuthorityRechecksRealDownloadRecordAndBytes() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let (authority, _, _) = try await authority(f)
        let record = try await downloadRecord(f)
        let observation = try await authority.observeDownload(record)
        let candidate = try XCTUnwrap(observation)
        let valid = try await authority.verify(candidate)
        XCTAssertTrue(valid)
    }
    func testNotificationFileDestinationUsesExistingDetailsWithoutExport() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let (authority, _, _) = try await authority(f)
        let record = try await downloadRecord(f)
        let observation = try await authority.observeDownload(record)
        let candidate = try XCTUnwrap(observation)
        let requestsBefore = await f.content.requests()
        let destination = try await authority.destination(candidate)
        guard case .file(let route) = destination else {
            return XCTFail("Expected authenticated file details")
        }
        XCTAssertEqual(route.nodeId, f.node.id)
        let requestsAfter = await f.content.requests()
        XCTAssertEqual(requestsBefore.count, requestsAfter.count)
        let files = try FileManager.default.contentsOfDirectory(atPath: f.root.path)
        XCTAssertFalse(files.contains { $0.contains("export") })
    }
    func testRemovedNodeCannotRoute() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let (authority, _, nodes) = try await authority(f)
        let record = try await downloadRecord(f)
        let observation = try await authority.observeDownload(record)
        let candidate = try XCTUnwrap(observation)
        nodes.values[f.node.id] = .unavailable
        guard case .unavailable = try await authority.destination(candidate) else {
            return XCTFail("Removed Node must be unavailable")
        }
    }
    func testRemovedLibraryCannotRoute() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let (authority, catalog, _) = try await authority(f)
        let record = try await downloadRecord(f)
        let observation = try await authority.observeDownload(record)
        let candidate = try XCTUnwrap(observation)
        catalog.libraries = []
        guard case .unavailable = try await authority.destination(candidate) else {
            return XCTFail("Removed Library must be unavailable")
        }
    }
    func testEvictedVerifiedCacheCannotNotifyAndDoesNotChangeOperationState() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let (authority, _, _) = try await authority(f)
        let record = try await downloadRecord(f)
        let observation = try await authority.observeDownload(record)
        let candidate = try XCTUnwrap(observation)
        try await f.staging.discard([record.id])
        do {
            _ = try await authority.verify(candidate)
            XCTFail("Missing cache must fail eligibility")
        } catch {}
        let unchanged = try await downloadRecord(f)
        XCTAssertEqual(unchanged.state, .verified)
    }
    func testRealLogoutRevokesNotificationAuthority() async throws {
        let f = try await downloadFixture(self)
        _ = try downloadCapability(await downloadRun(f))
        let (authority, _, _) = try await authority(f)
        let record = try await downloadRecord(f)
        let observation = try await authority.observeDownload(record)
        let candidate = try XCTUnwrap(observation)
        await f.queue.controller.requestLogout()
        do {
            _ = try await authority.verify(candidate)
            XCTFail("Logout must revoke presentation authority")
        } catch {}
    }
    func testDurablyConfirmedNonemptyInboundPageIsEligible() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        _ = await f.run()
        let row = try await f.record()
        let record = try XCTUnwrap(row)
        let candidate = NotificationEventClassifier.inbound(
            record, scope: String(repeating: "a", count: 64), recovery: false)
        XCTAssertEqual(candidate?.category, .backgroundSync)
        XCTAssertNil(
            NotificationEventClassifier.inbound(
                record, scope: String(repeating: "a", count: 64), recovery: true))
    }
    func testStagedInboundPageIsNotASynchronizationSuccess() async throws {
        let f = try await inboundFixture(self)
        _ = try await f.stage()
        let row = try await f.record()
        let record = try XCTUnwrap(row)
        XCTAssertNil(
            NotificationEventClassifier.inbound(
                record, scope: String(repeating: "a", count: 64), recovery: false))
    }
    func testUnknownACKCanOnlyQualifyAsAttention() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        await f.wire.fail(at: 1, .timeout)
        _ = await f.run()
        let row = try await f.record()
        let record = try XCTUnwrap(row)
        XCTAssertEqual(record.state, .ackInFlight)
        XCTAssertNil(
            NotificationEventClassifier.inbound(
                record, scope: String(repeating: "a", count: 64), recovery: false))
        XCTAssertEqual(
            NotificationEventClassifier.inbound(
                record, scope: String(repeating: "a", count: 64), recovery: true)?.category,
            .recovery)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
    }
    func testEmptyBackgroundRefreshHasNoDurableCompletionEvent() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 0)
        let result = await f.run(.background)
        XCTAssertEqual(result.progress.eventsApplied, 0)
        let row = try await f.record()
        XCTAssertNil(row)
    }
    private func upload(loseCreate: Bool = false) async throws -> (
        FileUploadResult, [NotificationCandidate], UploadCoordinatorProvider
    ) {
        let f = try await queueFixture(self, initializeBase: false)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let data = Data("hello".utf8)
        let source = f.url.deletingLastPathComponent().appendingPathComponent("private.txt")
        try data.write(to: source)
        let provider = UploadCoordinatorProvider(
            base: f.provider, library: f.scope.libraryId, parent: parent,
            source: data, loseFirstCreate: loseCreate, partiallyAcceptFirstPatch: 0,
            loseCompletion: false)
        let staging = try DownloadStagingStore(
            root: f.url.deletingLastPathComponent().appendingPathComponent("downloads"),
            freeBytes: { _ in UInt64.max })
        let coordinator = FileUploadCoordinator(
            repository: AuthenticatedUploadRepository(provider: provider, bridge: QueueValidator()),
            database: f.database, staging: UploadStagingStore(shared: staging),
            identityGenerator: FixedUploadIDGenerator(value: queueUUID(703)))
        var observations: [NotificationCandidate] = []
        coordinator.notificationObservation = { record in
            if let candidate = NotificationEventClassifier.upload(
                record, scope: String(repeating: "a", count: 64))
            {
                observations.append(candidate)
            }
        }
        let result = await coordinator.start(
            selectedURL: source,
            target: .createFile(library: f.scope.libraryId, parent: parent, name: "private.txt"),
            library: f.scope.libraryId, libraryStatus: .active, progress: { _ in })
        return (result, observations, provider)
    }
    func testUploadCompletionHookRequiresExplicitDurableCommit() async throws {
        let (result, observations, provider) = try await upload()
        guard case .completed(let record) = result else {
            return XCTFail("Expected committed upload")
        }
        XCTAssertNotNil(record.completion)
        XCTAssertEqual(provider.completionCalls, 1)
        XCTAssertEqual(observations.filter { $0.category == .committedUpload }.count, 1)
    }
    func testUnknownUploadNeverPresentsCompleted() async throws {
        let (result, observations, provider) = try await upload(loseCreate: true)
        guard case .stopped(let record?, _) = result else {
            return XCTFail("Expected uncertain upload")
        }
        XCTAssertTrue(record.uncertain)
        XCTAssertFalse(observations.contains { $0.category == .committedUpload })
        XCTAssertEqual(provider.completionCalls, 0)
    }
    func testPATCHAcceptedAndReadyToCompleteAreNotCommitted() async throws {
        let f = try await queueFixture(self)
        let target = UploadTarget.createFile(
            library: f.scope.libraryId,
            parent: try await NodeId.validated(queueUUID(2), using: QueueValidator()),
            name: "private.txt")
        let expected = try UploadExpectedContent(byteLength: 0, sha256: UploadPolicy.emptyDigest)
        let intent = try await UploadIntent.prepare(
            target: target, expected: expected,
            generator: FixedUploadIDGenerator(value: queueUUID(703)), bridge: QueueValidator())
        let record = UploadRecord(
            id: UploadOperationId(), stagingId: UploadOperationId(), stagingReleased: false,
            intent: intent, scope: f.scope, credentialId: queueUUID(92),
            originatingSessionId: UUID().uuidString.lowercased(),
            serverSessionId: queueUUID(703), serverTargetNodeId: nil, authoritativeOffset: 0,
            serverState: .open, state: .readyToComplete, uncertain: false, attemptCount: 1,
            completion: nil, failure: nil)
        XCTAssertNil(
            NotificationEventClassifier.upload(record, scope: String(repeating: "a", count: 64)))
    }
    func testEarlyNativeResponsesAreBoundedAndReplayedOnceAfterInstallation() {
        let relay = NotificationApplicationDelivery()
        let first = UUID()
        let latest = UUID()
        relay.receive(first)
        relay.receive(latest)
        var received: [UUID] = []
        relay.install { received.append($0) }
        relay.install { _ in XCTFail("Duplicate installation") }
        XCTAssertEqual(received, [latest])
    }
}

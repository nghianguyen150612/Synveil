import Foundation
import XCTest

@testable import Synveil

@MainActor
final class InboundSyncCoordinatorTests: XCTestCase {
    func testMissingCheckpointMakesZeroRequests() async throws {
        let f = try await inboundFixture(self, initializeBase: false)
        let result = await f.run()
        XCTAssertEqual(result.reason, .checkpointRequired)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testWrongDeviceMakesZeroRequests() async throws {
        let f = try await inboundFixture(self)
        let scope = try await queueScope(device: 93)
        let result = await f.coordinator.synchronize(scope: scope, configuration: .foreground)
        XCTAssertEqual(result.reason, .scopeMismatch)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testWrongOwnerMakesZeroRequests() async throws {
        let f = try await inboundFixture(self)
        let scope = try await queueScope(owner: 99)
        let result = await f.coordinator.synchronize(scope: scope, configuration: .foreground)
        XCTAssertEqual(result.reason, .scopeMismatch)
    }
    func testWrongEndpointMakesZeroRequests() async throws {
        let f = try await inboundFixture(self)
        let scope = try await queueScope(endpoint: "https://other.example")
        let result = await f.coordinator.synchronize(scope: scope, configuration: .foreground)
        XCTAssertEqual(result.reason, .scopeMismatch)
    }
    func testUnauthenticatedRunBlocked() async throws {
        let f = try await inboundFixture(self)
        await f.base.controller.requestLogout()
        let result = await f.run()
        XCTAssertTrue(
            [.authenticationRequired, .committedButSessionChanged].contains(result.reason))
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testCurrentScopeCanBeCapturedWithoutNetwork() async throws {
        let f = try await inboundFixture(self)
        let scope = try await f.coordinator.scope(libraryId: f.scope.libraryId)
        XCTAssertEqual(scope, f.scope)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testEmptyFeedIsActualUpToDateObservation() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 0)
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
        XCTAssertEqual(result.progress.pagesProcessed, 0)
        XCTAssertEqual(result.progress.eventsApplied, 0)
    }
    func testEmptyFeedDoesNotStageApplyOrAck() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 0)
        _ = await f.run()
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.map(\.method), [.get])
        XCTAssertTrue(f.nodes.calls.isEmpty)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM inbound_pages"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM projection_commits"), "0")
    }
    func testOnePageHappyPathUsesProductionSQLiteAck() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
        XCTAssertEqual(result.progress.eventsApplied, 1)
        XCTAssertEqual(result.progress.pagesProcessed, 1)
        XCTAssertEqual(result.progress.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "1")
        let row = try await f.record()
        XCTAssertEqual(row?.state, .ackConfirmed)
    }
    func testTwoPageHappyPath() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 2)
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
        XCTAssertEqual(result.progress.pagesProcessed, 2)
        XCTAssertEqual(result.progress.eventsApplied, 2)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.map(\.method), [.get, .post, .get, .post, .get])
    }
    func testThreePagesPreserveSequentialOrdering() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 3)
        let result = await f.run()
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "3")
        for position in 0..<3 {
            let record = try await f.record(from: String(position))
            XCTAssertEqual(record?.state, .ackConfirmed)
        }
    }
    func testProjectionCommitAndAckConfirmationPrecedeNextGet() async throws {
        let f = try await inboundFixture(self)
        let db = f.database
        let scope = f.scope
        await f.wire.inspect { index, request in
            if request.method == .post {
                let state = try await db.cachedProjectionState(
                    scope: scope, credentialId: queueUUID(92), bridge: QueueValidator())
                XCTAssertEqual(state.locallyApplied?.sequence.rawValue, index == 1 ? "1" : "2")
                let page = try await db.inboundPage(
                    scope: scope, position: feedPosition(from: index == 1 ? "0" : "1"),
                    credentialId: queueUUID(92), bridge: QueueValidator())
                XCTAssertEqual(page?.state, .ackInFlight)
            }
            if index == 2 || index == 4 {
                let base = try await db.syncBase(scope: scope)
                XCTAssertEqual(base?.sequence, index == 2 ? "1" : "2")
                let page = try await db.inboundPage(
                    scope: scope, position: feedPosition(from: index == 2 ? "0" : "1"),
                    credentialId: queueUUID(92), bridge: QueueValidator())
                XCTAssertEqual(page?.state, .ackConfirmed)
            }
        }
        try await f.configure(pages: 2)
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
    }
    func testStagingCommitPrecedesMaterialization() async throws {
        let f = try await inboundFixture(self)
        let gate = QueueGate()
        f.nodes.gate = gate
        try await f.configure()
        let run = Task { await f.run() }
        await gate.wait()
        let row = try await f.record()
        XCTAssertEqual(row?.state, .receivedUnapplied)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM projection_commits"), "0")
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
        await gate.release()
        let result = await run.value
        XCTAssertEqual(result.reason, .upToDate)
    }
    func testHighWatermarkNeverSkipsSequences() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(high: 9000)
        let result = await f.run(
            InboundSyncRunConfiguration(pageSize: 200, maximumPages: 1, maximumEvents: 4096))
        XCTAssertEqual(result.reason, .moreWork)
        XCTAssertEqual(result.progress.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "1")
        XCTAssertEqual(result.progress.observedHighWatermark?.sequence.rawValue, "9000")
    }
    func testRequestsContainOnlyPageLimitNoInventedCursor() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 2)
        _ = await f.run()
        let requests = await f.wire.requests()
        for request in requests where request.method == .get {
            let items = URLComponents(url: request.url, resolvingAgainstBaseURL: false)?.queryItems
            XCTAssertEqual(items, [URLQueryItem(name: "limit", value: "200")])
        }
    }
    func testRepeatedPageStopsInsteadOfLooping() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([
            try queueHTTP(inboundObject(f.scope)),
            try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")),
            try queueHTTP(inboundObject(f.scope)),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .reconciliationRequired)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 3)
    }
    func testPageBudgetStopsWithoutNextRequest() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 3)
        let result = await f.run(
            InboundSyncRunConfiguration(pageSize: 200, maximumPages: 2, maximumEvents: 4096))
        XCTAssertEqual(result.reason, .moreWork)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 4)
    }
    func testEventBudgetReducesFeedLimit() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let result = await f.run(
            InboundSyncRunConfiguration(pageSize: 200, maximumPages: 8, maximumEvents: 1))
        XCTAssertEqual(result.reason, .moreWork)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(
            URLComponents(url: requests[0].url, resolvingAgainstBaseURL: false)?.queryItems?.first?
                .value, "1")
    }
    private func invalid(_ configuration: InboundSyncRunConfiguration) async throws {
        let f = try await inboundFixture(self)
        let result = await f.run(configuration)
        XCTAssertEqual(result.reason, .invalidConfiguration)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testZeroPageSizeRejected() async throws {
        try await invalid(.init(pageSize: 0, maximumPages: 8, maximumEvents: 4096))
    }
    func testPageSize501Rejected() async throws {
        try await invalid(.init(pageSize: 501, maximumPages: 8, maximumEvents: 4096))
    }
    func testZeroPagesRejected() async throws {
        try await invalid(.init(pageSize: 200, maximumPages: 0, maximumEvents: 4096))
    }
    func testHardMaximumPagesEnforced() async throws {
        try await invalid(.init(pageSize: 200, maximumPages: 65, maximumEvents: 4096))
    }
    func testZeroEventsNeverMeansUnlimited() async throws {
        try await invalid(.init(pageSize: 200, maximumPages: 8, maximumEvents: 0))
    }
    func testHardMaximumEventsEnforced() async throws {
        try await invalid(.init(pageSize: 200, maximumPages: 8, maximumEvents: 4097))
    }
    func testNegativeBudgetRejected() async throws {
        try await invalid(.init(pageSize: -1, maximumPages: -1, maximumEvents: -1))
    }
    func testMinimumAndHardMaximumConfigurationsValid() async throws {
        XCTAssertTrue(
            InboundSyncRunConfiguration(pageSize: 1, maximumPages: 1, maximumEvents: 1).isValid)
        XCTAssertTrue(
            InboundSyncRunConfiguration(pageSize: 500, maximumPages: 64, maximumEvents: 4096)
                .isValid)
    }
    func testDefaultIsEightSequentialPages() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 9, kind: .nodePurged)
        let result = await f.run()
        XCTAssertEqual(result.reason, .moreWork)
        XCTAssertEqual(result.progress.pagesProcessed, 8)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 16)
    }
    func testMaximumEventBudgetIs4096() async throws {
        let f = try await inboundFixture(self)
        var script: [HTTPTransportResponse] = []
        var from: UInt64 = 0
        for count in [500, 500, 500, 500, 500, 500, 500, 500, 96] {
            script.append(
                try queueHTTP(inboundObject(f.scope, from: from, count: count, kind: .nodePurged)))
            from += UInt64(count)
            script.append(
                try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: String(from))))
        }
        await f.wire.configure(script)
        let result = await f.run(.init(pageSize: 500, maximumPages: 64, maximumEvents: 4096))
        XCTAssertEqual(result.reason, .moreWork)
        XCTAssertEqual(result.progress.eventsApplied, 4096)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 18)
        XCTAssertEqual(
            URLComponents(url: requests[16].url, resolvingAgainstBaseURL: false)?.queryItems?.first?
                .value, "96")
    }
    func testNoOutboundMutationPost() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        _ = await f.run()
        let requests = await f.wire.requests()
        XCTAssertTrue(
            requests.filter { $0.method == .post }.allSatisfy {
                $0.url.path.hasSuffix("/changes/ack")
            })
    }
    func testConstructionAndLocalStatusDoNotSynchronize() async throws {
        let f = try await inboundFixture(self)
        let status = await f.coordinator.status(scope: f.scope)
        XCTAssertNil(status.stopReason)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testPartialProjectionStaysPartialAfterSuccess() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        _ = await f.run()
        let status = await f.coordinator.status(scope: f.scope)
        XCTAssertEqual(status.completeness, .partial)
        XCTAssertNil(status.stopReason)
    }
    func testProgressSeparatesStagedAppliedAndConfirmed() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        var snapshots: [InboundSyncProgress] = []
        _ = await f.run { snapshots.append($0) }
        let staged = try XCTUnwrap(snapshots.first { $0.phase == .staged })
        XCTAssertEqual(staged.eventsApplied, 0)
        let applied = try XCTUnwrap(snapshots.first { $0.phase == .applied })
        XCTAssertEqual(applied.eventsApplied, 1)
        XCTAssertEqual(applied.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(applied.serverConfirmed?.sequence.rawValue, "0")
        let confirmed = try XCTUnwrap(snapshots.first { $0.phase == .confirmed })
        XCTAssertEqual(confirmed.pagesProcessed, 1)
        XCTAssertEqual(confirmed.serverConfirmed?.sequence.rawValue, "1")
        XCTAssertTrue(snapshots.contains { $0.phase == .acknowledging })
    }
    private func event(_ kind: SyncChangeKind) async throws {
        let f = try await inboundFixture(self)
        try await f.configure(kind: kind)
        let result = await f.run()
        XCTAssertEqual(result.reason, .upToDate)
        XCTAssertEqual(result.progress.eventsApplied, 1)
    }
    func testNodeCreated() async throws { try await event(.nodeCreated) }
    func testNodeRenamed() async throws { try await event(.nodeRenamed) }
    func testNodeMoved() async throws { try await event(.nodeMoved) }
    func testNodeTrashed() async throws { try await event(.nodeTrashed) }
    func testNodeRestored() async throws { try await event(.nodeRestored) }
    func testFileContentCommitted() async throws { try await event(.fileContentCommitted) }
    func testFileVersionRestored() async throws { try await event(.fileVersionRestored) }
    func testNodePurged() async throws { try await event(.nodePurged) }
    func testMaterializationRevisionAdvanceBlocksAck() async throws {
        let f = try await inboundFixture(self)
        let node = try await projectionNode(scope: f.scope, id: 11, revision: "9")
        f.nodes.values[node.id] = .loaded(node)
        try await f.configure()
        let result = await f.run()
        XCTAssertEqual(result.reason, .metadataChanged)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
        let row = try await f.record()
        XCTAssertEqual(row?.state, .receivedUnapplied)
    }
    func testMissingMaterializationBlocksAck() async throws {
        let f = try await inboundFixture(self)
        f.nodes.values.removeAll()
        try await f.configure()
        let result = await f.run()
        XCTAssertEqual(result.reason, .missingMaterialization)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testOfflineRetainsTypedResult() async throws {
        let f = try await inboundFixture(self)
        let result = await f.run()
        XCTAssertEqual(result.reason, .offline)
    }
    func testAuthenticationFailureRoutesThroughExistingRecovery() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([try inboundError("authentication_failed", status: 401)])
        let result = await f.run()
        XCTAssertEqual(result.reason, .authenticationRequired)
        XCTAssertNotEqual(f.base.controller.state, .authenticated)
    }
    func testDeviceRevocationIsDistinct() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([try inboundError("device_revoked", status: 403)])
        let result = await f.run()
        XCTAssertEqual(result.reason, .deviceRevoked)
    }
    func testAckDeviceRevocationRemainsDistinctAfterSessionRecovery() async throws {
        let f = try await inboundFixture(self)
        await f.wire.configure([
            try queueHTTP(inboundObject(f.scope)),
            try inboundError("device_revoked", status: 403),
        ])
        let result = await f.run()
        XCTAssertEqual(result.reason, .deviceRevoked)
        XCTAssertEqual(result.progress.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
    }
    func testMaterializationRevisionRegressionBlocksAck() async throws {
        let f = try await inboundFixture(self)
        let node = try await projectionNode(scope: f.scope, id: 11, revision: "7")
        f.nodes.values[node.id] = .loaded(node)
        try await f.configure()
        let result = await f.run()
        XCTAssertEqual(result.reason, .metadataChanged)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testEmptyPartialDirectoryNeverBecomesAuthoritative() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(kind: .nodePurged)
        _ = await f.run()
        let result = await f.projection.children(
            scope: f.scope,
            parentId: try await NodeId.validated(queueUUID(2), using: QueueValidator()), limit: 10)
        guard case .loaded(let children) = result else { return XCTFail("Expected local read") }
        XCTAssertTrue(children.nodes.isEmpty)
        XCTAssertNotEqual(children.knowledge, .complete)
    }

}

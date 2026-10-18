import Foundation
import XCTest

@testable import Synveil

#if canImport(SwiftUI)
    import SwiftUI
    import UIKit
#endif

@MainActor
final class RebaselineViewModelTests: XCTestCase {
    func testAppearanceInspectsOnlyLocalStatusAndStartRequiresConfirmation() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let model = RebaselineViewModel(
            library: try await snapshotLibrary(f.scope), coordinator: snapshotCoordinator(f),
            sessionController: f.controller)
        await model.loadStatus()
        XCTAssertTrue(model.canStart)
        await model.start(confirmedByUser: false)
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testDurableCountersPreparedAndCompleteMessages() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        let model = RebaselineViewModel(
            library: try await snapshotLibrary(f.scope), coordinator: coordinator,
            sessionController: f.controller)
        await model.loadStatus()
        await f.transport.set(try queueHTTP(snapshotEnvelope(snapshotBootstrapObject(f.scope))))
        await model.start(confirmedByUser: true)
        await f.transport.setSequence([
            try queueHTTP(
                snapshotPageObject(f.scope, rows: Array(snapshotRows().prefix(2)), more: true)),
            try queueHTTP(snapshotPageObject(f.scope, rows: Array(snapshotRows().suffix(1)))),
        ])
        await model.continueSnapshot()
        XCTAssertEqual(model.progress?.stagedNodes, 3)
        XCTAssertEqual(model.progress?.pages, 2)
        await model.continueSnapshot()
        XCTAssertEqual(model.progress?.state, .prepared)
        await model.continueSnapshot(confirmedByUser: false)
        XCTAssertEqual(model.progress?.state, .prepared)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        await model.continueSnapshot(confirmedByUser: true)
        XCTAssertEqual(model.progress?.state, .activeComplete)
        XCTAssertTrue(model.message.contains("sequence 25"))
        XCTAssertTrue(model.message.contains("Newer server changes may exist"))
        XCTAssertFalse(model.message.contains("opaque"))
    }
    func testUnknownCompletionExposesExplicitRecoveryWithoutAutomaticRetry() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        let model = RebaselineViewModel(
            library: try await snapshotLibrary(f.scope), coordinator: coordinator,
            sessionController: f.controller)
        await model.loadStatus()
        await f.transport.fail(.timeout)
        await model.continueSnapshot(confirmedByUser: true)
        XCTAssertEqual(model.action, .recoverOriginalCompletion)
        let before = await f.transport.requests().count
        await model.continueSnapshot(confirmedByUser: false)
        await model.loadStatus()
        let after = await f.transport.requests().count
        XCTAssertEqual(before, after)
    }
    func testLogoutInvalidatesAndClearsSnapshotProgress() async throws {
        let f = try await queueFixture(self)
        try await snapshotPrepared(f)
        let model = RebaselineViewModel(
            library: try await snapshotLibrary(f.scope), coordinator: snapshotCoordinator(f),
            sessionController: f.controller)
        await model.loadStatus()
        await f.controller.requestLogout()
        model.sessionDidChange()
        XCTAssertNil(model.progress)
        XCTAssertNil(model.action)
        XCTAssertFalse(model.canStart)
    }
    func testSavedTopologyNavigatesWithoutFabricatedLiveNodes() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        let root = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        let saved = try await f.database.cachedChildren(
            scope: f.scope, parentId: root, limit: 100, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertTrue(saved.nodes.isEmpty)
        XCTAssertEqual(saved.knowledge, .complete)
        XCTAssertEqual(saved.snapshotNodes.map(\.id.rawValue), [queueUUID(2)])
        let record = try await f.database.cachedNode(
            scope: f.scope, nodeId: root, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertNil(record?.metadata)
        XCTAssertNil(record?.completeNode)
        XCTAssertEqual(record?.provenance, .snapshotManifest)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let again = try await reopened.cachedChildren(
            scope: f.scope, parentId: root, limit: 100, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(again, saved)
    }
    func testIncrementalSyncAfterSnapshotCutOverlaysAndRetainsCompleteness() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        let event = projectionEvent(id: 3, sequence: "26", parent: 2)
        let page = try await projectionPage(f, events: [event], from: 25)
        let live = try await projectionNode(scope: f.scope, id: 3)
        let result = try await projectionApply(
            f, page: page, repository: projectionNodes(f, node: live))
        guard case .applied = result else {
            return XCTFail("Post-snapshot page must apply at the exact cut")
        }
        try await projectionConfirm(f, page: page)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let children = try await f.database.cachedChildren(
            scope: f.scope, parentId: parent, limit: 100, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(children.nodes.map(\.id), [live.id])
        XCTAssertTrue(children.snapshotNodes.isEmpty)
        XCTAssertEqual(children.projection.completeness, .complete)
        XCTAssertEqual(children.projection.serverConfirmed?.sequence.rawValue, "26")
    }

    func testExpiredCompletionRetentionBlocksNewStartAndExplainsReconciliation() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        let model = RebaselineViewModel(
            library: try await snapshotLibrary(f.scope), coordinator: coordinator,
            sessionController: f.controller)
        await model.loadStatus()
        await f.transport.fail(.timeout)
        await model.continueSnapshot(confirmedByUser: true)
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "bootstrap_expired", "message": "Expired",
                        "request_id": "snapshot-request-01", "retryable": false,
                    ]
                ], status: 410))
        await model.continueSnapshot(confirmedByUser: true)
        XCTAssertEqual(model.progress?.state, .reconciliationRequired)
        XCTAssertNil(model.action)
        XCTAssertFalse(model.canStart)
        XCTAssertTrue(model.message.contains("Authoritative reconciliation"))
    }
    func testSnapshotAndPostCutRowsShareFoldersFirstNumericNameOrdering() async throws {
        let f = try await queueFixture(self)
        let directory = try await projectionNode(
            scope: f.scope, id: 10, name: "Folder 10", kind: .directory)
        let file = try await projectionNode(scope: f.scope, id: 20, name: "A file")
        var row = snapshotNodeObject(30, kind: "DIRECTORY")
        row["name"] = "folder 2"
        let snapshot = try await JSONDecoder().decode(
            RebaselineNodeDTO.self, from: JSONSerialization.data(withJSONObject: row)
        ).validated(bridge: QueueValidator())
        let state = NodeProjectionState(
            completeness: .complete, incrementalAnchor: nil, locallyApplied: nil,
            serverConfirmed: nil, library: nil)
        let presentation = NodeCachedDirectoryPresentation(
            nodes: [file, directory], knowledge: .complete, hasMore: false, projection: state,
            liveFailure: nil, snapshotNodes: [snapshot])
        XCTAssertEqual(
            presentation.orderedItems.map(\.id.rawValue),
            [queueUUID(30), queueUUID(10), queueUUID(20)])
    }

    func testHandoffRefreshesSurroundingSyncStatusWithoutNetworkWork() async throws {
        let f = try await inboundFixture(self)
        let library = try await snapshotLibrary(f.scope)
        let parent = SyncStatusViewModel(
            library: library, coordinator: f.coordinator, projection: f.projection,
            checkpoint: f.base.service, sessionController: f.base.controller)
        await parent.loadStatus()
        XCTAssertNotEqual(parent.completeness, .complete)
        let before = await f.base.transport.requests().count
        let model = RebaselineViewModel(
            library: library, coordinator: snapshotCoordinator(f.base),
            sessionController: f.base.controller, statusDidChange: { await parent.loadStatus() })
        await model.loadStatus()
        await f.base.transport.set(
            try queueHTTP(snapshotEnvelope(snapshotBootstrapObject(f.scope))))
        await model.start(confirmedByUser: true)
        await f.base.transport.set(try queueHTTP(snapshotPageObject(f.scope, rows: snapshotRows())))
        await model.continueSnapshot()
        await model.continueSnapshot()
        await f.base.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        await model.continueSnapshot(confirmedByUser: true)
        XCTAssertEqual(parent.completeness, .complete)
        XCTAssertEqual(parent.progress.locallyApplied?.sequence.rawValue, "25")
        XCTAssertEqual(parent.progress.serverConfirmed?.sequence.rawValue, "25")
        XCTAssertTrue(parent.canSync)
        let syncRequests = await f.wire.requests()
        let snapshotRequests = await f.base.transport.requests()
        XCTAssertTrue(syncRequests.isEmpty)
        XCTAssertEqual(snapshotRequests.count, before + 3)
    }
    #if canImport(SwiftUI)
        func testNativeRebaselineViewRendersAndSupportsAccessibilityDynamicType() async throws {
            let f = try await queueFixture(self)
            let view = RebaselineProgressView(
                library: try await snapshotLibrary(f.scope), coordinator: snapshotCoordinator(f),
                sessionController: f.controller)
            let host = UIHostingController(
                rootView: List { view }.environment(\.dynamicTypeSize, .accessibility5))
            host.loadViewIfNeeded()
            XCTAssertNotNil(host.view)
        }
        func testNativeLimitedSnapshotInformationWithLongUnicodeName() async throws {
            let f = try await queueFixture(self)
            var row = snapshotNodeObject(3)
            row["name"] = String(repeating: "e\u{301}📂", count: 100)
            let node = try await JSONDecoder().decode(
                RebaselineNodeDTO.self, from: JSONSerialization.data(withJSONObject: row)
            ).validated(bridge: QueueValidator())
            let host = UIHostingController(
                rootView: SnapshotNodeInformationView(node: node, sessionController: f.controller)
                    .environment(\.dynamicTypeSize, .accessibility5))
            host.loadViewIfNeeded()
            XCTAssertNotNil(host.view)
        }
    #endif
}

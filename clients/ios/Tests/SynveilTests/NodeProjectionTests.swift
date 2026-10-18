import Foundation
import XCTest

@testable import Synveil

@MainActor
final class NodeProjectionTests: XCTestCase {
    func testInvalidNodeId() async throws { try await invalidMetadata("id", value: "bad") }
    func testInvalidLibraryId() async throws { try await invalidMetadata("library", value: "bad") }
    func testInvalidParentId() async throws { try await invalidMetadata("parent", value: "bad") }
    func testInvalidVersionId() async throws {
        try await invalidMetadata("fileVersion", value: "bad")
    }
    func testUnsupportedProjectionVersion() async throws {
        try await invalidMetadata("version", value: 2)
    }
    func testRevisionLeadingZero() async throws {
        try await invalidMetadata("revision", value: "08")
    }
    func testRevisionOverflow() async throws {
        try await invalidMetadata("revision", value: "18446744073709551616")
    }
    func testZeroRevision() async throws { try await invalidMetadata("revision", value: "0") }
    func testInvalidTimestamp() async throws { try await invalidMetadata("created", value: 1e+100) }
    func testUpdateBeforeCreation() async throws { try await invalidMetadata("updated", value: 0) }
    func testDirectoryWithVersion() async throws {
        try await invalidMetadata("kind", value: "DIRECTORY")
    }
    func testActiveTrashTimestamp() async throws { try await invalidMetadata("trashed", value: 1) }
    func testActiveRestoreDeadline() async throws {
        try await invalidMetadata("deadline", value: 1)
    }
    func testActivePurgeEligibility() async throws {
        try await invalidMetadata("purgeEligible", value: true)
    }
    func testEmptyName() async throws { try await invalidMetadata("name", value: "") }
    func testUnknownKind() async throws { try await invalidMetadata("kind", value: "UNKNOWN") }
    func testUnknownState() async throws { try await invalidMetadata("state", value: "UNKNOWN") }
    func testSelfParent() async throws {
        try await invalidMetadata("parent", value: "018f0010-abcd-7000-8000-000000000001")
    }
    private func invalidMetadata(_ key: String, value: Any) async throws {
        let scope = try await queueScope()
        let node = try await projectionNode(scope: scope, version: 3)
        var object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: JSONEncoder().encode(NodeProjectionMetadata(node)))
                as? [String: Any])
        object[key] = value
        do {
            let metadata = try JSONDecoder().decode(
                NodeProjectionMetadata.self, from: JSONSerialization.data(withJSONObject: object))
            _ = try await metadata.validated(using: QueueValidator())
            XCTFail("Invalid persisted metadata must fail closed")
        } catch {}
    }
    func testValidCanonicalNodeRoundTrip() async throws {
        let node = try await projectionNode(scope: queueScope())
        let prepared = try await PreparedProjectionNode.validated(node, bridge: QueueValidator())
        let decoded = try await JSONDecoder().decode(
            NodeProjectionMetadata.self, from: prepared.bytes
        ).validated(using: QueueValidator())
        XCTAssertEqual(node, decoded)
    }
    func testExactRevisionThroughU64Maximum() async throws {
        let node = try await projectionNode(scope: queueScope(), revision: "18446744073709551615")
        let prepared = try await PreparedProjectionNode.validated(node, bridge: QueueValidator())
        XCTAssertEqual(prepared.node.revision.rawValue, "18446744073709551615")
    }
    func testExactSequenceAboveFloatingPointIntegerRange() async throws {
        let position = try feedPosition(from: "9007199254740993")
        XCTAssertEqual(position.sequence.rawValue, "9007199254740993")
    }
    func testPartialNodeNotExposedAsComplete() async throws {
        let scope = try await queueScope()
        let node = try await projectionNode(scope: scope)
        let record = CachedNodeRecord(
            scope: scope, position: try feedPosition(from: "1"), id: node.id,
            revision: try ClientMutationDecimal(validating: "9"), lifecycle: .trashed,
            provenance: .lastKnown, metadata: node)
        XCTAssertNil(record.completeNode)
        XCTAssertEqual(record.metadata, node)
    }
    func testTombstoneIsNotActiveItem() async throws {
        let scope = try await queueScope()
        let node = try await projectionNode(scope: scope)
        let record = CachedNodeRecord(
            scope: scope, position: try feedPosition(from: "1"), id: node.id,
            revision: try ClientMutationDecimal(validating: "8"), lifecycle: .purged,
            provenance: .event, metadata: nil)
        XCTAssertNil(record.completeNode)
    }
    func testCompleteNodeUsesCanonicalFields() async throws {
        let scope = try await queueScope()
        let node = try await projectionNode(scope: scope)
        let record = CachedNodeRecord(
            scope: scope, position: try feedPosition(from: "1"), id: node.id,
            revision: try ClientMutationDecimal(validating: "8"), lifecycle: .active,
            provenance: .canonical, metadata: node)
        XCTAssertEqual(record.completeNode, node)
    }
    func testMetadataDoesNotContainContentOrLocalPaths() async throws {
        let prepared = try await PreparedProjectionNode.validated(
            projectionNode(scope: queueScope(), version: 3), bridge: QueueValidator())
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: prepared.bytes) as? [String: Any])
        for key in ["size", "sha256", "mime", "path", "transferStatus"] {
            XCTAssertNil(object[key])
        }
        XCTAssertEqual(object["fileVersion"] as? String, queueUUID(3))
    }
    func testEventNodeCreated() async throws { try await event(.nodeCreated) }
    func testEventNodeRenamed() async throws { try await event(.nodeRenamed) }
    func testEventNodeMoved() async throws { try await event(.nodeMoved) }
    func testEventNodeTrashed() async throws { try await event(.nodeTrashed) }
    func testEventNodeRestored() async throws { try await event(.nodeRestored) }
    func testEventFileContentCommitted() async throws { try await event(.fileContentCommitted) }
    func testEventFileVersionRestored() async throws { try await event(.fileVersionRestored) }
    func testEventNodePurged() async throws { try await event(.nodePurged) }
    private func event(_ kind: SyncChangeKind) async throws {
        let f = try await queueFixture(self)
        let version: Int? = [.fileContentCommitted, .fileVersionRestored].contains(kind) ? 3 : nil
        let page = try await projectionPage(f, events: [projectionEvent(kind, version: version)])
        let nodes = try await projectionNodes(
            f, node: projectionNode(scope: f.scope, version: version))
        guard
            case .applied(let record, let state, _) = try await projectionApply(
                f, page: page, repository: nodes)
        else { return XCTFail("Page did not commit") }
        XCTAssertEqual(record.state, .appliedAckPending)
        XCTAssertEqual(state.completeness, .partial)
        XCTAssertEqual(state.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(state.serverConfirmed?.sequence.rawValue, "0")
        let node = try await f.database.cachedNode(
            scope: f.scope, nodeId: page.events[0].resourceId, credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(node?.revision.rawValue, "8")
        switch kind {
        case .nodePurged:
            XCTAssertEqual(node?.lifecycle, .purged)
            XCTAssertNil(node?.metadata)
            XCTAssertTrue(nodes.calls.isEmpty)
        case .nodeTrashed:
            XCTAssertEqual(node?.lifecycle, .trashed)
            XCTAssertNil(node?.completeNode)
            XCTAssertNil(node?.metadata)
            XCTAssertTrue(nodes.calls.isEmpty)
        default:
            XCTAssertEqual(node?.completeNode?.name, "Exact e\u{301} / 📂")
            XCTAssertEqual(node?.metadata?.currentVersionId?.rawValue, version.map(queueUUID))
        }
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testConcurrentServerAdvanceLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, revision: "9"))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.reconciliationRequired))
        try assertUnapplied(f)
    }
    func testCanonicalRevisionRegressionLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, revision: "7"))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.revisionRegression))
        try assertUnapplied(f)
    }
    func testWrongLibraryCanonicalLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: queueScope(library: 81)))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.scopeMismatch))
        try assertUnapplied(f)
    }
    func testWrongNodeIdentityLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, id: 3))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.scopeMismatch))
        try assertUnapplied(f)
    }
    func testWrongParentCanonicalLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, parent: 3))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.malformedMetadata))
        try assertUnapplied(f)
    }
    func testWrongKindCanonicalLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, kind: .directory))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.malformedMetadata))
        try assertUnapplied(f)
    }
    func testTrashedCanonicalForRenameLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, state: .trashed))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.malformedMetadata))
        try assertUnapplied(f)
    }
    func testUnexpectedVersionCanonicalLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, version: 4))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.malformedMetadata))
        try assertUnapplied(f)
    }
    func testMissingParentCanonicalLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .loaded(try await projectionNode(scope: f.scope, parent: nil))
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.malformedMetadata))
        try assertUnapplied(f)
    }
    func testMissingCanonicalLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .unavailable
        let applied = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(applied, .failed(.missingMaterialization))
        try assertUnapplied(f)
    }
    func testMalformedCanonicalLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .inconsistent
        let applied = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(applied, .failed(.malformedMetadata))
        try assertUnapplied(f)
    }
    func testCanonicalGetTimeoutLeavesPageUnapplied() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        nodes.values[id] = .failed(.timeout)
        let applied = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(applied, .failed(.transport(.timeout)))
        try assertUnapplied(f)
    }
    func testParentMustBeDirectory() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let parent = try await projectionNode(scope: f.scope, id: 2, parent: nil)
        nodes.values[parent.id] = .loaded(parent)
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.invalidParent))
        try assertUnapplied(f)
    }
    func testParentWrongLibrary() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let parent = try await projectionNode(
            scope: queueScope(library: 81), id: 2, parent: nil, kind: .directory)
        nodes.values[parent.id] = .loaded(parent)
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.invalidParent))
        try assertUnapplied(f)
    }
    func testParentSelfCycle() async throws {
        let f = try await queueFixture(self)
        let nodes = try await projectionNodes(f)
        let parent = try await projectionNode(scope: f.scope, id: 2, parent: 1, kind: .directory)
        nodes.values[parent.id] = .loaded(parent)
        let result = try await projectionApply(f, repository: nodes)
        XCTAssertEqual(result, .failed(.invalidParent))
        try assertUnapplied(f)
    }
    func testRepeatedNodeEventsMaterializeFinalRevisionOnly() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(
            f,
            events: [
                projectionEvent(.nodeCreated, revision: "7"),
                projectionEvent(.nodeRenamed, event: 1002, sequence: "2"),
            ])
        let nodes = try await projectionNodes(f)
        guard case .applied = try await projectionApply(f, page: page, repository: nodes) else {
            return XCTFail()
        }
        XCTAssertEqual(nodes.calls.count, 2)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_events"), "2")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "1")
    }
    func testCreateThenPurgeNeedsNoStaleGet() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(
            f,
            events: [
                projectionEvent(.nodeCreated, revision: "7"),
                projectionEvent(.nodePurged, event: 1002, sequence: "2"),
            ])
        let nodes = ProjectionNodes()
        guard case .applied = try await projectionApply(f, page: page, repository: nodes) else {
            return XCTFail()
        }
        XCTAssertTrue(nodes.calls.isEmpty)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT lifecycle FROM cached_nodes"), "PURGED")
    }
    func testEventRevisionRegressionInsidePage() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(
            f,
            events: [projectionEvent(revision: "9"), projectionEvent(event: 1002, sequence: "2")])
        let result = try await projectionApply(f, page: page)
        XCTAssertEqual(result, .failed(.revisionRegression))
        try assertUnapplied(f)
    }
    func testPurgedNodeCannotBeRestoredInsidePage() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(
            f,
            events: [
                projectionEvent(.nodePurged),
                projectionEvent(.nodeRestored, event: 1002, revision: "9", sequence: "2"),
            ])
        let result = try await projectionApply(f, page: page)
        XCTAssertEqual(result, .failed(.revisionRegression))
        try assertUnapplied(f)
    }
    func testEmptyCacheNeverClaimsComplete() async throws {
        let f = try await queueFixture(self)
        let state = await projectionCache(f).projectionState(scope: f.scope)
        guard case .loaded(let value) = state else { return XCTFail() }
        XCTAssertEqual(value.completeness, .uninitialized)
        XCTAssertNil(value.locallyApplied)
    }
    func testEmptyPartialDirectoryIsNotAuthoritativelyEmpty() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let id = try await NodeId.validated(queueUUID(88), using: QueueValidator())
        guard
            case .loaded(let children) = await projectionCache(f).children(
                scope: f.scope, parentId: id, limit: 10)
        else { return XCTFail() }
        XCTAssertTrue(children.nodes.isEmpty)
        XCTAssertEqual(children.knowledge, .missing)
        XCTAssertEqual(children.projection.completeness, .partial)
    }
    func testOfflineReadDoesNotSendRequest() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let before = await f.transport.requests().count
        let id = try await NodeId.validated(queueUUID(1), using: QueueValidator())
        guard case .found = await projectionCache(f).node(scope: f.scope, nodeId: id) else {
            return XCTFail()
        }
        let after = await f.transport.requests().count
        XCTAssertEqual(before, after)
    }
    private func assertUnapplied(_ f: QueueFixture) throws {
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "RECEIVED_UNAPPLIED")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_commits"), "0")
    }
}

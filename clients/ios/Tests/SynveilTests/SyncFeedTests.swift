import Foundation
import XCTest

@testable import Synveil

@MainActor
final class SyncFeedTests: XCTestCase {
    func testValidEmptyPage() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(scope: scope, object: feedObject(scope: scope, count: 0))
        XCTAssertTrue(page.events.isEmpty)
        XCTAssertNil(page.evidence)
        XCTAssertEqual(page.through.rawValue, "0")
    }
    func testValidNonemptyExactRevisionAndOptionalFields() async throws {
        let scope = try await queueScope()
        var object = feedObject(scope: scope)
        var data = object["data"] as! [String: Any]
        var events = data["changes"] as! [[String: Any]]
        events[0]["parent_node_id"] = queueUUID(2)
        events[0]["current_version_id"] = queueUUID(3)
        events[0]["node_kind"] = "FILE"
        events[0]["node_state"] = "PURGING"
        data["changes"] = events
        object["data"] = data
        let page = try await feedPage(scope: scope, object: object)
        XCTAssertEqual(page.events.count, 2)
        XCTAssertEqual(page.events[0].resourceRevision.rawValue, "9007199254740993")
        XCTAssertEqual(page.events[0].parentId?.rawValue, queueUUID(2))
        XCTAssertEqual(page.events[0].currentVersionId?.rawValue, queueUUID(3))
        XCTAssertEqual(page.events[0].nodeState, .purging)
        XCTAssertEqual(page.events[0].schemaVersion, 1)
    }
    func testKindNodeCreated() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(
                feedObject(scope: scope), key: "change_kind", value: "NODE_CREATED"))
        XCTAssertEqual(page.events.first?.kind.rawValue, "NODE_CREATED")
    }
    func testKindNodeRenamed() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(
                feedObject(scope: scope), key: "change_kind", value: "NODE_RENAMED"))
        XCTAssertEqual(page.events.first?.kind.rawValue, "NODE_RENAMED")
    }
    func testKindNodeMoved() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(feedObject(scope: scope), key: "change_kind", value: "NODE_MOVED"))
        XCTAssertEqual(page.events.first?.kind.rawValue, "NODE_MOVED")
    }
    func testKindNodeTrashed() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(
                feedObject(scope: scope), key: "change_kind", value: "NODE_TRASHED"))
        XCTAssertEqual(page.events.first?.kind.rawValue, "NODE_TRASHED")
    }
    func testKindNodeRestored() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(
                feedObject(scope: scope), key: "change_kind", value: "NODE_RESTORED"))
        XCTAssertEqual(page.events.first?.kind.rawValue, "NODE_RESTORED")
    }
    func testKindFileContentCommitted() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(
                feedObject(scope: scope), key: "change_kind", value: "FILE_CONTENT_COMMITTED"))
        XCTAssertEqual(page.events.first?.kind.rawValue, "FILE_CONTENT_COMMITTED")
    }
    func testKindFileVersionRestored() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(
                feedObject(scope: scope), key: "change_kind", value: "FILE_VERSION_RESTORED"))
        XCTAssertEqual(page.events.first?.kind.rawValue, "FILE_VERSION_RESTORED")
    }
    func testKindNodePurged() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope,
            object: eventChanged(feedObject(scope: scope), key: "change_kind", value: "NODE_PURGED")
        )
        XCTAssertEqual(page.events.first?.kind.rawValue, "NODE_PURGED")
    }
    func testRejectUnknownKind() async throws {
        try await rejectEvent("change_kind", value: "NODE_FUTURE")
    }
    func testRejectFutureSchema() async throws { try await rejectEvent("schema_version", value: 2) }
    func testRejectSchemaZero() async throws { try await rejectEvent("schema_version", value: 0) }
    func testRejectStringSchema() async throws {
        try await rejectEvent("schema_version", value: "1")
    }
    func testRejectBoolSchema() async throws {
        try await rejectEvent("schema_version", value: true)
    }
    func testRejectResourceKind() async throws {
        try await rejectEvent("resource_kind", value: "FILE")
    }
    func testRejectEventId() async throws { try await rejectEvent("event_id", value: "bad") }
    func testRejectUUIDv4() async throws {
        try await rejectEvent("event_id", value: "018f0010-abcd-4000-8000-000000000001")
    }
    func testRejectResourceId() async throws { try await rejectEvent("resource_id", value: "bad") }
    func testRejectParentId() async throws { try await rejectEvent("parent_node_id", value: "bad") }
    func testRejectVersionId() async throws {
        try await rejectEvent("current_version_id", value: "bad")
    }
    func testRejectNodeKind() async throws { try await rejectEvent("node_kind", value: "folder") }
    func testRejectNodeState() async throws {
        try await rejectEvent("node_state", value: "DELETED")
    }
    func testRejectRevisionLeadingZero() async throws {
        try await rejectEvent("resource_revision", value: "01")
    }
    func testRejectRevisionNegative() async throws {
        try await rejectEvent("resource_revision", value: "-1")
    }
    func testRejectRevisionExponent() async throws {
        try await rejectEvent("resource_revision", value: "1e3")
    }
    func testRejectRevisionOverflow() async throws {
        try await rejectEvent("resource_revision", value: "18446744073709551616")
    }
    func testRejectNumericRevision() async throws {
        try await rejectEvent("resource_revision", value: 1)
    }
    func testRejectInvalidTimestamp() async throws {
        try await rejectEvent("occurred_at", value: "2026-02-30T00:00:00Z")
    }
    func testRejectTimestampNoZone() async throws {
        try await rejectEvent("occurred_at", value: "2026-10-09T12:00:00")
    }
    func testRejectUnknownEventKey() async throws { try await rejectEvent("future", value: true) }
    func testRejectNullOptionalParent() async throws {
        try await rejectEvent("parent_node_id", value: NSNull())
    }
    func testRejectNullOptionalKind() async throws {
        try await rejectEvent("node_kind", value: NSNull())
    }
    func testRejectNullOptionalState() async throws {
        try await rejectEvent("node_state", value: NSNull())
    }
    func testRejectNullOptionalVersion() async throws {
        try await rejectEvent("current_version_id", value: NSNull())
    }
    func testRejectMissingFirst() async throws { try await rejectEvent("sequence", value: "2") }
    func testRejectSequenceLeadingZero() async throws {
        try await rejectEvent("sequence", value: "01")
    }
    func testRejectWrongDevice() async throws {
        try await rejectData("device_id", value: queueUUID(99), failure: .scopeMismatch)
    }
    func testRejectWrongLibrary() async throws {
        try await rejectData("library_id", value: queueUUID(99), failure: .scopeMismatch)
    }
    func testRejectZeroEpoch() async throws {
        try await rejectData("epoch", value: "0", failure: .protocolFailure)
    }
    func testRejectEpochMismatch() async throws {
        try await rejectData("epoch", value: "2", failure: .rebaselineRequired)
    }
    func testRejectFromMismatch() async throws {
        try await rejectData("from_sequence", value: "1", failure: .checkpointConflict)
    }
    func testRejectThroughMismatch() async throws {
        try await rejectData("through_sequence", value: "3", failure: .protocolFailure)
    }
    func testRejectHighBelowThrough() async throws {
        try await rejectData("high_watermark", value: "1", failure: .protocolFailure)
    }
    func testRejectIncorrectHasMore() async throws {
        try await rejectData("has_more", value: true, failure: .protocolFailure)
    }
    func testRejectUnknownData() async throws {
        try await rejectData("future", value: true, failure: .protocolFailure)
    }
    func testRejectNullToken() async throws {
        try await rejectData("ack_token", value: NSNull(), failure: .protocolFailure)
    }
    func testRejectOversizeToken() async throws {
        try await rejectData(
            "ack_token", value: String(repeating: "a", count: 257), failure: .protocolFailure)
    }
    func testRejectTokenWhitespace() async throws {
        try await rejectData("ack_token", value: "bad token", failure: .protocolFailure)
    }
    func testRejectTokenURLCharacters() async throws {
        try await rejectData("ack_token", value: "bad/token", failure: .protocolFailure)
    }
    func testRejectEmptyToken() async throws {
        try await rejectData("ack_token", value: "", failure: .protocolFailure)
    }
    func testRejectNumericDecimal() async throws {
        try await rejectData("through_sequence", value: 2, failure: .protocolFailure)
    }
    func testRejectDecimalOverflow() async throws {
        try await rejectData(
            "high_watermark", value: "18446744073709551616", failure: .protocolFailure)
    }
    func testMissingEveryRequiredEventField() async throws {
        let scope = try await queueScope()
        for key in [
            "event_id", "schema_version", "sequence", "resource_kind", "resource_id", "change_kind",
            "resource_revision", "occurred_at",
        ] {
            let object = eventChanged(feedObject(scope: scope), key: key, value: nil)
            await feedAssertFailure(.protocolFailure) {
                try await feedPage(scope: scope, object: object)
            }
        }
    }
    func testMissingEveryRequiredFeedField() async throws {
        let scope = try await queueScope()
        for key in [
            "device_id", "library_id", "epoch", "from_sequence", "through_sequence",
            "high_watermark", "has_more", "changes",
        ] {
            let object = dataChanged(feedObject(scope: scope), key: key, value: nil)
            await feedAssertFailure(.protocolFailure) {
                try await feedPage(scope: scope, object: object)
            }
        }
    }
    func testNullEveryRequiredEventField() async throws {
        let scope = try await queueScope()
        for key in [
            "event_id", "schema_version", "sequence", "resource_kind", "resource_id", "change_kind",
            "resource_revision", "occurred_at",
        ] {
            await feedAssertFailure(.protocolFailure) {
                try await feedPage(
                    scope: scope,
                    object: eventChanged(feedObject(scope: scope), key: key, value: NSNull()))
            }
        }
    }
    func testDuplicateEventIdentity() async throws {
        try await rejectSecond("event_id", value: queueUUID(1000))
    }
    func testDuplicateSequence() async throws { try await rejectSecond("sequence", value: "1") }
    func testSequenceGap() async throws { try await rejectSecond("sequence", value: "3") }
    func testSequenceOutOfOrder() async throws { try await rejectSecond("sequence", value: "0") }
    func testNonemptyRequiresToken() async throws { try await rejectData("ack_token", value: nil) }
    func testEmptyCannotHaveToken() async throws {
        let scope = try await queueScope()
        let object = dataChanged(
            feedObject(scope: scope, count: 0), key: "ack_token", value: "evidence")
        await feedAssertFailure(.protocolFailure) {
            try await feedPage(scope: scope, object: object)
        }
    }
    func testMaximumFiveHundredEventsAccepted() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(scope: scope, object: feedObject(scope: scope, count: 500))
        XCTAssertEqual(page.events.count, 500)
    }
    func testFiveHundredOneRejected() async throws {
        let scope = try await queueScope()
        await feedAssertFailure(.protocolFailure) {
            try await feedPage(scope: scope, object: feedObject(scope: scope, count: 501))
        }
    }
    func testRequestedLimitEnforced() async throws {
        let scope = try await queueScope()
        await feedAssertFailure(.protocolFailure) { try await feedPage(scope: scope, limit: 1) }
    }
    func testSequenceAtUnsignedMaximumEmpty() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(
            scope: scope, object: feedObject(scope: scope, count: 0, from: UInt64.max),
            from: String(UInt64.max))
        XCTAssertEqual(page.through.rawValue, String(UInt64.max))
    }
    func testSequenceIncrementOverflowRejected() async throws {
        let scope = try await queueScope()
        var object = feedObject(scope: scope, count: 1)
        object = dataChanged(object, key: "from_sequence", value: String(UInt64.max))
        object = dataChanged(object, key: "through_sequence", value: String(UInt64.max))
        object = dataChanged(object, key: "high_watermark", value: String(UInt64.max))
        await feedAssertFailure(.protocolFailure) {
            try await feedPage(scope: scope, object: object, from: String(UInt64.max))
        }
    }
    func testMalformedSuccessfulResponse() async throws {
        let scope = try await queueScope()
        for body in ["null", "{}", "[]", "not JSON"] {
            await feedAssertFailure(.protocolFailure) {
                try await SyncFeedResponseDecoder(bridge: QueueValidator()).decode(
                    HTTPTransportResponse(
                        statusCode: 200, headers: ["Content-Type": "application/json"],
                        body: Data(body.utf8)), scope: scope, expected: feedPosition())
            }
        }
    }
    func testOversizedBody() async throws {
        let scope = try await queueScope()
        await feedAssertFailure(.protocolFailure) {
            try await SyncFeedResponseDecoder(bridge: QueueValidator()).decode(
                HTTPTransportResponse(
                    statusCode: 200, headers: ["Content-Type": "application/json"],
                    body: Data(repeating: 32, count: SyncFeedPolicy.maximumResponseBytes + 1)),
                scope: scope, expected: feedPosition())
        }
    }
    func testWrongContentType() async throws {
        let scope = try await queueScope()
        await feedAssertFailure(.protocolFailure) {
            try await SyncFeedResponseDecoder(bridge: QueueValidator()).decode(
                queueHTTP(feedObject(scope: scope), headers: ["Content-Type": "text/html"]),
                scope: scope, expected: feedPosition())
        }
    }
    func testInvalidRequestIdAndUnknownMeta() async throws {
        let scope = try await queueScope()
        for meta in [
            ["request_id": "unsafe\nrequest"], ["request_id": "feed-request-01", "extra": "x"],
        ] {
            var object = feedObject(scope: scope)
            object["meta"] = meta
            await feedAssertFailure(.protocolFailure) {
                try await feedPage(scope: scope, object: object)
            }
        }
    }
    func testHeaderRequestIdMismatch() async throws {
        let scope = try await queueScope()
        await feedAssertFailure(.protocolFailure) {
            try await SyncFeedResponseDecoder(bridge: QueueValidator()).decode(
                queueHTTP(
                    feedObject(scope: scope),
                    headers: [
                        "Content-Type": "application/json", "X-Request-Id": "different-request",
                    ]), scope: scope, expected: feedPosition())
        }
    }
    func testUnknownEnvelopeField() async throws {
        let scope = try await queueScope()
        var object = feedObject(scope: scope)
        object["extra"] = true
        await feedAssertFailure(.protocolFailure) {
            try await feedPage(scope: scope, object: object)
        }
    }
    func testEmptyPageCannotClaimUndeliveredHighWatermark() async throws {
        let scope = try await queueScope()
        let object = feedObject(scope: scope, count: 0, high: 1)
        await feedAssertFailure(.protocolFailure) {
            try await feedPage(scope: scope, object: object)
        }
    }
    func testMaximumU64ResourceRevisionIsExact() async throws {
        let scope = try await queueScope()
        let object = eventChanged(
            feedObject(scope: scope), key: "resource_revision", value: "18446744073709551615")
        let page = try await feedPage(scope: scope, object: object)
        XCTAssertEqual(page.events[0].resourceRevision.rawValue, "18446744073709551615")
    }
    private func dataChanged(_ object: [String: Any], key: String, value: Any?) -> [String: Any] {
        var result = object
        var data = result["data"] as! [String: Any]
        data[key] = value
        result["data"] = data
        return result
    }
    private func eventChanged(_ object: [String: Any], key: String, value: Any?, index: Int = 0)
        -> [String: Any]
    {
        var result = object
        var data = result["data"] as! [String: Any]
        var events = data["changes"] as! [[String: Any]]
        events[index][key] = value
        data["changes"] = events
        result["data"] = data
        return result
    }
    private func rejectEvent(_ key: String, value: Any?) async throws {
        let scope = try await queueScope()
        await feedAssertFailure(.protocolFailure) {
            try await feedPage(
                scope: scope, object: eventChanged(feedObject(scope: scope), key: key, value: value)
            )
        }
    }
    private func rejectData(_ key: String, value: Any?, failure: SyncFeedFailure = .protocolFailure)
        async throws
    {
        let scope = try await queueScope()
        await feedAssertFailure(failure) {
            try await feedPage(
                scope: scope, object: dataChanged(feedObject(scope: scope), key: key, value: value))
        }
    }
    private func rejectSecond(_ key: String, value: Any) async throws {
        let scope = try await queueScope()
        await feedAssertFailure(.protocolFailure) {
            try await feedPage(
                scope: scope,
                object: eventChanged(feedObject(scope: scope), key: key, value: value, index: 1))
        }
    }
}

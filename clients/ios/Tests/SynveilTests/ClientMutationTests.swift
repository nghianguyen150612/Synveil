import Foundation
import XCTest

@testable import Synveil

@MainActor
final class ClientMutationTests: XCTestCase {
    private let bridge = MutationTestValidator()

    func testFiveKindVocabularyIsClosed() throws {
        XCTAssertEqual(
            Set(ClientMutationKind.allCases.map(\.rawValue)),
            ["CREATE_DIRECTORY", "RENAME_NODE", "MOVE_NODE", "TRASH_NODE", "RESTORE_NODE"])
        for value in ["PATCH", "UPLOAD", "rename_node", "REPLACE_CONTENT"] {
            XCTAssertThrowsError(
                try JSONDecoder().decode(ClientMutationKind.self, from: Data("\"\(value)\"".utf8)))
        }
    }

    func testCanonicalMutationIdUsesRealRust() async throws {
        let rust = try await RustBridgeAsyncAdapter()
        let id = try await ClientMutationId.validated(mutationUUID(10), using: rust)
        XCTAssertEqual(id.rawValue, mutationUUID(10))
    }

    func testGeneratedIdsUseSharedRustUUIDv7AndAreDistinct() async throws {
        let rust = try await RustBridgeAsyncAdapter()
        var ids: Set<ClientMutationId> = []
        for _ in 0..<32 {
            let id = try await ClientMutationId.generate(using: rust, validator: rust)
            XCTAssertTrue(ids.insert(id).inserted)
        }
    }

    func testNoncanonicalMutationIdsFailClosed() async throws {
        let rust = try await RustBridgeAsyncAdapter()
        for id in [
            "", "bad", mutationUUID(10).uppercased(),
            mutationUUID(10).replacingOccurrences(of: "-7", with: "-4"),
            mutationUUID(10).replacingOccurrences(of: "-8", with: "-c"), mutationUUID(10) + "\n",
        ] {
            do {
                _ = try await ClientMutationId.validated(id, using: rust)
                XCTFail("Invalid identity accepted")
            } catch {}
        }
    }

    func testInvalidDeviceIdRejected() async {
        do {
            _ = try await ClientMutationDeviceId.validated("bad", using: bridge)
            XCTFail()
        } catch {}
    }
    func testInvalidLibraryIdRejected() async {
        do {
            _ = try await LibraryId.validated("bad", using: bridge)
            XCTFail()
        } catch {}
    }
    func testInvalidNodeIdRejected() async {
        do {
            _ = try await NodeId.validated("bad", using: bridge)
            XCTFail()
        } catch {}
    }
    func testInvalidParentIdRejected() async {
        do {
            _ = try await NodeId.validated(mutationUUID(3).uppercased(), using: bridge)
            XCTFail()
        } catch {}
    }

    func testCanonicalBaseDecimals() throws {
        for value in ["0", "1", "9007199254740993", "18446744073709551615"] {
            XCTAssertEqual(try ClientMutationDecimal(validating: value).rawValue, value)
        }
    }
    func testInvalidDecimalTextRejected() {
        for value in ["", "01", "-1", "+1", "1.0", "1e9", " 1", "1\n", "١"] {
            XCTAssertThrowsError(try ClientMutationDecimal(validating: value))
        }
    }
    func testZeroEpochIsNotAuthoritativeBase() async throws {
        let operation = try await mutationPrepared()
        XCTAssertThrowsError(
            try ClientMutationBase(
                scope: operation.base.scope,
                epoch: ClientMutationDecimal(validating: "0"),
                sequence: ClientMutationDecimal(validating: "0")))
    }
    func testExactLargeBaseAndRevisionSurviveEncoding() async throws {
        let large = "18446744073709551615"
        let op = try await mutationPrepared(epoch: large, sequence: large, revision: large)
        let object = try JSONSerialization.jsonObject(with: op.requestBody) as! [String: Any]
        XCTAssertEqual(object["base_epoch"] as? String, large)
        XCTAssertEqual(object["base_sequence"] as? String, large)
        XCTAssertEqual(
            (object["payload"] as? [String: Any])?["expected_revision"] as? String, large)
    }
    func testNamesRemainLogicalMetadataThroughRealRust() async throws {
        let rust = try await RustBridgeAsyncAdapter()
        let op = try await mutationPrepared(name: " ../Logical/A 🚀 ", validator: rust)
        let object = try JSONSerialization.jsonObject(with: op.requestBody) as! [String: Any]
        XCTAssertEqual(
            (object["payload"] as? [String: Any])?["new_name"] as? String, " ../Logical/A 🚀 ")
    }
    func testInvalidNameRejectedThroughRealRust() async throws {
        let rust = try await RustBridgeAsyncAdapter()
        for name in ["", String(repeating: "🚀", count: 257)] {
            do {
                _ = try await mutationPrepared(name: name, validator: rust)
                XCTFail("Invalid name accepted")
            } catch {}
        }
    }
    func testUTF8NameBoundaryPreserved() async throws {
        let rust = try await RustBridgeAsyncAdapter()
        let name = String(repeating: "🚀", count: 256)
        let op = try await mutationPrepared(name: name, validator: rust)
        let node = try await NodeId.validated(mutationUUID(1), using: rust)
        XCTAssertEqual(
            op.payload.intent,
            .renameNode(
                nodeId: node,
                expectedRevision: try NodeRevision(validating: "3"), newName: name))
    }
    func testObviousSameNodeMoveRejected() async throws {
        let node = try await NodeId.validated(mutationUUID(1), using: bridge)
        let revision = try NodeRevision(validating: "3")
        do {
            _ = try await mutationPrepared(
                intent: .moveNode(
                    nodeId: node, expectedRevision: revision,
                    newParentNodeId: node, expectedNewParentRevision: revision))
            XCTFail()
        } catch {}
    }
    func testPreparedBodyIsStableAndRetryNeverRecreatesIdentity() async throws {
        let a = try await mutationPrepared(name: "escaped \" \n 🚀")
        let b = try await mutationPrepared(name: "escaped \" \n 🚀")
        XCTAssertEqual(a, b)
        XCTAssertEqual(a.requestBody, b.requestBody)
        let copiedBody = a.requestBody
        var changed = copiedBody
        changed.append(0)
        XCTAssertEqual(a.requestBody, copiedBody)
        XCTAssertNotEqual(a.requestBody, changed)
    }
    func testRequestBoundIncludesUTF8AndJSONOverhead() {
        XCTAssertNoThrow(
            try ClientMutationPolicy.validateRequestSize(Data(repeating: 0, count: 16384)))
        XCTAssertThrowsError(
            try ClientMutationPolicy.validateRequestSize(Data(repeating: 0, count: 16385)))
    }
    func testOversizedPreparedRequestRejected() async throws {
        // A canonical decimal can exceed the wire body bound even with Rust-valid names.
        do {
            _ = try await mutationPrepared(sequence: String(repeating: "9", count: 16384))
            XCTFail()
        } catch { XCTAssertEqual(error as? ClientMutationFailure, .payloadTooLarge) }
    }
    func testPrivateValuesAreRedacted() async throws {
        let op = try await mutationPrepared(name: "private-filename")
        for text in [
            String(describing: op), String(reflecting: op), String(reflecting: op.payload),
            String(reflecting: op.payload.intent),
        ] {
            XCTAssertFalse(text.contains("private-filename"))
            XCTAssertFalse(text.contains(mutationBearer))
        }
    }

    func testValidAppliedResponseAndNoCheckpointAdvancement() async throws {
        let operation = try await mutationPrepared(sequence: "0")
        let before = operation.base
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationApplied(), for: operation)
        guard case .applied(let value) = result else { return XCTFail() }
        XCTAssertFalse(value.replayed)
        XCTAssertEqual(value.journalSequence.rawValue, "9")
        XCTAssertEqual(operation.base, before)
        XCTAssertEqual(operation.base.sequence.rawValue, "0")
        XCTAssertEqual(value.node.revision.rawValue, "9007199254740993")
    }
    func testAppliedReplayFlagPreserved() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationApplied(replayed: true), for: op)
        guard case .applied(let value) = result else { return XCTFail() }
        XCTAssertTrue(value.replayed)
    }
    func testMutationProjectionHasNoReadOnlyNodeDefaults() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationApplied(), for: op)
        guard case .applied(let value) = result else { return XCTFail() }
        XCTAssertNil(value.node.currentVersionId)
        XCTAssertNil(value.node.trashedAt)
    }
    func testOptionalMutationNodeFieldsCanBeAbsent() async throws {
        try await assertApplied { object in
            var data = object["data"] as! [String: Any]
            var node = data["node"] as! [String: Any]
            node.removeValue(forKey: "parent_node_id")
            data["node"] = node
            object["data"] = data
        }
    }
    func testFullConflictDetailsPreserved() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationConflict(full: true), for: op)
        guard case .conflict(let value?, let requestId) = result else { return XCTFail() }
        XCTAssertEqual(value.reason, .revisionMismatch)
        XCTAssertEqual(value.currentRevision?.rawValue, "9007199254740993")
        XCTAssertEqual(value.expectedRevision?.rawValue, "3")
        XCTAssertEqual(value.currentName, "Authoritative name")
        XCTAssertEqual(value.currentState, .active)
        XCTAssertEqual(value.currentParentId?.rawValue, mutationUUID(2))
        XCTAssertEqual(value.serverEpoch.rawValue, "1")
        XCTAssertEqual(value.serverSequence.rawValue, "9")
        XCTAssertTrue(value.replayed)
        XCTAssertEqual(requestId, "mutation-request-01")
    }
    func testConflictOptionalFieldsMayBeAbsent() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationConflict(), for: op)
        guard case .conflict(let value?, _) = result else { return XCTFail() }
        XCTAssertNil(value.expectedRevision)
        XCTAssertNil(value.currentName)
        XCTAssertNil(value.currentState)
        XCTAssertNil(value.currentParentId)
    }
    func testConflictDetailsMayBeAbsent() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(409, "mutation_conflict"), for: op)
        XCTAssertEqual(result, .conflict(nil, requestId: "mutation-request-01"))
    }
    func testAllConflictReasonsDecodeWithoutResolution() async throws {
        let op = try await mutationPrepared()
        for reason in ClientMutationConflictReason.allCases {
            let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
                mutationConflict(reason: reason.rawValue), for: op)
            guard case .conflict(let value?, _) = result else { return XCTFail() }
            XCTAssertEqual(value.reason, reason)
        }
    }
    func testUnknownConflictReasonRejected() async throws {
        await assertInvalid(mutationConflict(reason: "LAST_WRITE_WINS"))
    }
    func testNullConflictDetailsRejected() async throws {
        await assertInvalid(mutationError(409, "mutation_conflict", details: NSNull()))
    }
    func testWrongHTTPCodeNeverUsesMessageForClassification() async throws {
        await assertInvalid(mutationError(503, "authentication_failed"))
        await assertInvalid(mutationError(409, "internal_error"))
    }

    func testCreateDirectoryExactPayloadSchema() async throws {
        let parent = try await NodeId.validated(mutationUUID(2), using: bridge)
        let parentRevision = try NodeRevision(validating: "7")
        let intent: ClientMutationIntent = .createDirectory(
            parentNodeId: parent, expectedParentRevision: parentRevision, name: "Documents")
        let op = try await mutationPrepared(intent: intent)
        let object = try JSONSerialization.jsonObject(with: op.requestBody) as! [String: Any]
        XCTAssertEqual(
            Set(object.keys), ["mutation_id", "base_epoch", "base_sequence", "kind", "payload"])
        XCTAssertEqual(object["kind"] as? String, intent.kind.rawValue)
        XCTAssertEqual(
            object["payload"] as? [String: String],
            [
                "parent_node_id": mutationUUID(2), "expected_parent_revision": "7",
                "name": "Documents",
            ])
    }

    func testRenameNodeExactPayloadSchema() async throws {
        let node = try await NodeId.validated(mutationUUID(1), using: bridge)
        let revision = try NodeRevision(validating: "3")
        let intent: ClientMutationIntent = .renameNode(
            nodeId: node, expectedRevision: revision, newName: "Report.txt")
        let op = try await mutationPrepared(intent: intent)
        let object = try JSONSerialization.jsonObject(with: op.requestBody) as! [String: Any]
        XCTAssertEqual(
            Set(object.keys), ["mutation_id", "base_epoch", "base_sequence", "kind", "payload"])
        XCTAssertEqual(object["kind"] as? String, intent.kind.rawValue)
        XCTAssertEqual(
            object["payload"] as? [String: String],
            ["node_id": mutationUUID(1), "expected_revision": "3", "new_name": "Report.txt"])
    }

    func testMoveNodeExactPayloadSchema() async throws {
        let node = try await NodeId.validated(mutationUUID(1), using: bridge)
        let parent = try await NodeId.validated(mutationUUID(2), using: bridge)
        let revision = try NodeRevision(validating: "3")
        let parentRevision = try NodeRevision(validating: "7")
        let intent: ClientMutationIntent = .moveNode(
            nodeId: node, expectedRevision: revision, newParentNodeId: parent,
            expectedNewParentRevision: parentRevision)
        let op = try await mutationPrepared(intent: intent)
        let object = try JSONSerialization.jsonObject(with: op.requestBody) as! [String: Any]
        XCTAssertEqual(
            Set(object.keys), ["mutation_id", "base_epoch", "base_sequence", "kind", "payload"])
        XCTAssertEqual(object["kind"] as? String, intent.kind.rawValue)
        XCTAssertEqual(
            object["payload"] as? [String: String],
            [
                "node_id": mutationUUID(1), "expected_revision": "3",
                "new_parent_node_id": mutationUUID(2), "expected_new_parent_revision": "7",
            ])
    }

    func testTrashNodeExactPayloadSchema() async throws {
        let node = try await NodeId.validated(mutationUUID(1), using: bridge)
        let revision = try NodeRevision(validating: "3")
        let intent: ClientMutationIntent = .trashNode(nodeId: node, expectedRevision: revision)
        let op = try await mutationPrepared(intent: intent)
        let object = try JSONSerialization.jsonObject(with: op.requestBody) as! [String: Any]
        XCTAssertEqual(
            Set(object.keys), ["mutation_id", "base_epoch", "base_sequence", "kind", "payload"])
        XCTAssertEqual(object["kind"] as? String, intent.kind.rawValue)
        XCTAssertEqual(
            object["payload"] as? [String: String],
            ["node_id": mutationUUID(1), "expected_revision": "3"])
    }

    func testRestoreNodeExactPayloadSchema() async throws {
        let node = try await NodeId.validated(mutationUUID(1), using: bridge)
        let parent = try await NodeId.validated(mutationUUID(2), using: bridge)
        let revision = try NodeRevision(validating: "3")
        let parentRevision = try NodeRevision(validating: "7")
        let intent: ClientMutationIntent = .restoreNode(
            nodeId: node, expectedRevision: revision, expectedParentNodeId: parent,
            expectedParentRevision: parentRevision)
        let op = try await mutationPrepared(intent: intent)
        let object = try JSONSerialization.jsonObject(with: op.requestBody) as! [String: Any]
        XCTAssertEqual(
            Set(object.keys), ["mutation_id", "base_epoch", "base_sequence", "kind", "payload"])
        XCTAssertEqual(object["kind"] as? String, intent.kind.rawValue)
        XCTAssertEqual(
            object["payload"] as? [String: String],
            [
                "node_id": mutationUUID(1), "expected_revision": "3",
                "expected_parent_node_id": mutationUUID(2), "expected_parent_revision": "7",
            ])
    }

    func testWrongMutationIdRejected() async throws {
        try await invalidAppliedData { $0["mutation_id"] = mutationUUID(99) }
    }

    func testWrongMutationKindRejected() async throws {
        try await invalidAppliedData { $0["kind"] = "TRASH_NODE" }
    }

    func testUnknownMutationKindRejected() async throws {
        try await invalidAppliedData { $0["kind"] = "PATCH" }
    }

    func testWrongOutcomeRejected() async throws {
        try await invalidAppliedData { $0["outcome"] = "PENDING" }
    }

    func testInvalidJournalIdRejected() async throws {
        try await invalidAppliedData { $0["journal_event_id"] = "bad" }
    }

    func testInvalidJournalSequenceRejected() async throws {
        try await invalidAppliedData { $0["journal_sequence"] = "01" }
    }

    func testNumericJournalSequenceRejected() async throws {
        try await invalidAppliedData { $0["journal_sequence"] = 9 }
    }

    func testNullReplayFlagRejected() async throws {
        try await invalidAppliedData { $0["replayed"] = NSNull() }
    }

    func testNumericReplayFlagRejected() async throws {
        try await invalidAppliedData { $0["replayed"] = 1 }
    }

    func testUnknownAppliedFieldRejected() async throws {
        try await invalidAppliedData { $0["extra"] = true }
    }

    func testCrossLibraryNodeRejected() async throws {
        try await invalidAppliedNode { $0["library_id"] = mutationUUID(81) }
    }

    func testWrongTargetNodeRejected() async throws {
        try await invalidAppliedNode { $0["id"] = mutationUUID(99) }
    }

    func testInvalidResponseNodeIdRejected() async throws {
        try await invalidAppliedNode { $0["id"] = "bad" }
    }

    func testInvalidResponseParentIdRejected() async throws {
        try await invalidAppliedNode { $0["parent_node_id"] = "bad" }
    }

    func testInvalidVersionIdRejected() async throws {
        try await invalidAppliedNode { $0["current_version_id"] = "bad" }
    }

    func testInvalidNodeRevisionRejected() async throws {
        try await invalidAppliedNode { $0["revision"] = "01" }
    }

    func testNumericNodeRevisionRejected() async throws {
        try await invalidAppliedNode { $0["revision"] = 3 }
    }

    func testInvalidNodeNameRejected() async throws {
        try await invalidAppliedNode { $0["name"] = "" }
    }

    func testUnknownNodeKindRejected() async throws {
        try await invalidAppliedNode { $0["kind"] = "LINK" }
    }

    func testPurgingMutationNodeRejected() async throws {
        try await invalidAppliedNode { $0["state"] = "PURGING" }
    }

    func testUnknownNodeStateRejected() async throws {
        try await invalidAppliedNode { $0["state"] = "DELETED" }
    }

    func testInvalidCreatedTimestampRejected() async throws {
        try await invalidAppliedNode { $0["created_at"] = "2026-02-30T12:00:00Z" }
    }

    func testInvalidUpdatedTimestampRejected() async throws {
        try await invalidAppliedNode { $0["updated_at"] = "bad" }
    }

    func testInvalidTrashedTimestampRejected() async throws {
        try await invalidAppliedNode { $0["trashed_at"] = "bad" }
    }

    func testNullOptionalNodeFieldRejected() async throws {
        try await invalidAppliedNode { $0["parent_node_id"] = NSNull() }
    }

    func testInventedReadOnlyNodeFieldRejected() async throws {
        try await invalidAppliedNode { $0["purge_eligible"] = false }
    }

    func testUnknownNodeFieldRejected() async throws {
        try await invalidAppliedNode { $0["storage_key"] = "private" }
    }

    func testUnknownEnvelopeFieldRejected() async throws {
        try await invalidApplied { $0["extra"] = true }
    }

    func testMissingRequiredAppliedFieldsRejected() async throws {
        for key in [
            "outcome", "mutation_id", "kind", "replayed", "node", "journal_event_id",
            "journal_sequence",
        ] {
            try await invalidAppliedData { $0.removeValue(forKey: key) }
        }
        for key in [
            "id", "library_id", "kind", "state", "name", "revision", "created_at", "updated_at",
        ] {
            try await invalidAppliedNode { $0.removeValue(forKey: key) }
        }
    }

    func testMissingAndUnknownMetadataRejected() async throws {
        try await invalidApplied { $0.removeValue(forKey: "meta") }
        try await invalidApplied { $0["meta"] = ["request_id": "bad"] }
        try await invalidApplied {
            $0["meta"] = ["request_id": "mutation-request-01", "extra": "bad"]
        }
    }

    func testMalformedJSONRejected() async throws {
        await assertInvalid(mutationHTTP(200, Data("{bad".utf8)))
    }

    func testInvalidOrMismatchedRequestIdHeaderRejected() async throws {
        for id in ["bad", "different-request-01"] {
            await assertInvalid(
                mutationHTTP(
                    200, mutationApplied().body,
                    headers: ["Content-Type": "application/json", "X-Request-Id": id]))
        }
    }

    func testBoundedResponseRejected() async throws {
        await assertInvalid(
            mutationHTTP(
                200, Data(repeating: 32, count: ClientMutationPolicy.maximumResponseBytes + 1)))
    }

    func testIncorrectContentTypeRejected() async throws {
        await assertInvalid(
            mutationHTTP(200, mutationApplied().body, headers: ["Content-Type": "text/html"]))
    }

    func testStrictConflictShapeAndScope() async throws {
        let good = mutationConflict(full: true)
        let original = try JSONSerialization.jsonObject(with: good.body) as! [String: Any]
        for (key, value) in [
            ("extra", "bad"), ("resource_id", mutationUUID(99)), ("conflict_id", "bad"),
            ("current_revision", "01"), ("current_state", "DELETED"), ("current_parent_id", "bad"),
            ("current_name", ""), ("server_sequence", "01"), ("outcome", "APPLIED"),
        ] {
            var error = original["error"] as! [String: Any]
            var details = error["details"] as! [String: Any]
            details[key] = value
            error["details"] = details
            await assertInvalid(
                mutationHTTP(409, try JSONSerialization.data(withJSONObject: ["error": error])))
        }
    }

    func testMutationIdConflictClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(409, "mutation_id_conflict"), for: op)
        XCTAssertEqual(result, .mutationIdConflict(requestId: "mutation-request-01"))
    }

    func testRebaselineRequiredClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(409, "sync_rebaseline_required"), for: op)
        XCTAssertEqual(result, .rebaselineRequired(requestId: "mutation-request-01"))
    }

    func testPayloadTooLargeClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(413, "payload_too_large"), for: op)
        XCTAssertEqual(result, .failed(.payloadTooLarge))
    }

    func testDependencyUnavailableClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(503, "dependency_unavailable"), for: op)
        XCTAssertEqual(result, .failed(.transientFailure(.dependencyUnavailable)))
    }

    func testInvalidMutationClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(400, "invalid_mutation"), for: op)
        XCTAssertEqual(result, .failed(.permanentRejection(.invalidMutation)))
    }

    func testAuthenticationFailureClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(401, "authentication_failed"), for: op)
        XCTAssertEqual(result, .failed(.authenticationRejected))
    }

    func testDeviceRevokedClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(401, "device_revoked"), for: op)
        XCTAssertEqual(result, .failed(.deviceRevoked))
    }

    func testPermissionDeniedClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(403, "permission_denied"), for: op)
        XCTAssertEqual(result, .failed(.permanentRejection(.permissionDenied)))
    }

    func testNotFoundClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(404, "not_found"), for: op)
        XCTAssertEqual(result, .failed(.permanentRejection(.notFound)))
    }

    func testInvalidPersistedDataClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(500, "invalid_persisted_data"), for: op)
        XCTAssertEqual(result, .failed(.permanentRejection(.invalidPersistedData)))
    }

    func testInternalErrorClassification() async throws {
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            mutationError(500, "internal_error"), for: op)
        XCTAssertEqual(result, .failed(.transientFailure(.internalError)))
    }

    func testAllFiveKindsValidateAppliedResponse() async throws {
        let node = try await NodeId.validated(mutationUUID(1), using: bridge)
        let parent = try await NodeId.validated(mutationUUID(2), using: bridge)
        let revision = try NodeRevision(validating: "3")
        let parentRevision = try NodeRevision(validating: "7")
        let intents: [ClientMutationIntent] = [
            .createDirectory(
                parentNodeId: parent, expectedParentRevision: parentRevision, name: "Documents"),
            .renameNode(nodeId: node, expectedRevision: revision, newName: "Documents"),
            .moveNode(
                nodeId: node, expectedRevision: revision, newParentNodeId: parent,
                expectedNewParentRevision: parentRevision),
            .trashNode(nodeId: node, expectedRevision: revision),
            .restoreNode(
                nodeId: node, expectedRevision: revision, expectedParentNodeId: parent,
                expectedParentRevision: parentRevision),
        ]
        for intent in intents {
            let op = try await mutationPrepared(intent: intent)
            var root = mutationAppliedObject()
            var data = root["data"] as! [String: Any]
            data["kind"] = intent.kind.rawValue
            var resultNode = data["node"] as! [String: Any]
            if intent.kind == .createDirectory { resultNode["kind"] = "DIRECTORY" }
            if intent.kind == .trashNode {
                resultNode["state"] = "TRASHED"
                resultNode["trashed_at"] = "2026-10-09T12:00:01Z"
            }
            data["node"] = resultNode
            root["data"] = data
            let response = mutationHTTP(200, try JSONSerialization.data(withJSONObject: root))
            let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
                response, for: op)
            guard case .applied(let value) = result else { return XCTFail() }
            XCTAssertEqual(value.kind, intent.kind)
        }
    }
    func testPreparedRequestAtExact16KiBBoundary() async throws {
        let small = try await mutationPrepared()
        let digits = ClientMutationPolicy.maximumRequestBytes - small.requestBody.count + 1
        let exact = try await mutationPrepared(sequence: String(repeating: "9", count: digits))
        XCTAssertEqual(exact.requestBody.count, ClientMutationPolicy.maximumRequestBytes)
        do {
            _ = try await mutationPrepared(sequence: String(repeating: "9", count: digits + 1))
            XCTFail()
        } catch { XCTAssertEqual(error as? ClientMutationFailure, .payloadTooLarge) }
    }
    func testStrictConflictMissingFieldsAndNullOptionalsRejected() async throws {
        let original =
            try JSONSerialization.jsonObject(with: mutationConflict(full: true).body)
            as! [String: Any]
        let error = original["error"] as! [String: Any]
        let details = error["details"] as! [String: Any]
        for key in [
            "outcome", "replayed", "conflict_id", "reason", "resource_id", "server_epoch",
            "server_sequence",
        ] {
            var changed = details
            changed.removeValue(forKey: key)
            await assertInvalid(mutationError(409, "mutation_conflict", details: changed))
        }
        for key in [
            "expected_revision", "current_revision", "current_state", "current_parent_id",
            "current_name",
        ] {
            var changed = details
            changed[key] = NSNull()
            await assertInvalid(mutationError(409, "mutation_conflict", details: changed))
        }
    }

    func testNameOccupiedConflictMayIdentifyExistingSibling() async throws {
        let root =
            try JSONSerialization.jsonObject(with: mutationConflict(reason: "NAME_OCCUPIED").body)
            as! [String: Any]
        let error = root["error"] as! [String: Any]
        var details = error["details"] as! [String: Any]
        details["resource_id"] = mutationUUID(99)
        let response = mutationError(409, "mutation_conflict", details: details)
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            response, for: op)
        guard case .conflict(let value?, _) = result else { return XCTFail() }
        XCTAssertEqual(value.resourceId.rawValue, mutationUUID(99))
        XCTAssertEqual(value.reason, .nameOccupied)
    }

    private func assertApplied(_ change: (inout [String: Any]) -> Void) async throws {
        var object = mutationAppliedObject()
        change(&object)
        let response = mutationHTTP(200, try JSONSerialization.data(withJSONObject: object))
        let op = try await mutationPrepared()
        let result = try await ClientMutationResponseDecoder(bridge: bridge).decode(
            response, for: op)
        guard case .applied = result else { return XCTFail() }
    }
    private func assertInvalid(_ response: HTTPTransportResponse) async {
        do {
            let op = try await mutationPrepared()
            _ = try await ClientMutationResponseDecoder(bridge: bridge).decode(response, for: op)
            XCTFail("Unverified response accepted")
        } catch {}
    }
    private func invalidApplied(_ change: (inout [String: Any]) -> Void) async throws {
        var object = mutationAppliedObject()
        change(&object)
        await assertInvalid(mutationHTTP(200, try JSONSerialization.data(withJSONObject: object)))
    }
    private func invalidAppliedData(_ change: (inout [String: Any]) -> Void) async throws {
        try await invalidApplied { root in
            var data = root["data"] as! [String: Any]
            change(&data)
            root["data"] = data
        }
    }
    private func invalidAppliedNode(_ change: (inout [String: Any]) -> Void) async throws {
        try await invalidAppliedData { data in
            var node = data["node"] as! [String: Any]
            change(&node)
            data["node"] = node
        }
    }
}

private func mutationUUID(_ number: Int) -> String {
    String(format: "018f0010-abcd-7000-8000-%012x", number)
}
private let mutationBearer = "svd1_" + String(repeating: "a", count: 64)

private struct MutationTestValidator: RustBridgeProtocol {
    func parseSHA256(_ value: String) async throws -> Data { Data() }
    func formatSHA256(_ value: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ value: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ value: String) async throws -> Bool {
        DeviceCredential.isValid(value)
    }
    func validateLibraryID(_ value: String) async throws -> Bool {
        LibraryWireValidation.matches(
            value, pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    }
    func validateNodeID(_ value: String) async throws -> Bool { try await validateLibraryID(value) }
    func validateLogicalName(_ value: String) async throws -> Bool {
        !value.isEmpty && value.utf8.count <= 1024
    }
}

private func mutationPrepared(
    name: String = "Logical/A 🚀", epoch: String = "1", sequence: String = "0",
    revision: String = "3", device: Int = 91, library: Int = 80, owner: Int = 90,
    endpoint: ServerEndpoint? = nil, intent: ClientMutationIntent? = nil,
    validator: any RustBridgeProtocol = MutationTestValidator()
) async throws -> PreparedClientMutation {
    let endpoint =
        try endpoint ?? ServerEndpoint(validating: "https://mutation.example:8443/synveil")
    let scope = ClientMutationScope(
        serverEndpoint: endpoint, ownerUserId: mutationUUID(owner),
        deviceId: try await ClientMutationDeviceId.validated(
            mutationUUID(device), using: validator),
        libraryId: try await LibraryId.validated(mutationUUID(library), using: validator))
    let base = try ClientMutationBase(
        scope: scope, epoch: ClientMutationDecimal(validating: epoch),
        sequence: ClientMutationDecimal(validating: sequence))
    let node = try await NodeId.validated(mutationUUID(1), using: validator)
    let id = try await ClientMutationId.validated(mutationUUID(10), using: validator)
    return try await PreparedClientMutation(
        id: id, base: base,
        intent: intent
            ?? .renameNode(
                nodeId: node, expectedRevision: try NodeRevision(validating: revision),
                newName: name), bridge: validator)
}

private func mutationAppliedObject(replayed: Bool = false) -> [String: Any] {
    [
        "data": [
            "outcome": "APPLIED", "mutation_id": mutationUUID(10), "kind": "RENAME_NODE",
            "replayed": replayed,
            "journal_event_id": mutationUUID(11), "journal_sequence": "9",
            "node": [
                "id": mutationUUID(1), "library_id": mutationUUID(80),
                "parent_node_id": mutationUUID(2),
                "kind": "FILE", "state": "ACTIVE", "name": "Logical/A 🚀",
                "revision": "9007199254740993",
                "created_at": "2026-10-09T12:00:00Z", "updated_at": "2026-10-09T12:00:01Z",
            ],
        ],
        "meta": ["request_id": "mutation-request-01"],
    ]
}
private func mutationHTTP(_ status: Int, _ bytes: Data, headers: [String: String]? = nil)
    -> HTTPTransportResponse
{
    HTTPTransportResponse(
        statusCode: status, headers: headers ?? ["Content-Type": "application/json"], body: bytes)
}
private func mutationApplied(replayed: Bool = false) -> HTTPTransportResponse {
    mutationHTTP(
        200, try! JSONSerialization.data(withJSONObject: mutationAppliedObject(replayed: replayed)))
}
private func mutationError(_ status: Int, _ code: String, details: Any? = nil)
    -> HTTPTransportResponse
{
    var error: [String: Any] = [
        "code": code, "message": "Safe error", "request_id": "mutation-request-01",
        "retryable": false,
    ]
    if let details { error["details"] = details }
    return mutationHTTP(status, try! JSONSerialization.data(withJSONObject: ["error": error]))
}
private func mutationConflict(full: Bool = false, reason: String = "REVISION_MISMATCH")
    -> HTTPTransportResponse
{
    var details: [String: Any] = [
        "outcome": "CONFLICT", "replayed": true, "conflict_id": mutationUUID(12),
        "reason": reason, "resource_id": mutationUUID(1), "server_epoch": "1",
        "server_sequence": "9",
    ]
    if full {
        details["expected_revision"] = "3"
        details["current_revision"] = "9007199254740993"
        details["current_state"] = "ACTIVE"
        details["current_parent_id"] = mutationUUID(2)
        details["current_name"] = "Authoritative name"
    }
    return mutationError(409, "mutation_conflict", details: details)
}

@MainActor
final class ClientMutationRepositoryTests: XCTestCase {
    func testProductionGateWithoutDurableAuthorizerMakesZeroCalls() async throws {
        let f = try fixture(authorizer: nil)
        let op = try await mutationPrepared()
        let result = await f.repository.submit(op)
        XCTAssertEqual(result, .failed(.preparationRequired))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testPreparedOperationUsesCorrectDevicePOSTAndHeaders() async throws {
        let f = try fixture()
        let op = try await mutationPrepared()
        guard case .applied = await f.repository.submit(op) else { return XCTFail() }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
        let request = try XCTUnwrap(requests.first)
        XCTAssertEqual(request.method, .post)
        XCTAssertEqual(
            request.url.absoluteString,
            "https://mutation.example:8443/synveil/api/v1/devices/\(mutationUUID(91))/libraries/\(mutationUUID(80))/mutations"
        )
        XCTAssertEqual(request.body, op.requestBody)
        XCTAssertEqual(request.headers["Authorization"], "Bearer " + mutationBearer)
        XCTAssertEqual(request.headers["Content-Type"], "application/json")
        XCTAssertEqual(request.headers["Accept"], "application/json")
        XCTAssertEqual(request.headers["Accept-Encoding"], "identity")
        XCTAssertEqual(request.headers["User-Agent"], "Synveil/0.1.0 (iOS)")
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertNil(request.headers["X-CSRF-Token"])
        XCTAssertNil(request.url.user)
        XCTAssertNil(request.url.query)
        XCTAssertFalse(request.url.absoluteString.contains(mutationBearer))
        XCTAssertFalse(String(decoding: op.requestBody, as: UTF8.self).contains(mutationBearer))
        XCTAssertFalse(String(reflecting: request).contains(mutationBearer))
    }
    func testNoAutomaticPOSTReplayAfterTimeout() async throws {
        let f = try fixture(error: .timeout)
        let op = try await mutationPrepared()
        let result = await f.repository.submit(op)
        XCTAssertEqual(result, .outcomeUnknown(.timeout))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
        XCTAssertEqual(requests.first?.body, op.requestBody)
    }
    func testExplicitReplayRetainsByteIdentityBaseAndPayload() async throws {
        let f = try fixture(responses: [mutationApplied(), mutationApplied(replayed: true)])
        let op = try await mutationPrepared()
        guard case .applied(let first) = await f.repository.submit(op) else { return XCTFail() }
        guard case .applied(let second) = await f.repository.submit(op) else { return XCTFail() }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(requests[0], requests[1])
        XCTAssertEqual(first.mutationId, second.mutationId)
        XCTAssertTrue(second.replayed)
        XCTAssertEqual(op.base.sequence.rawValue, "0")
    }
    func testOutcomeUnknownRetainsPreparedOperationForExplicitRetry() async throws {
        let f = try fixture(responses: [
            mutationHTTP(200, Data("bad".utf8)), mutationApplied(replayed: true),
        ])
        let op = try await mutationPrepared()
        let original = op
        let first = await f.repository.submit(op)
        XCTAssertEqual(first, .outcomeUnknown(.protocolFailure))
        guard case .applied(let second) = await f.repository.submit(op) else { return XCTFail() }
        XCTAssertTrue(second.replayed)
        XCTAssertEqual(op, original)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests[0].body, requests[1].body)
    }
    func testNoRequestWhenUnauthenticated() async throws {
        let f = try fixture(authenticated: false)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.unauthenticated))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testCredentialUnavailableProducesZeroCalls() async throws {
        let f = try fixture()
        await f.store.replace(nil)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.credentialUnavailable))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testWrongDeviceCannotUseCurrentBearerOrCheckpoint() async throws {
        let f = try fixture()
        let result = await f.repository.submit(try await mutationPrepared(device: 99))
        XCTAssertEqual(result, .failed(.scopeMismatch))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testWrongOwnerCannotUseCurrentSession() async throws {
        let f = try fixture()
        let result = await f.repository.submit(try await mutationPrepared(owner: 99))
        XCTAssertEqual(result, .failed(.scopeMismatch))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testWrongOriginCannotDispatch() async throws {
        let f = try fixture()
        let wrong = try ServerEndpoint(validating: "https://wrong.example")
        let result = await f.repository.submit(try await mutationPrepared(endpoint: wrong))
        XCTAssertEqual(result, .failed(.scopeMismatch))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testRejectedDurableAuthorizationMakesZeroCalls() async throws {
        let f = try fixture(authorizer: MutationTestAuthorizer(rejection: .preparationRequired))
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.preparationRequired))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testOversizedPreparationNeverReachesTransport() async throws {
        let f = try fixture()
        do {
            let op = try await mutationPrepared(sequence: String(repeating: "9", count: 16384))
            _ = await f.repository.submit(op)
            XCTFail()
        } catch {}
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testProviderRechecksSizeAndIdentityBeforeDispatch() async throws {
        let f = try fixture()
        let scope = try await f.provider.begin()
        let op = try await mutationPrepared(device: 99)
        do {
            _ = try await f.provider.submitMutation(op, scope: scope, onDispatch: { XCTFail() })
            XCTFail()
        } catch { XCTAssertEqual(error as? ClientMutationFailure, .scopeMismatch) }
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func test401UsesCurrentSessionRecoveryWithoutDeletingCredentials() async throws {
        let f = try fixture(responses: [mutationError(401, "authentication_failed")])
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.authenticationRejected))
        XCTAssertEqual(f.controller.state, .recoveryRequired(.authentication))
        let deletes = await f.store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }
    func testDocumentedDeviceRevocationUsesExistingRecovery() async throws {
        let f = try fixture(responses: [mutationError(401, "device_revoked")])
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.deviceRevoked))
        XCTAssertEqual(f.controller.state, .recoveryRequired(.deviceRevoked))
        let deletes = await f.store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }
    func test503RetainsSessionAndCredentials() async throws {
        let f = try fixture(responses: [mutationError(503, "dependency_unavailable")])
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.transientFailure(.dependencyUnavailable)))
        XCTAssertEqual(f.controller.state, .authenticated)
        let deletes = await f.store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }
    func test404NeverImpliesDeviceRevocation() async throws {
        let f = try fixture(responses: [mutationError(404, "not_found")])
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.permanentRejection(.notFound)))
        XCTAssertEqual(f.controller.state, .authenticated)
    }
    func testRedirectResponseDoesNotFollowAnotherEndpoint() async throws {
        let f = try fixture(responses: [
            mutationHTTP(307, Data(), headers: ["Location": "https://wrong.example"])
        ])
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.redirectRejected))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testMalformedSuccessNeverPublishesApplied() async throws {
        let f = try fixture(responses: [mutationHTTP(200, Data("{}".utf8))])
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .outcomeUnknown(.protocolFailure))
        XCTAssertEqual(f.controller.state, .authenticated)
    }
    func testCancellationBeforeDispatchIsProvenLocalCancellation() async throws {
        let gate = MutationTestGate()
        let f = try fixture(authorizer: MutationTestAuthorizer(gate: gate))
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.cancelled))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testCancellationAfterDispatchDoesNotClaimRollback() async throws {
        let f = try fixture(suspended: true)
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await f.transport.wait()
        task.cancel()
        await f.transport.release()
        let result = await task.value
        XCTAssertEqual(result, .outcomeUnknown(.cancelled))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testLogoutBeforeDispatchMakesZeroNetworkCalls() async throws {
        let gate = MutationTestGate()
        let f = try fixture(authorizer: MutationTestAuthorizer(gate: gate))
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testNoRequestDuringLogoutCleanup() async throws {
        let f = try fixture()
        let gate = MutationTestGate()
        await f.store.suspendDeletion(gate)
        let logout = Task { await f.controller.requestLogout() }
        await gate.wait()
        XCTAssertEqual(f.controller.state, .logoutInProgress)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .failed(.unauthenticated))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
        await gate.release()
        await logout.value
    }
    func testLateAppliedAfterLogoutIsUnknownAndUnpublished() async throws {
        let f = try fixture(suspended: true)
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await f.transport.wait()
        await f.controller.requestLogout()
        await f.transport.release()
        let result = await task.value
        XCTAssertEqual(result, .outcomeUnknown(.staleSession))
        XCTAssertNotEqual(f.controller.state, .authenticated)
    }
    func testLateAuthorizationRejectionCannotRecoverNewSession() async throws {
        let f = try fixture(
            responses: [mutationError(401, "authentication_failed")], suspended: true)
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await f.transport.wait()
        await f.controller.requestLogout()
        let session = try mutationSession(
            endpoint: f.endpoint, bearer: "svd1_" + String(repeating: "b", count: 64))
        await f.store.replace(session)
        f.controller.requireEnrollment()
        f.controller.markAuthenticated(after: SecureCredentialPersistenceReceipt(session: session))
        XCTAssertEqual(f.controller.state, .authenticated)
        await f.transport.release()
        let result = await task.value
        XCTAssertEqual(result, .outcomeUnknown(.staleSession))
        XCTAssertEqual(f.controller.state, .authenticated)
    }
    func testCredentialReplacementInvalidatesLateResult() async throws {
        let f = try fixture(suspended: true)
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await f.transport.wait()
        await f.store.replace(
            try mutationSession(
                endpoint: f.endpoint, bearer: "svd1_" + String(repeating: "b", count: 64)))
        await f.transport.release()
        let result = await task.value
        XCTAssertEqual(result, .outcomeUnknown(.staleSession))
        XCTAssertEqual(f.controller.state, .authenticated)
    }
    func testLateTransportFailureAfterReplacementStillFenced() async throws {
        let f = try fixture(error: .timeout, suspended: true)
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await f.transport.wait()
        await f.store.replace(
            try mutationSession(
                endpoint: f.endpoint, bearer: "svd1_" + String(repeating: "b", count: 64)))
        await f.transport.release()
        let result = await task.value
        XCTAssertEqual(result, .outcomeUnknown(.staleSession))
    }
    func testLogoutDuringRustResponseValidationCannotPublish() async throws {
        let gate = MutationTestGate()
        let f = try fixture(validator: MutationGatedValidator(gate: gate))
        let op = try await mutationPrepared()
        let task = Task { await f.repository.submit(op) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .outcomeUnknown(.staleSession))
    }

    func testTLSFailureAfterDispatchRetainsUnknownOutcome() async throws {
        let f = try fixture(error: .tlsError)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .outcomeUnknown(.tlsFailure))
        XCTAssertEqual(f.controller.state, .authenticated)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testResponseLossAfterDispatchRetainsUnknownOutcome() async throws {
        let f = try fixture(error: .offline)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .outcomeUnknown(.offline))
        XCTAssertEqual(f.controller.state, .authenticated)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testDNSFailureAfterDispatchRetainsUnknownOutcome() async throws {
        let f = try fixture(error: .dnsFailure)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .outcomeUnknown(.dnsFailure))
        XCTAssertEqual(f.controller.state, .authenticated)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testResponseLimitAfterDispatchRetainsUnknownOutcome() async throws {
        let f = try fixture(error: .bodyLimitExceeded)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .outcomeUnknown(.resourceLimit))
        XCTAssertEqual(f.controller.state, .authenticated)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testTransportMalformedResponseAfterDispatchRetainsUnknownOutcome() async throws {
        let f = try fixture(error: .malformedResponse)
        let result = await f.repository.submit(try await mutationPrepared())
        XCTAssertEqual(result, .outcomeUnknown(.protocolFailure))
        XCTAssertEqual(f.controller.state, .authenticated)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testCrossLibraryBaseRequiresDurableScopeAuthorization() async throws {
        let f = try fixture(authorizer: MutationTestAuthorizer(rejection: .scopeMismatch))
        let result = await f.repository.submit(try await mutationPrepared(library: 81))
        XCTAssertEqual(result, .failed(.scopeMismatch))
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testDurableConflictNeverAutomaticallyResubmitsChangedRevision() async throws {
        let f = try fixture(responses: [mutationConflict(full: true)])
        let op = try await mutationPrepared()
        let result = await f.repository.submit(op)
        guard case .conflict = result else { return XCTFail() }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
        XCTAssertEqual(requests.first?.body, op.requestBody)
    }
    func testMutationIdentityConflictDoesNotGenerateReplacement() async throws {
        let f = try fixture(responses: [mutationError(409, "mutation_id_conflict")])
        let op = try await mutationPrepared()
        let result = await f.repository.submit(op)
        XCTAssertEqual(result, .mutationIdConflict(requestId: "mutation-request-01"))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
        XCTAssertEqual(requests.first?.body, op.requestBody)
    }
    func testRebaselineDoesNotRetryOrAdvanceBase() async throws {
        let f = try fixture(responses: [mutationError(409, "sync_rebaseline_required")])
        let op = try await mutationPrepared()
        let before = op.base
        let result = await f.repository.submit(op)
        XCTAssertEqual(result, .rebaselineRequired(requestId: "mutation-request-01"))
        XCTAssertEqual(op.base, before)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    private struct Fixture {
        let endpoint: ServerEndpoint
        let controller: SessionController
        let store: MutationTestStore
        let transport: MutationTestTransport
        let provider: AuthenticatedLibraryRequestProvider
        let repository: AuthenticatedClientMutationRepository
    }
    private func fixture(
        responses: [HTTPTransportResponse] = [mutationApplied()],
        error: SynveilTransportError? = nil, suspended: Bool = false, authenticated: Bool = true,
        authorizer: (any ClientMutationPreparationAuthorizerProtocol)? = MutationTestAuthorizer(),
        validator: any RustBridgeProtocol = MutationTestValidator()
    ) throws -> Fixture {
        let endpoint = try ServerEndpoint(validating: "https://mutation.example:8443/synveil")
        let session = try mutationSession(endpoint: endpoint)
        let store = MutationTestStore(session)
        let controller = SessionController(
            logoutService: SessionLogoutService(credentialStore: store))
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()
        if authenticated {
            controller.markAuthenticated(
                after: SecureCredentialPersistenceReceipt(session: session))
        }
        let transport = MutationTestTransport(
            responses: responses, error: error, suspended: suspended)
        let provider = AuthenticatedLibraryRequestProvider(
            controller: controller, store: store, transport: transport)
        return Fixture(
            endpoint: endpoint, controller: controller, store: store, transport: transport,
            provider: provider,
            repository: AuthenticatedClientMutationRepository(
                provider: provider, bridge: validator, authorizer: authorizer))
    }
}

private func mutationSession(endpoint: ServerEndpoint, bearer: String = mutationBearer) throws
    -> DeviceCredentialSession
{
    let record = try DeviceCredentialRecord(
        ownerUserId: mutationUUID(90), deviceId: mutationUUID(91),
        credentialId: mutationUUID(92), credential: DeviceCredential(validatedRawValue: bearer),
        createdAt: "2026-10-09T12:00:00Z")
    return DeviceCredentialSession(serverEndpoint: endpoint, record: record)
}

@MainActor
private struct MutationTestAuthorizer: ClientMutationPreparationAuthorizerProtocol {
    var gate: MutationTestGate?
    var rejection: ClientMutationFailure?
    func authorizePersistedSubmission(_ mutation: PreparedClientMutation) async throws {
        if let gate { await gate.suspend() }
        if let rejection { throw rejection }
    }
}

private actor MutationTestGate {
    private var entered = false
    private var enteredWaiter: CheckedContinuation<Void, Never>?
    private var suspended: CheckedContinuation<Void, Never>?
    func suspend() async {
        entered = true
        enteredWaiter?.resume()
        enteredWaiter = nil
        await withCheckedContinuation { suspended = $0 }
    }
    func wait() async {
        if entered { return }
        await withCheckedContinuation { enteredWaiter = $0 }
    }
    func release() {
        suspended?.resume()
        suspended = nil
    }
}

private actor MutationTestTransport: HTTPTransportProtocol {
    private var responses: [HTTPTransportResponse]
    private let error: SynveilTransportError?
    private var recorded: [HTTPTransportRequest] = []
    private let gate: MutationTestGate?
    init(responses: [HTTPTransportResponse], error: SynveilTransportError?, suspended: Bool) {
        self.responses = responses
        self.error = error
        gate = suspended ? MutationTestGate() : nil
    }
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        recorded.append(request)
        if let gate { await gate.suspend() }
        if let error { throw error }
        guard !responses.isEmpty else { throw SynveilTransportError.malformedResponse }
        return responses.removeFirst()
    }
    func requests() -> [HTTPTransportRequest] { recorded }
    func wait() async { await gate?.wait() }
    func release() async { await gate?.release() }
}

private actor MutationTestStore: SecureCredentialSinkProtocol {
    private var session: DeviceCredentialSession?
    private var deletes = 0
    private var deletionGate: MutationTestGate?
    init(_ session: DeviceCredentialSession) { self.session = session }
    func preflight() async throws {}
    func store(_ record: DeviceCredentialRecord, for endpoint: ServerEndpoint) async throws
        -> SecureCredentialPersistenceReceipt
    {
        throw SecureCredentialSinkError.writeFailure
    }
    func update(_ record: DeviceCredentialRecord, for endpoint: ServerEndpoint) async throws
        -> SecureCredentialPersistenceReceipt
    {
        throw SecureCredentialSinkError.writeFailure
    }
    func load(expectedServerEndpoint: ServerEndpoint?) async throws -> DeviceCredentialSession {
        guard let session else { throw SecureCredentialSinkError.itemNotFound }
        if let expectedServerEndpoint, expectedServerEndpoint != session.serverEndpoint {
            throw SecureCredentialSinkError.scopeMismatch
        }
        return session
    }
    func delete() async throws {
        deletes += 1
        if let deletionGate { await deletionGate.suspend() }
        session = nil
    }
    func replace(_ session: DeviceCredentialSession?) { self.session = session }
    func deleteCount() -> Int { deletes }
    func suspendDeletion(_ gate: MutationTestGate) { deletionGate = gate }
}

private struct MutationGatedValidator: RustBridgeProtocol {
    let gate: MutationTestGate
    private let bridge = MutationTestValidator()
    func parseSHA256(_ value: String) async throws -> Data { Data() }
    func formatSHA256(_ value: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ value: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ value: String) async throws -> Bool {
        try await bridge.validateDeviceBearerToken(value)
    }
    func validateLibraryID(_ value: String) async throws -> Bool {
        try await bridge.validateLibraryID(value)
    }
    func validateNodeID(_ value: String) async throws -> Bool {
        try await bridge.validateNodeID(value)
    }
    func validateLogicalName(_ value: String) async throws -> Bool {
        await gate.suspend()
        return try await bridge.validateLogicalName(value)
    }
}

import Foundation
import XCTest

@testable import Synveil

@MainActor
final class SyncCheckpointTests: XCTestCase {
    func testCanonicalCheckpointPreservesScopeAndServerZero() async throws {
        let scope = try await queueScope()
        let value = try await queueCheckpoint(scope: scope)
        XCTAssertEqual(value.base.scope, scope)
        XCTAssertEqual(value.base.epoch.rawValue, "1")
        XCTAssertEqual(value.base.sequence.rawValue, "0")
    }
    func testExactMaximumUnsignedDecimalsPreserved() async throws {
        let scope = try await queueScope()
        let large = "18446744073709551615"
        let value = try await queueCheckpoint(scope: scope, epoch: large, sequence: large)
        XCTAssertEqual(value.base.epoch.rawValue, large)
        XCTAssertEqual(value.base.sequence.rawValue, large)
    }
    func testInvalidDeviceIdentityRejected() async throws {
        try await rejectData("device_id", value: "bad")
    }
    func testNoncanonicalDeviceIdentityRejected() async throws {
        try await rejectData("device_id", value: queueUUID(91).uppercased())
    }
    func testCrossDeviceResponseRejected() async throws {
        try await rejectData("device_id", value: queueUUID(99))
    }
    func testInvalidLibraryIdentityRejected() async throws {
        try await rejectData("library_id", value: "bad")
    }
    func testCrossLibraryResponseRejected() async throws {
        try await rejectData("library_id", value: queueUUID(99))
    }
    func testZeroEpochRejected() async throws { try await rejectData("epoch", value: "0") }
    func testNoncanonicalEpochRejected() async throws { try await rejectData("epoch", value: "01") }
    func testOverflowEpochRejected() async throws {
        try await rejectData("epoch", value: "18446744073709551616")
    }
    func testNumericEpochRejected() async throws { try await rejectData("epoch", value: 1) }
    func testNoncanonicalSequenceRejected() async throws {
        try await rejectData("acknowledged_sequence", value: "+1")
    }
    func testOverflowSequenceRejected() async throws {
        try await rejectData("acknowledged_sequence", value: "18446744073709551616")
    }
    func testNumericSequenceRejected() async throws {
        try await rejectData("acknowledged_sequence", value: 0)
    }
    func testMalformedTimestampRejected() async throws {
        try await rejectData("created_at", value: "2026-02-30T00:00:00Z")
    }
    func testUpdatedBeforeCreatedRejected() async throws {
        try await rejectData("updated_at", value: "2026-10-08T00:00:00Z")
    }
    func testMissingWatermarkIsValid() async throws {
        let value = try await queueCheckpoint(scope: queueScope())
        XCTAssertNil(value.lastSeenHighWatermark)
    }
    func testOptionalWatermarkPreservesExactValue() async throws {
        let scope = try await queueScope()
        let value = try await decode(
            queueCheckpointObject(
                scope: scope, sequence: "9007199254740993", watermark: "18446744073709551615"),
            scope: scope)
        XCTAssertEqual(value.lastSeenHighWatermark?.rawValue, "18446744073709551615")
    }
    func testWatermarkBelowAcknowledgementRejected() async throws {
        let scope = try await queueScope()
        await queueAssertFailure(.invalidCheckpoint) {
            try await decode(
                queueCheckpointObject(scope: scope, sequence: "5", watermark: "4"), scope: scope)
        }
    }
    func testExplicitNullWatermarkRejected() async throws {
        try await rejectData("last_seen_high_watermark", value: NSNull())
    }
    func testUnknownDataFieldRejected() async throws {
        try await rejectData("future_field", value: "anything")
    }
    func testUnknownRootFieldRejected() async throws {
        let scope = try await queueScope()
        var object = queueCheckpointObject(scope: scope)
        object["extra"] = true
        await queueAssertFailure(.invalidCheckpoint) { try await decode(object, scope: scope) }
    }
    func testUnknownMetaFieldRejected() async throws {
        let scope = try await queueScope()
        var object = queueCheckpointObject(scope: scope)
        object["meta"] = ["request_id": "checkpoint-request-01", "extra": "x"]
        await queueAssertFailure(.invalidCheckpoint) { try await decode(object, scope: scope) }
    }
    func testInvalidRequestIdRejected() async throws {
        let scope = try await queueScope()
        var object = queueCheckpointObject(scope: scope)
        object["meta"] = ["request_id": "secret\nunsafe"]
        await queueAssertFailure(.invalidCheckpoint) { try await decode(object, scope: scope) }
    }
    func testRequestHeaderMismatchRejected() async throws {
        let scope = try await queueScope()
        let response = try queueHTTP(
            queueCheckpointObject(scope: scope),
            headers: ["Content-Type": "application/json", "X-Request-Id": "different-request-01"])
        await queueAssertFailure(.invalidCheckpoint) {
            try await SyncCheckpointResponseDecoder(bridge: QueueValidator()).decode(
                response, scope: scope)
        }
    }
    func testMalformedHTTP200Rejected() async throws {
        let scope = try await queueScope()
        for bytes in [Data("{}".utf8), Data("null".utf8), Data("not json".utf8), Data("[]".utf8)] {
            await queueAssertFailure(.invalidCheckpoint) {
                try await SyncCheckpointResponseDecoder(bridge: QueueValidator()).decode(
                    HTTPTransportResponse(
                        statusCode: 200, headers: ["Content-Type": "application/json"], body: bytes),
                    scope: scope)
            }
        }
    }
    func testRequiredFieldsCannotBeOmitted() async throws {
        let scope = try await queueScope()
        for field in [
            "device_id", "library_id", "epoch", "acknowledged_sequence", "created_at", "updated_at",
        ] {
            var object = queueCheckpointObject(scope: scope)
            var data = object["data"] as! [String: Any]
            data.removeValue(forKey: field)
            object["data"] = data
            await queueAssertFailure(.invalidCheckpoint) { try await decode(object, scope: scope) }
        }
    }
    func testResponseSizeIsBounded() async throws {
        let scope = try await queueScope()
        let response = HTTPTransportResponse(
            statusCode: 200, headers: ["Content-Type": "application/json"],
            body: Data(repeating: 32, count: 16385))
        await queueAssertFailure(.invalidCheckpoint) {
            try await SyncCheckpointResponseDecoder(bridge: QueueValidator()).decode(
                response, scope: scope)
        }
    }
    func testWrongContentTypeRejected() async throws {
        let scope = try await queueScope()
        let response = try queueHTTP(
            queueCheckpointObject(scope: scope), headers: ["Content-Type": "text/html"])
        await queueAssertFailure(.invalidCheckpoint) {
            try await SyncCheckpointResponseDecoder(bridge: QueueValidator()).decode(
                response, scope: scope)
        }
    }
    func testExplicitAuthenticatedDeviceCheckpointRoute() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let before = await f.transport.requests()
        XCTAssertTrue(before.isEmpty)
        let result = await f.service.prepare(scope: f.scope)
        guard case .prepared = result else { return XCTFail() }
        let requests = await f.transport.requests()
        let request = try XCTUnwrap(requests.first)
        XCTAssertEqual(request.method, .get)
        XCTAssertEqual(
            request.url.absoluteString,
            "https://queue.example:8443/synveil/api/v1/devices/\(queueUUID(91))/libraries/\(queueUUID(80))/checkpoint"
        )
        XCTAssertEqual(request.headers["Authorization"], "Bearer " + queueBearer)
        XCTAssertNil(request.url.query)
        XCTAssertNil(request.url.user)
        XCTAssertNil(request.url.password)
        XCTAssertFalse(request.url.absoluteString.contains(queueBearer))
        XCTAssertNil(request.body)
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertNil(request.headers["X-CSRF-Token"])
    }
    func testNoAutomaticCheckpointInitialization() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        _ = await f.queue.recoverInterruptedOperations()
        _ = await f.queue.pending(scope: f.scope, limit: 10)
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testWrongOriginCheckpointPreparationMakesNoRequest() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let wrongScope = try await queueScope(endpoint: "https://wrong.example")
        let result = await f.service.prepare(scope: wrongScope)
        XCTAssertEqual(result, .failed(.scopeMismatch))
        let sent = await f.transport.requests()
        XCTAssertTrue(sent.isEmpty)
    }
    func testCheckpointPersistedTransactionallyAndReopens() async throws {
        let f = try await queueFixture(self)
        let second = try MutationQueueSQLiteStore(url: f.url)
        let stored = try await second.syncBase(scope: f.scope)
        XCTAssertEqual(stored?.status, .verified)
        XCTAssertEqual(stored?.sequence, "0")
        let value = try await SyncCheckpointResponseDecoder(bridge: QueueValidator()).decode(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: XCTUnwrap(stored).responseBody), scope: f.scope)
        XCTAssertEqual(value.base.scope, f.scope)
    }
    func testMissingBaseBlocksEnqueueWithoutDefaultValues() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let result = await f.queue.enqueue(try await queuePrepared())
        XCTAssertEqual(result, .failed(.syncBaseUnavailable))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM mutations"), "0")
    }
    func testOfflineWithVerifiedPersistedBaseCanEnqueue() async throws {
        let f = try await queueFixture(self)
        await f.transport.fail(.offline)
        _ = try await queueEnqueued(f)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testWrongLibraryCannotBorrowExistingBase() async throws {
        let f = try await queueFixture(self)
        let other = try await queuePrepared(scope: queueScope(library: 99))
        let result = await f.queue.enqueue(other)
        XCTAssertEqual(result, .failed(.syncBaseUnavailable))
    }
    func testChangedCheckpointBlocksWithoutRewritingOutstandingMutation() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        await f.transport.set(
            try queueHTTP(queueCheckpointObject(scope: f.scope, epoch: "2", sequence: "9")))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.reconciliationRequired))
        let row = try await queueRecord(f)
        XCTAssertEqual(row.mutation, original.mutation)
        let enqueued = await f.queue.enqueue(
            try await queuePrepared(id: 11, node: 2, epoch: "2", sequence: "9"))
        XCTAssertEqual(enqueued, .failed(.reconciliationRequired))
    }
    func testRewindCheckpointIsReconciliationRequired() async throws {
        let f = try await queueFixture(self)
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "9")))
        _ = await f.service.prepare(scope: f.scope)
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "8")))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.reconciliationRequired))
    }
    func testInvalidCheckpointQuarantinesBaseInsteadOfDefaulting() async throws {
        let f = try await queueFixture(self)
        await f.transport.set(
            try queueHTTP(["data": [:], "meta": ["request_id": "checkpoint-request-01"]]))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.invalidCheckpoint))
        let stored = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(stored?.status, .invalid)
        let enqueued = await f.queue.enqueue(try await queuePrepared())
        XCTAssertEqual(enqueued, .failed(.invalidCheckpoint))
    }
    func testOfflineCheckpointFailureRetainsVerifiedBase() async throws {
        let f = try await queueFixture(self)
        await f.transport.fail(.offline)
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.transport(.offline)))
        let stored = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(stored?.status, .verified)
    }
    func testLateCheckpointAfterLogoutCannotPublishBase() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await f.service.prepare(scope: f.scope) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertNil(base)
    }
    func testLateCheckpointAfterCredentialReplacementRejected() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await f.service.prepare(scope: f.scope) }
        await gate.wait()
        await f.credentials.replace(
            try queueSession(f.scope, bearer: "svd1_" + String(repeating: "b", count: 64)))
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result, .failed(.staleSession))
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertNil(base)
    }
    func testFailedCheckpointCommitLeavesPreviousBase() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let value = try await queueCheckpoint(scope: f.scope, sequence: "2")
        fault.arm(.beforeCommit)
        await queueAssertFailure(.diskFull) { try await f.database.persistCheckpoint(value) }
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.sequence, "0")
    }
    func testForgedCheckpointProvenanceRejected() async throws {
        let f = try await queueFixture(self)
        let verified = try await queueCheckpoint(scope: f.scope)
        let forged = SyncCheckpoint(
            base: try ClientMutationBase(
                scope: f.scope, epoch: ClientMutationDecimal(validating: "2"),
                sequence: verified.base.sequence),
            createdAt: verified.createdAt, updatedAt: verified.updatedAt,
            lastSeenHighWatermark: nil, requestId: verified.requestId,
            responseBody: verified.responseBody)
        let session = try await f.queue.capture(scope: f.scope)
        await queueAssertFailure(.invalidCheckpoint) {
            try await f.queue.persist(checkpoint: forged, session: session)
        }
    }
    func testVerifiedCheckpointAuthenticationRejectionUsesRecoveryOwner() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "authentication_failed", "message": "Safe failure",
                        "request_id": "checkpoint-request-01", "retryable": false,
                    ]
                ], status: 401))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.transport(.authenticationRejected)))
        XCTAssertEqual(f.controller.state, .recoveryRequired(.authentication))
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertNil(base)
    }
    func testVerifiedCheckpointRevocationUsesExistingRecovery() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "device_revoked", "message": "Safe failure",
                        "request_id": "checkpoint-request-01", "retryable": false,
                    ]
                ], status: 401))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.transport(.deviceRevoked)))
        XCTAssertEqual(f.controller.state, .recoveryRequired(.deviceRevoked))
    }
    func testUnverifiedCheckpoint401DoesNotTriggerRecovery() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "unknown_auth_error", "message": "Safe failure",
                        "request_id": "checkpoint-request-01", "retryable": false,
                    ]
                ], status: 401))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.invalidCheckpoint))
        XCTAssertEqual(f.controller.state, .authenticated)
    }
    func testCheckpoint404RetainsCredentialAndDoesNotImplyRevocation() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "not_found", "message": "Safe failure",
                        "request_id": "checkpoint-request-01", "retryable": false,
                    ]
                ], status: 404))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.transport(.httpFailure(statusCode: 404))))
        XCTAssertEqual(f.controller.state, .authenticated)
        let absent = try await f.credentials.isActiveCredentialAbsent()
        XCTAssertFalse(absent)
    }
    func testCheckpoint503RetainsVerifiedBaseAndCredentials() async throws {
        let f = try await queueFixture(self)
        await f.transport.set(
            try queueHTTP(
                [
                    "error": [
                        "code": "dependency_unavailable", "message": "Safe failure",
                        "request_id": "checkpoint-request-01", "retryable": true,
                    ]
                ], status: 503))
        let result = await f.service.prepare(scope: f.scope)
        XCTAssertEqual(result, .failed(.transport(.serverUnavailable)))
        XCTAssertEqual(f.controller.state, .authenticated)
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.status, .verified)
    }

    private func decode(_ object: [String: Any], scope: ClientMutationScope) async throws
        -> SyncCheckpoint
    {
        try await SyncCheckpointResponseDecoder(bridge: QueueValidator()).decode(
            queueHTTP(object), scope: scope)
    }
    private func rejectData(_ key: String, value: Any) async throws {
        let scope = try await queueScope()
        var object = queueCheckpointObject(scope: scope)
        var data = object["data"] as! [String: Any]
        data[key] = value
        object["data"] = data
        await queueAssertFailure(.invalidCheckpoint) { try await decode(object, scope: scope) }
    }
}

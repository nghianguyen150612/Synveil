import Foundation
import XCTest

@testable import Synveil

func syncError(_ code: String, status: Int) throws -> HTTPTransportResponse {
    try queueHTTP(
        [
            "error": [
                "code": code, "message": "Do not retain diagnostic", "retryable": false,
                "request_id": "sync-error-01",
            ]
        ], status: status)
}

@MainActor
final class SyncAckTests: XCTestCase {
    func testControlledAppliedFixtureUsesAuthenticatedAckContract() async throws {
        let (f, projection, service, receipt) = try await ackFixture()
        let result = await service.acknowledge(receipt)
        guard case .confirmed(let checkpoint) = result else { return XCTFail() }
        XCTAssertEqual(checkpoint.base.sequence.rawValue, "2")
        let requests = await f.transport.requests()
        let request = try XCTUnwrap(requests.last)
        XCTAssertEqual(request.method, .post)
        XCTAssertEqual(
            request.url.path,
            "/synveil/api/v1/devices/\(f.scope.deviceId.rawValue)/libraries/\(f.scope.libraryId.rawValue)/changes/ack"
        )
        XCTAssertNil(request.url.query)
        XCTAssertFalse(request.url.absoluteString.contains(receipt.evidence.token))
        XCTAssertEqual(request.headers["Authorization"], "Bearer " + queueBearer)
        XCTAssertEqual(request.headers["Content-Type"], "application/json")
        XCTAssertEqual(request.headers["Accept"], "application/json")
        XCTAssertEqual(request.headers["Accept-Encoding"], "identity")
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertFalse(request.headers.keys.contains { $0.lowercased().contains("csrf") })
        let body = try XCTUnwrap(request.body)
        XCTAssertLessThanOrEqual(body.count, 2048)
        let json = try JSONSerialization.jsonObject(with: body) as! [String: String]
        XCTAssertEqual(json, ["ack_token": receipt.evidence.token])
        let count = await projection.count()
        XCTAssertEqual(count, 1)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "0")
    }
    func testMissingApplicationCommitBlocksReceiptAndPost() async throws {
        let f = try await queueFixture(self)
        _ = try await feedStage(f)
        let projection = try AppliedFeedFixture(try await feedPage(scope: f.scope), allowed: false)
        let service = SyncAckService(
            provider: f.provider, projection: projection, bridge: QueueValidator())
        await feedAssertFailure(.applicationCommitRequired) {
            try await service.receipt(scope: f.scope, position: feedPosition())
        }
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testReceiptFromAnotherCapabilityCannotAuthorizePost() async throws {
        let (f, projection, _, receipt) = try await ackFixture()
        let other = SyncAckService(
            provider: f.provider, projection: projection, bridge: QueueValidator())
        let result = await other.acknowledge(receipt)
        XCTAssertEqual(result, .failed(.applicationCommitRequired))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testRevokedProjectionProofCannotAuthorizePost() async throws {
        let (f, projection, service, receipt) = try await ackFixture()
        await projection.revoke()
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .failed(.applicationCommitRequired))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testRegressionBehindDeliveredPageIsUnknown() async throws {
        try await rejectCheckpoint(sequence: "1", expected: .checkpointConflict)
    }
    func testUnexpectedCheckpointAheadOfProjectionIsUnknown() async throws {
        try await rejectCheckpoint(sequence: "3", expected: .checkpointAheadOfProjection)
    }
    func testEpochMismatchIsUnknownRebaseline() async throws {
        try await rejectCheckpoint(sequence: "2", epoch: "2", expected: .rebaselineRequired)
    }
    func testCrossDeviceCheckpointRejected() async throws {
        try await rejectCheckpoint(
            scope: try await queueScope(device: 99), expected: .protocolFailure)
    }
    func testCrossLibraryCheckpointRejected() async throws {
        try await rejectCheckpoint(
            scope: try await queueScope(library: 99), expected: .protocolFailure)
    }
    func testMalformedCheckpointResponsePreservesUncertainty() async throws {
        let (f, projection, service, receipt) = try await ackFixture()
        await f.transport.set(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: Data("{}".utf8)))
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .outcomeUnknown(.protocolFailure))
        let count = await projection.count()
        XCTAssertEqual(count, 0)
    }
    func testTimeoutAfterDispatchPreservesEvidenceWithoutRetry() async throws {
        let (f, projection, service, receipt) = try await ackFixture()
        await f.transport.fail(.timeout)
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .outcomeUnknown(.transport(.timeout)))
        XCTAssertEqual(receipt.evidence.token, "v1.sync-ack.original-evidence_123")
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 2)
        let count = await projection.count()
        XCTAssertEqual(count, 0)
    }
    func testRedirectRejectedAfterDispatch() async throws {
        let (f, _, service, receipt) = try await ackFixture()
        await f.transport.fail(.redirectRejected(statusCode: 302))
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .outcomeUnknown(.transport(.redirectRejected)))
    }
    func testCheckpointConflictTypedBlockedResult() async throws {
        try await rejectedError("checkpoint_conflict", status: 409, expected: .checkpointConflict)
    }
    func testRebaselineTypedBlockedResult() async throws {
        try await rejectedError(
            "sync_rebaseline_required", status: 409, expected: .rebaselineRequired)
    }
    func testUnavailablePreservesCredentialAndEvidence() async throws {
        let (f, _, service, receipt) = try await ackFixture()
        await f.transport.set(try syncError("dependency_unavailable", status: 503))
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .failed(.transport(.serverUnavailable)))
        let session = try await f.credentials.load(expectedServerEndpoint: f.scope.serverEndpoint)
        XCTAssertEqual(session.record.credential.rawValue, queueBearer)
        XCTAssertEqual(receipt.evidence.token, "v1.sync-ack.original-evidence_123")
    }
    func testResponseLossOnLocalConfirmationIsUnknown() async throws {
        let (_, projection, service, receipt) = try await ackFixture()
        await projection.failConfirmation()
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .outcomeUnknown(.storage(.diskFull)))
    }
    func testCancellationAfterDispatchCannotConfirm() async throws {
        let (f, projection, service, receipt) = try await ackFixture()
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await service.acknowledge(receipt) }
        await gate.wait()
        task.cancel()
        await gate.release()
        guard case .outcomeUnknown = await task.value else { return XCTFail() }
        let count = await projection.count()
        XCTAssertEqual(count, 0)
    }
    func testLateAckAfterLogoutCannotPublishProgress() async throws {
        let (f, projection, service, receipt) = try await ackFixture()
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await service.acknowledge(receipt) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        guard case .outcomeUnknown = await task.value else { return XCTFail() }
        let count = await projection.count()
        XCTAssertEqual(count, 0)
    }
    func testCredentialReplacementFencesAckBeforeDispatch() async throws {
        let (f, _, service, receipt) = try await ackFixture()
        await f.credentials.replace(try queueSession(f.scope, credential: 99))
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .failed(.transport(.staleSession)))
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testScopeMismatchCannotBorrowCredential() async throws {
        let (f, _, service, _) = try await ackFixture()
        let other = try await queueScope(device: 99)
        await feedAssertFailure(.scopeMismatch) {
            try await service.receipt(scope: other, position: feedPosition())
        }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testReplayCheckpointFurtherAheadAcceptedOnlyWithinAppliedProjection() async throws {
        let (f, projection, service, receipt) = try await ackFixture(applied: "4", confirmed: "3")
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "4")))
        guard case .confirmed(let checkpoint) = await service.acknowledge(receipt) else {
            return XCTFail()
        }
        XCTAssertEqual(checkpoint.base.sequence.rawValue, "4")
        let count = await projection.count()
        XCTAssertEqual(count, 1)
    }
    func testRegressionBehindPreviouslyConfirmedCheckpointRejected() async throws {
        let (f, _, service, receipt) = try await ackFixture(applied: "4", confirmed: "3")
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "2")))
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .outcomeUnknown(.checkpointConflict))
    }
    func testReceiptCannotClaimUnappliedSequence() async throws {
        let f = try await queueFixture(self)
        let page = try await feedPage(scope: f.scope)
        let projection = try AppliedFeedFixture(page, applied: "1")
        let service = SyncAckService(
            provider: f.provider, projection: projection, bridge: QueueValidator())
        await feedAssertFailure(.applicationCommitRequired) {
            try await service.receipt(scope: f.scope, position: feedPosition())
        }
    }
    func testEvidenceDescriptionsAreRedacted() async throws {
        let scope = try await queueScope()
        let page = try await feedPage(scope: scope)
        XCTAssertFalse(String(describing: page).contains("original-evidence"))
        XCTAssertFalse(String(reflecting: page.evidence!).contains("original-evidence"))
    }
    private func ackFixture(applied: String? = nil, confirmed: String = "0") async throws -> (
        QueueFixture, AppliedFeedFixture, SyncAckService, AppliedFeedCommitReceipt
    ) {
        let f = try await queueFixture(self)
        let page = try await feedPage(scope: f.scope)
        let projection = try AppliedFeedFixture(page, applied: applied, confirmed: confirmed)
        let service = SyncAckService(
            provider: f.provider, projection: projection, bridge: QueueValidator())
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "2")))
        return (f, projection, service, receipt)
    }
    private func rejectCheckpoint(
        sequence: String = "2", epoch: String = "1", scope: ClientMutationScope? = nil,
        expected: SyncFeedFailure
    ) async throws {
        let (f, projection, service, receipt) = try await ackFixture()
        await f.transport.set(
            try queueHTTP(
                queueCheckpointObject(scope: scope ?? f.scope, epoch: epoch, sequence: sequence)))
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .outcomeUnknown(expected))
        let count = await projection.count()
        XCTAssertEqual(count, 0)
    }
    private func rejectedError(_ code: String, status: Int, expected: SyncFeedFailure) async throws
    {
        let (f, projection, service, receipt) = try await ackFixture()
        await f.transport.set(try syncError(code, status: status))
        let result = await service.acknowledge(receipt)
        XCTAssertEqual(result, .failed(expected))
        let blocked = await projection.blockedReason()
        XCTAssertEqual(blocked, expected)
        let count = await projection.count()
        XCTAssertEqual(count, 0)
    }
}

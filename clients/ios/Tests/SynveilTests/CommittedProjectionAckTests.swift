import Foundation
import XCTest

@testable import Synveil

@MainActor
final class CommittedProjectionAckTests: XCTestCase {
    func testStagedOnlyPageCannotObtainReceipt() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await f.database.stageFeed(
            page, credentialId: queueUUID(92), bridge: QueueValidator())
        await feedAssertFailure(.applicationCommitRequired) {
            try await projectionAck(f).receipt(scope: f.scope, position: page.start)
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "0")
    }
    func testCommittedProjectionObtainsGenuineProof() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let proof = try await projectionStorage(f).claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        XCTAssertEqual(proof.locallyApplied.sequence.rawValue, "1")
        XCTAssertEqual(proof.previouslyConfirmed.sequence.rawValue, "0")
        XCTAssertEqual(proof.evidence.scope, f.scope)
        XCTAssertEqual(proof.commitIdentity.split(separator: "/").count, 2)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "1")
    }
    func testChangedCommitIdentityCannotAuthorizeDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let original = try await storage.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let forged = AppliedSyncPageProof(
            evidence: original.evidence, locallyApplied: original.locallyApplied,
            previouslyConfirmed: original.previouslyConfirmed, commitIdentity: "forged/attempt")
        await feedAssertFailure(.applicationCommitRequired) {
            try await storage.validateAppliedPage(forged, credentialId: queueUUID(92))
        }
    }
    func testChangedAppliedPositionCannotAuthorizeDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let original = try await storage.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let forged = AppliedSyncPageProof(
            evidence: original.evidence, locallyApplied: try feedPosition(from: "2"),
            previouslyConfirmed: original.previouslyConfirmed,
            commitIdentity: original.commitIdentity)
        await feedAssertFailure(.applicationCommitRequired) {
            try await storage.validateAppliedPage(forged, credentialId: queueUUID(92))
        }
    }
    func testChangedConfirmedPositionCannotAuthorizeDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let original = try await storage.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let forged = AppliedSyncPageProof(
            evidence: original.evidence, locallyApplied: original.locallyApplied,
            previouslyConfirmed: try feedPosition(from: "1"),
            commitIdentity: original.commitIdentity)
        await feedAssertFailure(.applicationCommitRequired) {
            try await storage.validateAppliedPage(forged, credentialId: queueUUID(92))
        }
    }
    func testChangedTokenCannotAuthorizeDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let original = try await storage.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let e = SyncAckEvidence(
            scope: original.evidence.scope, epoch: original.evidence.epoch,
            from: original.evidence.from,
            through: original.evidence.through, highWatermark: original.evidence.highWatermark,
            token: "v1.changed-token")
        let forged = AppliedSyncPageProof(
            evidence: e, locallyApplied: original.locallyApplied,
            previouslyConfirmed: original.previouslyConfirmed,
            commitIdentity: original.commitIdentity)
        do {
            try await storage.validateAppliedPage(forged, credentialId: queueUUID(92))
            XCTFail()
        } catch {}
    }
    func testMissingTokenCannotAuthorizeDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let original = try await storage.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let e = SyncAckEvidence(
            scope: original.evidence.scope, epoch: original.evidence.epoch,
            from: original.evidence.from,
            through: original.evidence.through, highWatermark: original.evidence.highWatermark,
            token: "")
        let forged = AppliedSyncPageProof(
            evidence: e, locallyApplied: original.locallyApplied,
            previouslyConfirmed: original.previouslyConfirmed,
            commitIdentity: original.commitIdentity)
        do {
            try await storage.validateAppliedPage(forged, credentialId: queueUUID(92))
            XCTFail()
        } catch {}
    }
    func testChangedScopeCannotAuthorizeDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let original = try await storage.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let e = SyncAckEvidence(
            scope: try await queueScope(device: 93), epoch: original.evidence.epoch,
            from: original.evidence.from,
            through: original.evidence.through, highWatermark: original.evidence.highWatermark,
            token: original.evidence.token)
        let forged = AppliedSyncPageProof(
            evidence: e, locallyApplied: original.locallyApplied,
            previouslyConfirmed: original.previouslyConfirmed,
            commitIdentity: original.commitIdentity)
        do {
            try await storage.validateAppliedPage(forged, credentialId: queueUUID(92))
            XCTFail()
        } catch {}
    }
    func testChangedPageThroughCannotAuthorizeDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let original = try await storage.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let e = SyncAckEvidence(
            scope: original.evidence.scope, epoch: original.evidence.epoch,
            from: original.evidence.from,
            through: try ClientMutationDecimal(validating: "2"),
            highWatermark: original.evidence.highWatermark, token: original.evidence.token)
        let forged = AppliedSyncPageProof(
            evidence: e, locallyApplied: original.locallyApplied,
            previouslyConfirmed: original.previouslyConfirmed,
            commitIdentity: original.commitIdentity)
        do {
            try await storage.validateAppliedPage(forged, credentialId: queueUUID(92))
            XCTFail()
        } catch {}
    }
    func testReceiptCannotBeUsedByDifferentServiceOwner() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let first = projectionAck(f)
        let receipt = try await first.receipt(scope: f.scope, position: feedPosition())
        let result = await projectionAck(f).acknowledge(receipt)
        XCTAssertEqual(result, .failed(.applicationCommitRequired))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT dispatched FROM sync_ack_attempts"), "0")
    }
    func testTwoConcurrentReceiptCallersHaveSingleLease() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let storage = projectionStorage(f)
        let scope = f.scope
        let position = try feedPosition()
        let first = Task {
            try await storage.claimAppliedPage(
                scope: scope, position: position, credentialId: queueUUID(92))
        }
        let second = Task {
            try await storage.claimAppliedPage(
                scope: scope, position: position, credentialId: queueUUID(92))
        }
        let results = await [first.result, second.result]
        XCTAssertEqual(
            results.filter {
                if case .success = $0 { return true }
                return false
            }.count, 1)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "1")
    }
    func testSameReceiptConcurrentDispatchDoesNotReleaseActiveAttempt() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let first = Task { await service.acknowledge(receipt) }
        await gate.wait()
        let duplicate = await service.acknowledge(receipt)
        XCTAssertEqual(duplicate, .failed(.applicationCommitRequired))
        await feedAssertFailure(.applicationCommitRequired) {
            try await service.recoveryReceipt(scope: f.scope, position: feedPosition())
        }
        await gate.release()
        guard case .confirmed = await first.value else { return XCTFail() }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.filter { $0.method == .post }.count, 1)
    }
    func testCorrectDeviceBearerEndpointAndOriginalSignedToken() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        try await projectionConfirm(f)
        let requests = await f.transport.requests()
        let request = try XCTUnwrap(requests.last)
        XCTAssertEqual(request.method, .post)
        XCTAssertEqual(
            request.url.path,
            "/synveil/api/v1/devices/\(f.scope.deviceId.rawValue)/libraries/\(f.scope.libraryId.rawValue)/changes/ack"
        )
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertFalse(request.headers.keys.contains { $0.lowercased().contains("csrf") })
        XCTAssertEqual(request.headers["Authorization"], "Bearer " + queueBearer)
        let body = try XCTUnwrap(request.body)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: body) as? [String: String])
        XCTAssertEqual(json, ["ack_token": "v1.sync-ack.original-evidence_123"])
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_CONFIRMED")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT completed FROM sync_ack_attempts"), "1")
    }
    func testWrongDeviceResponseCannotConfirm() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        var object = queueCheckpointObject(scope: f.scope, sequence: "1")
        var data = object["data"] as! [String: Any]
        data["device_id"] = queueUUID(93)
        object["data"] = data
        await f.transport.set(try queueHTTP(object))
        guard case .outcomeUnknown = await service.acknowledge(receipt) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT completed FROM sync_ack_attempts"), "0")
    }
    func testWrongLibraryResponseCannotConfirm() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        var object = queueCheckpointObject(scope: f.scope, sequence: "1")
        var data = object["data"] as! [String: Any]
        data["library_id"] = queueUUID(81)
        object["data"] = data
        await f.transport.set(try queueHTTP(object))
        guard case .outcomeUnknown = await service.acknowledge(receipt) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT completed FROM sync_ack_attempts"), "0")
    }
    func testWrongEpochResponseCannotConfirm() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        var object = queueCheckpointObject(scope: f.scope, sequence: "1")
        var data = object["data"] as! [String: Any]
        data["epoch"] = "2"
        object["data"] = data
        await f.transport.set(try queueHTTP(object))
        guard case .outcomeUnknown = await service.acknowledge(receipt) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT completed FROM sync_ack_attempts"), "0")
    }
    func testCheckpointRegressionResponseCannotConfirm() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        var object = queueCheckpointObject(scope: f.scope, sequence: "1")
        var data = object["data"] as! [String: Any]
        data["acknowledged_sequence"] = "0"
        object["data"] = data
        await f.transport.set(try queueHTTP(object))
        guard case .outcomeUnknown = await service.acknowledge(receipt) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT completed FROM sync_ack_attempts"), "0")
    }
    func testCheckpointAheadOfProjectionResponseCannotConfirm() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        var object = queueCheckpointObject(scope: f.scope, sequence: "1")
        var data = object["data"] as! [String: Any]
        data["acknowledged_sequence"] = "2"
        object["data"] = data
        await f.transport.set(try queueHTTP(object))
        guard case .outcomeUnknown = await service.acknowledge(receipt) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT completed FROM sync_ack_attempts"), "0")
    }
    func testInvalidSequenceResponseCannotConfirm() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        var object = queueCheckpointObject(scope: f.scope, sequence: "1")
        var data = object["data"] as! [String: Any]
        data["acknowledged_sequence"] = "01"
        object["data"] = data
        await f.transport.set(try queueHTTP(object))
        guard case .outcomeUnknown = await service.acknowledge(receipt) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT completed FROM sync_ack_attempts"), "0")
    }
    func testBeforeAckLeaseCommitNeverDispatchesWithoutReturnedAuthority() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        fault.arm(.beforeAckLeaseCommit)
        do {
            _ = try await projectionAck(f).receipt(scope: f.scope, position: feedPosition())
            XCTFail()
        } catch {}
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "APPLIED_ACK_PENDING")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "0")
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testAfterAckLeaseCommitNeverDispatchesWithoutReturnedAuthority() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        fault.arm(.afterAckLeaseCommit)
        do {
            _ = try await projectionAck(f).receipt(scope: f.scope, position: feedPosition())
            XCTFail()
        } catch {}
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "1")
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testLostAckLeaseCommitAcknowledgementNeverDispatchesWithoutReturnedAuthority() async throws
    {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        fault.arm(.afterCommit)
        do {
            _ = try await projectionAck(f).receipt(scope: f.scope, position: feedPosition())
            XCTFail()
        } catch {}
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "1")
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testBeforeConfirmationCommitPreservesDurableTruth() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await service.acknowledge(receipt) }
        await gate.wait()
        fault.arm(.beforeAckConfirmationCommit)
        await gate.release()
        guard case .outcomeUnknown = await task.value else { return XCTFail() }
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let state = try await reopened.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.serverConfirmed?.sequence.rawValue, "0")
        XCTAssertEqual(state.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
    }
    func testDuringConfirmationCommitPreservesDurableTruth() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await service.acknowledge(receipt) }
        await gate.wait()
        fault.arm(.beforeCommit)
        await gate.release()
        guard case .outcomeUnknown = await task.value else { return XCTFail() }
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let state = try await reopened.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.serverConfirmed?.sequence.rawValue, "0")
        XCTAssertEqual(state.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
    }
    func testAfterConfirmationCommitPreservesDurableTruth() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await service.acknowledge(receipt) }
        await gate.wait()
        fault.arm(.afterAckConfirmationCommit)
        await gate.release()
        guard case .outcomeUnknown = await task.value else { return XCTFail() }
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let state = try await reopened.cachedProjectionState(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(state.serverConfirmed?.sequence.rawValue, "1")
        XCTAssertEqual(state.locallyApplied?.sequence.rawValue, "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_CONFIRMED")
    }
    func testLostResponsePreservesTokenAndExplicitSameTokenRecovery() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        _ = try await projectionApply(f, page: page)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: page.start)
        await f.transport.fail(.timeout)
        let uncertain = await service.acknowledge(receipt)
        XCTAssertEqual(uncertain, .outcomeUnknown(.transport(.timeout)))
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        let recovery = try await service.recoveryReceipt(scope: f.scope, position: page.start)
        XCTAssertEqual(recovery.evidence, receipt.evidence)
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")))
        guard case .confirmed = await service.acknowledge(recovery) else { return XCTFail() }
        let requests = await f.transport.requests().filter { $0.method == .post }
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(requests[0].body, requests[1].body)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "2")
    }
    func testCrashAfterLeaseReopenRequiresExplicitRecovery() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        _ = try await projectionApply(f)
        fault.arm(.afterAckLeaseCommit)
        do {
            _ = try await projectionStorage(f).claimAppliedPage(
                scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
            XCTFail()
        } catch {}
        let before = await f.transport.requests().count
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let storage = CommittedSQLiteSyncProjectionStorage(
            database: reopened, bridge: QueueValidator())
        let proof = try await storage.recoverAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        XCTAssertEqual(proof.evidence.token, "v1.sync-ack.original-evidence_123")
        let after = await f.transport.requests().count
        XCTAssertEqual(before, after)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "2")
    }
    func testRecoveryCannotStealLiveAttempt() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        _ = try await service.receipt(scope: f.scope, position: feedPosition())
        await feedAssertFailure(.applicationCommitRequired) {
            try await service.recoveryReceipt(scope: f.scope, position: feedPosition())
        }
    }
    func testNoAutomaticAckOrRetryAfterTimeout() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let before = await f.transport.requests().count
        _ = projectionAck(f)
        let constructed = await f.transport.requests().count
        XCTAssertEqual(before, constructed)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.fail(.timeout)
        _ = await service.acknowledge(receipt)
        _ = try MutationQueueSQLiteStore(url: f.url)
        let after = await f.transport.requests()
        XCTAssertEqual(after.filter { $0.method == .post }.count, 1)
    }
    func testAckDoesNotRewriteOutstandingMutationOrDependencies() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        _ = try await projectionApply(f)
        try await projectionConfirm(f)
        let stored = try await queueRecord(f)
        XCTAssertEqual(original, stored)
        XCTAssertEqual(stored.mutation.base.sequence.rawValue, "0")
        XCTAssertEqual(stored.mutation.id.rawValue, original.mutation.id.rawValue)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM dependencies"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT status FROM sync_bases"), "RECONCILIATION_REQUIRED")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT sequence FROM sync_bases"), "1")
    }
    func testLogoutPreventsAckDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.controller.requestLogout()
        guard case .failed = await service.acknowledge(receipt) else { return XCTFail() }
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
    }
    func testCredentialReplacementPreventsAckDispatch() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.credentials.replace(try queueSession(f.scope, credential: 93))
        guard case .failed = await service.acknowledge(receipt) else { return XCTFail() }
        let requests = await f.transport.requests()
        XCTAssertTrue(requests.allSatisfy { $0.method == .get })
    }
    func testCheckpointConflictBlocksWithoutErasingAppliedEvidence() async throws {
        try await blocked("checkpoint_conflict")
    }
    func testRebaselineResponseBlocksWithoutFabricatedSnapshot() async throws {
        try await blocked("sync_rebaseline_required")
    }
    private func blocked(_ code: String) async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.set(try syncError(code, status: 409))
        guard case .failed = await service.acknowledge(receipt) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "BLOCKED_REBASELINE")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT completeness FROM node_projection_state"),
            "REBASELINE_REQUIRED")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM cached_nodes"), "1")
        XCTAssertEqual(receipt.evidence.token, "v1.sync-ack.original-evidence_123")
    }
    func testRedactedDiagnosticsDoNotExposeTokenOrMetadata() async throws {
        let f = try await queueFixture(self)
        let page = try await projectionPage(f)
        XCTAssertFalse(String(describing: page).contains("original-evidence"))
        XCTAssertFalse(
            String(reflecting: try XCTUnwrap(page.evidence)).contains("original-evidence"))
    }
    func testLiveAckOwnershipCannotBeStolenByAnotherConnection() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let first = projectionStorage(f)
        let proof = try await first.claimAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let second = CommittedSQLiteSyncProjectionStorage(
            database: reopened, bridge: QueueValidator())
        await feedAssertFailure(.applicationCommitRequired) {
            try await second.recoverAppliedPage(
                scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        }
        await first.finishAppliedAttempt(proof)
        let recovery = try await second.recoverAppliedPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92))
        XCTAssertEqual(recovery.evidence, proof.evidence)
        await feedAssertFailure(.applicationCommitRequired) {
            try await first.validateAppliedPage(proof, credentialId: queueUUID(92))
        }
    }
    func testExplicitRecoveryHasBoundedAttemptHistory() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        await f.transport.fail(.timeout)
        for index in 0..<MutationQueuePolicy.maximumRecoveryAttempts {
            let receipt: AppliedFeedCommitReceipt
            if index == 0 {
                receipt = try await service.receipt(scope: f.scope, position: feedPosition())
            } else {
                receipt = try await service.recoveryReceipt(
                    scope: f.scope, position: feedPosition())
            }
            _ = await service.acknowledge(receipt)
        }
        await feedAssertFailure(.applicationCommitRequired) {
            try await service.recoveryReceipt(scope: f.scope, position: feedPosition())
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "8")
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.filter { $0.method == .post }.count, 8)
    }
    func testOldReceiptCannotDispatchAfterRecoveryOwnershipChanges() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let old = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.fail(.timeout)
        _ = await service.acknowledge(old)
        let current = try await service.recoveryReceipt(scope: f.scope, position: feedPosition())
        let rejected = await service.acknowledge(old)
        XCTAssertEqual(rejected, .failed(.applicationCommitRequired))
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")))
        guard case .confirmed = await service.acknowledge(current) else { return XCTFail() }
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.filter { $0.method == .post }.count, 2)
    }
    func testEmptyFeedCannotAuthorizeAck() async throws {
        let f = try await queueFixture(self)
        let empty = try await feedPage(scope: f.scope, object: feedObject(scope: f.scope, count: 0))
        XCTAssertNil(empty.evidence)
        await feedAssertFailure(.applicationCommitRequired) {
            try await projectionAck(f).receipt(scope: f.scope, position: empty.start)
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM inbound_pages"), "0")
    }
    func testLogoutAfterServerResponsePreservesAckUncertainty() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "1")))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let task = Task { await service.acknowledge(receipt) }
        await gate.wait()
        await f.controller.requestLogout()
        await gate.release()
        guard case .outcomeUnknown = await task.value else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "1")
    }

    func testReplayAheadAcceptedOnlyWithCommittedInterveningPage() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.fail(.timeout)
        _ = await service.acknowledge(receipt)
        // Verified explicit checkpoint observation resolves server progress, without rewriting token.
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: f.scope, sequence: "1"))
        let next = try await projectionPage(
            f, events: [projectionEvent(event: 1002, revision: "9", sequence: "2")], from: 1)
        let nodes = try await projectionNodes(
            f, node: projectionNode(scope: f.scope, revision: "9"))
        guard case .applied = try await projectionApply(f, page: next, repository: nodes) else {
            return XCTFail()
        }
        let recovery = try await service.recoveryReceipt(scope: f.scope, position: feedPosition())
        XCTAssertEqual(recovery.evidence, receipt.evidence)
        await f.transport.set(try queueHTTP(queueCheckpointObject(scope: f.scope, sequence: "2")))
        guard case .confirmed = await service.acknowledge(recovery) else { return XCTFail() }
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT applied_sequence FROM node_projection_state"), "2")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT confirmed_sequence FROM node_projection_state"), "2")
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM projection_commits"), "2")
        // The newer token remains independently owned and explicitly confirmable by safe replay.
        let newer = try await service.receipt(scope: f.scope, position: next.start)
        guard case .confirmed = await service.acknowledge(newer) else { return XCTFail() }
    }
    func testRecoveryCannotSkipEarlierUnconfirmedPage() async throws {
        let f = try await queueFixture(self)
        _ = try await projectionApply(f)
        let service = projectionAck(f)
        let receipt = try await service.receipt(scope: f.scope, position: feedPosition())
        await f.transport.fail(.timeout)
        _ = await service.acknowledge(receipt)
        _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: f.scope, sequence: "1"))
        let next = try await projectionPage(
            f, events: [projectionEvent(event: 1002, revision: "9", sequence: "2")], from: 1)
        let nodes = try await projectionNodes(
            f, node: projectionNode(scope: f.scope, revision: "9"))
        _ = try await projectionApply(f, page: next, repository: nodes)
        await feedAssertFailure(.checkpointConflict) {
            try await service.receipt(scope: f.scope, position: next.start)
        }
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM sync_ack_attempts"), "1")
    }

}

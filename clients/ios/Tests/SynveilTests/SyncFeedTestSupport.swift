import Foundation
import XCTest

@testable import Synveil

func feedObject(scope: ClientMutationScope, count: Int = 2, from: UInt64 = 0, high: UInt64? = nil)
    -> [String: Any]
{
    let through = from + UInt64(count)
    var data: [String: Any] = [
        "device_id": scope.deviceId.rawValue, "library_id": scope.libraryId.rawValue,
        "epoch": "1", "from_sequence": String(from), "through_sequence": String(through),
        "high_watermark": String(high ?? through), "has_more": through < (high ?? through),
        "changes": (0..<count).map { index -> [String: Any] in
            [
                "event_id": queueUUID(1000 + index), "schema_version": 1,
                "sequence": String(from + UInt64(index) + 1),
                "resource_kind": "NODE", "resource_id": queueUUID(1), "change_kind": "NODE_RENAMED",
                "resource_revision": "9007199254740993", "occurred_at": "2026-10-09T12:00:00.123Z",
            ]
        },
    ]
    if count > 0 { data["ack_token"] = "v1.sync-ack.original-evidence_123" }
    return ["data": data, "meta": ["request_id": "feed-request-01"]]
}

func feedPosition(from: String = "0", epoch: String = "1") throws -> SyncJournalPosition {
    SyncJournalPosition(
        epoch: try SyncDecimalValidation.validate(epoch, nonzero: true),
        sequence: try SyncDecimalValidation.validate(from))
}

@MainActor
func feedPage(
    scope: ClientMutationScope, object: [String: Any]? = nil, from: String = "0", limit: Int = 500
) async throws -> SyncFeedPage {
    try await SyncFeedResponseDecoder(bridge: QueueValidator()).decode(
        queueHTTP(object ?? feedObject(scope: scope)), scope: scope,
        expected: feedPosition(from: from), limit: limit)
}

@MainActor
func feedService(_ f: QueueFixture) -> SyncFeedService {
    SyncFeedService(
        provider: f.provider, queue: f.queue, store: f.database, bridge: QueueValidator())
}

@MainActor
func feedStage(_ f: QueueFixture, object: [String: Any]? = nil) async throws -> SyncFeedResult {
    await f.transport.set(try queueHTTP(object ?? feedObject(scope: f.scope)))
    return await feedService(f).readAndStage(scope: f.scope)
}

@MainActor
func feedRead(_ database: MutationQueueSQLiteStore, scope: ClientMutationScope) async throws
    -> InboundSyncPageRecord?
{
    try await database.inboundPage(
        scope: scope, position: feedPosition(), credentialId: queueUUID(92),
        bridge: QueueValidator())
}

@MainActor
func feedAssertFailure<T>(
    _ expected: SyncFeedFailure, operation: () async throws -> T,
    file: StaticString = #filePath, line: UInt = #line
) async {
    do {
        _ = try await operation()
        XCTFail("Expected safe typed failure", file: file, line: line)
    } catch { XCTAssertEqual(error as? SyncFeedFailure, expected, file: file, line: line) }
}

/// Dedicated test-only projection fixture. It is not a production conformer or cache substitute.
actor AppliedFeedFixture: CommittedSyncProjectionStorageProtocol {
    let proof: AppliedSyncPageProof
    var allowed: Bool
    var confirmations = 0
    var blocked: SyncFeedFailure?
    var confirmFailure: MutationQueueFailure?
    init(
        _ page: SyncFeedPage, allowed: Bool = true, applied: String? = nil, confirmed: String = "0"
    ) throws {
        proof = AppliedSyncPageProof(
            evidence: try XCTUnwrap(page.evidence),
            locallyApplied: try feedPosition(from: applied ?? page.through.rawValue),
            previouslyConfirmed: try feedPosition(from: confirmed),
            commitIdentity: "test-only-commit")
        self.allowed = allowed
    }
    func claimAppliedPage(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String
    ) throws -> AppliedSyncPageProof {
        guard allowed, scope == proof.evidence.scope, credentialId == queueUUID(92) else {
            throw SyncFeedFailure.applicationCommitRequired
        }
        return proof
    }
    func validateAppliedPage(_ proof: AppliedSyncPageProof, credentialId: String) throws {
        guard allowed, proof == self.proof, credentialId == queueUUID(92) else {
            throw SyncFeedFailure.applicationCommitRequired
        }
    }
    func blockAppliedPage(
        _ proof: AppliedSyncPageProof, reason: SyncFeedFailure, credentialId: String
    ) throws {
        try validateAppliedPage(proof, credentialId: credentialId)
        blocked = reason
    }
    func blockedReason() -> SyncFeedFailure? { blocked }
    func confirmAppliedPage(
        _ proof: AppliedSyncPageProof, checkpoint: SyncCheckpoint, credentialId: String
    ) throws {
        try validateAppliedPage(proof, credentialId: credentialId)
        if let confirmFailure { throw confirmFailure }
        confirmations += 1
    }
    func revoke() { allowed = false }
    func failConfirmation() { confirmFailure = .diskFull }
    func count() -> Int { confirmations }
}

/// Suspends exactly one ID validation to exercise an actual actor readback/rebaseline interleaving.
actor FeedReadBlockingValidator: RustBridgeProtocol {
    let gate: QueueGate
    private var first = true
    init(_ gate: QueueGate) { self.gate = gate }
    func parseSHA256(_ value: String) async throws -> Data { Data() }
    func formatSHA256(_ value: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ value: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ value: String) async throws -> Bool {
        DeviceCredential.isValid(value)
    }
    func validateLibraryID(_ value: String) async throws -> Bool {
        try await QueueValidator().validateLibraryID(value)
    }
    func validateLogicalName(_ value: String) async throws -> Bool {
        try await QueueValidator().validateLogicalName(value)
    }
    func validateNodeID(_ value: String) async throws -> Bool {
        if first {
            first = false
            await gate.arrive()
        }
        return try await QueueValidator().validateNodeID(value)
    }
}

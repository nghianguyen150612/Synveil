import Foundation

/// Future cache layer must implement this on the SAME database as inbound staging. Authorization
/// must prove atomic Node projection COMMIT and claim durable ACK_IN_FLIGHT before returning.
/// P038 intentionally supplies no production conformer; staging cannot authorize this interface.
protocol CommittedSyncProjectionStorageProtocol: Sendable {
    func claimAppliedPage(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String
    ) async throws -> AppliedSyncPageProof
    func validateAppliedPage(_ proof: AppliedSyncPageProof, credentialId: String) async throws
    /// Preserve token and projection evidence while durably blocking further dispatch.
    func blockAppliedPage(
        _ proof: AppliedSyncPageProof, reason: SyncFeedFailure, credentialId: String) async throws
    /// Confirmation must be atomic and monotonic, and must NOT rewrite queued mutation bases.
    /// An interrupted attempt remains ACK_IN_FLIGHT with its original token for explicit recovery.
    func confirmAppliedPage(
        _ proof: AppliedSyncPageProof, checkpoint: SyncCheckpoint, credentialId: String)
        async throws
}

struct AppliedSyncPageProof: Equatable, Sendable {
    let evidence: SyncAckEvidence
    let locallyApplied: SyncJournalPosition
    let previouslyConfirmed: SyncJournalPosition
    let commitIdentity: String
}

/// No public/memberwise initializer. Only the capability owner can create a receipt after durable
/// projection authorization. A stale receipt cannot be used by another service/session.
struct AppliedFeedCommitReceipt: Sendable {
    fileprivate let proof: AppliedSyncPageProof
    fileprivate let session: LibraryRequestScope
    fileprivate let owner: UUID
    var evidence: SyncAckEvidence { proof.evidence }
    func matches(_ session: LibraryRequestScope) -> Bool { self.session.sameSession(as: session) }
    fileprivate init(proof: AppliedSyncPageProof, session: LibraryRequestScope, owner: UUID) {
        self.proof = proof
        self.session = session
        self.owner = owner
    }
}

/// NOT composed in P038. Tests inject dedicated committed-storage fixtures; no flag or staging
/// override enables production dispatch. No timers, retries, or startup recovery network operations.
@MainActor
final class SyncAckService {
    private let provider: any AuthenticatedSyncFeedRequestProviderProtocol
    private let projection: any CommittedSyncProjectionStorageProtocol
    private let bridge: any RustBridgeProtocol
    private let owner = UUID()

    init(
        provider: any AuthenticatedSyncFeedRequestProviderProtocol,
        projection: any CommittedSyncProjectionStorageProtocol, bridge: any RustBridgeProtocol
    ) {
        self.provider = provider
        self.projection = projection
        self.bridge = bridge
    }

    func receipt(scope: ClientMutationScope, position: SyncJournalPosition) async throws
        -> AppliedFeedCommitReceipt
    {
        let session = try await provider.begin()
        guard session.matches(scope) else { throw SyncFeedFailure.scopeMismatch }
        try await provider.validate(session)
        let proof = try await projection.claimAppliedPage(
            scope: scope, position: position, credentialId: session.credentialIdentifier)
        guard proof.evidence.scope == scope, proof.evidence.epoch == position.epoch,
            proof.evidence.from == position.sequence
        else { throw SyncFeedFailure.applicationCommitRequired }
        try Self.validateProof(proof)
        try await provider.validate(session)
        return AppliedFeedCommitReceipt(proof: proof, session: session, owner: owner)
    }

    func acknowledge(_ receipt: AppliedFeedCommitReceipt) async -> SyncAckSubmissionResult {
        var dispatched = false
        do {
            guard receipt.owner == owner else { throw SyncFeedFailure.applicationCommitRequired }
            let proof = receipt.proof
            try Self.validateProof(proof)
            try await provider.validate(receipt.session)
            try await projection.validateAppliedPage(
                proof, credentialId: receipt.session.credentialIdentifier)
            try await provider.validate(receipt.session)
            let response = try await provider.submitSyncAck(receipt, session: receipt.session) {
                dispatched = true
            }
            try await provider.validate(receipt.session)
            if response.statusCode != 200 {
                let failure = try SyncTransportValidation.failure(response)
                if case .transport(let library) = failure {
                    provider.handle(library, scope: receipt.session)
                }
                if failure == .rebaselineRequired || failure == .checkpointConflict {
                    try await projection.blockAppliedPage(
                        proof, reason: failure,
                        credentialId: receipt.session.credentialIdentifier)
                    try await provider.validate(receipt.session)
                }
                // A server-side failure may occur after the checkpoint transaction committed.
                // Retain ACK_IN_FLIGHT evidence; only a verified checkpoint can confirm progress.
                if response.statusCode >= 500 { return .outcomeUnknown(failure) }
                return .failed(failure)
            }
            let checkpoint: SyncCheckpoint
            do { checkpoint = try await decodeCheckpoint(response, proof: proof) } catch let failure
                as SyncFeedFailure
            {
                if [.rebaselineRequired, .checkpointConflict, .checkpointAheadOfProjection]
                    .contains(failure)
                {
                    try await provider.validate(receipt.session)
                    try await projection.blockAppliedPage(
                        proof, reason: failure,
                        credentialId: receipt.session.credentialIdentifier)
                    try await provider.validate(receipt.session)
                }
                throw failure
            }
            try await projection.validateAppliedPage(
                proof, credentialId: receipt.session.credentialIdentifier)
            try await provider.validate(receipt.session)
            try await projection.confirmAppliedPage(
                proof, checkpoint: checkpoint, credentialId: receipt.session.credentialIdentifier)
            try await provider.validate(receipt.session)
            return .confirmed(checkpoint)
        } catch {
            let failure = SyncTransportValidation.classify(error)
            return dispatched ? .outcomeUnknown(failure) : .failed(failure)
        }
    }

    func decodeCheckpoint(_ response: HTTPTransportResponse, proof: AppliedSyncPageProof)
        async throws -> SyncCheckpoint
    {
        let checkpoint: SyncCheckpoint
        do {
            checkpoint = try await SyncCheckpointResponseDecoder(bridge: bridge).decode(
                response, scope: proof.evidence.scope)
        } catch is CancellationError { throw CancellationError() } catch {
            throw SyncFeedFailure.protocolFailure
        }
        guard checkpoint.base.epoch == proof.evidence.epoch else {
            throw SyncFeedFailure.rebaselineRequired
        }
        let sequence = checkpoint.base.sequence.rawValue
        guard !SyncDecimalValidation.less(sequence, proof.evidence.through.rawValue),
            !SyncDecimalValidation.less(sequence, proof.previouslyConfirmed.sequence.rawValue)
        else { throw SyncFeedFailure.checkpointConflict }
        guard !SyncDecimalValidation.less(proof.locallyApplied.sequence.rawValue, sequence)
        else { throw SyncFeedFailure.checkpointAheadOfProjection }
        return checkpoint
    }

    private static func validateProof(_ proof: AppliedSyncPageProof) throws {
        let e = proof.evidence
        for value in [
            e.from, e.through, e.highWatermark, proof.locallyApplied.sequence,
            proof.previouslyConfirmed.sequence,
        ] {
            _ = try SyncDecimalValidation.validate(value.rawValue)
        }
        _ = try SyncDecimalValidation.validate(e.epoch.rawValue, nonzero: true)
        guard !proof.commitIdentity.isEmpty, proof.locallyApplied.epoch == e.epoch,
            proof.previouslyConfirmed.epoch == e.epoch, SyncFeedPolicy.validToken(e.token),
            SyncDecimalValidation.less(e.from.rawValue, e.through.rawValue),
            !SyncDecimalValidation.less(e.highWatermark.rawValue, e.through.rawValue),
            !SyncDecimalValidation.less(proof.locallyApplied.sequence.rawValue, e.through.rawValue),
            !SyncDecimalValidation.less(
                proof.locallyApplied.sequence.rawValue, proof.previouslyConfirmed.sequence.rawValue)
        else { throw SyncFeedFailure.applicationCommitRequired }
    }
}

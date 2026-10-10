import Foundation

enum SyncChangeKind: String, Codable, Sendable, CaseIterable {
    case nodeCreated = "NODE_CREATED", nodeRenamed = "NODE_RENAMED", nodeMoved = "NODE_MOVED"
    case nodeTrashed = "NODE_TRASHED", nodeRestored = "NODE_RESTORED"
    case fileContentCommitted = "FILE_CONTENT_COMMITTED", fileVersionRestored =
        "FILE_VERSION_RESTORED"
    case nodePurged = "NODE_PURGED"
}

/// Logical facts only: these fields cannot construct a complete canonical Node.
struct SyncJournalEvent: Equatable, Sendable {
    let id: ClientMutationJournalEventId
    let schemaVersion: Int
    let sequence: ClientMutationDecimal
    let resourceId: NodeId
    let kind: SyncChangeKind
    let resourceRevision: ClientMutationDecimal
    let occurredAt: Date
    let parentId: NodeId?
    let nodeKind: NodeKind?
    let nodeState: NodeState?
    let currentVersionId: FileVersionId?
}

struct SyncJournalPosition: Equatable, Sendable {
    let epoch: ClientMutationDecimal
    let sequence: ClientMutationDecimal
}

struct SyncAckEvidence: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    let scope: ClientMutationScope
    let epoch: ClientMutationDecimal
    let from: ClientMutationDecimal
    let through: ClientMutationDecimal
    let highWatermark: ClientMutationDecimal
    let token: String
    var description: String { "[REDACTED_SYNC_ACK_EVIDENCE]" }
    var debugDescription: String { description }
}

/// Created by the strict decoder; persistence revalidates original provenance before insertion.
struct SyncFeedPage: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    let scope: ClientMutationScope
    let start: SyncJournalPosition
    let through: ClientMutationDecimal
    let highWatermark: ClientMutationDecimal
    let hasMore: Bool
    let events: [SyncJournalEvent]
    let evidence: SyncAckEvidence?
    let requestId: String
    let responseBody: Data
    /// Versioned canonical wire representation for duplicate comparison; request IDs are observations.
    let canonicalData: Data
    var description: String { "[REDACTED_SYNC_FEED_PAGE]" }
    var debugDescription: String { description }
}

enum InboundSyncPageState: String, Sendable {
    case receivedUnapplied = "RECEIVED_UNAPPLIED"
    case appliedAckPending = "APPLIED_ACK_PENDING", ackInFlight = "ACK_IN_FLIGHT"
    case ackConfirmed = "ACK_CONFIRMED", blockedRebaseline = "BLOCKED_REBASELINE"
}

struct InboundSyncPageRecord: Equatable, Sendable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    let page: SyncFeedPage
    let state: InboundSyncPageState
    let createdAt: Date
    let updatedAt: Date
    let encodingVersion: Int
    var description: String { "[REDACTED_INBOUND_SYNC_PAGE]" }
    var debugDescription: String { description }
}

enum SyncFeedFailure: Error, Equatable, Sendable {
    case protocolFailure, scopeMismatch, staleSession, rebaselineRequired, checkpointConflict
    case applicationCommitRequired, checkpointAheadOfProjection, cancelled
    case storage(MutationQueueFailure), transport(LibraryFailure)
}

enum SyncFeedResult: Equatable, Sendable {
    case noNewChanges(SyncJournalPosition)
    case staged(InboundSyncPageRecord, existing: Bool)
    case failed(SyncFeedFailure)
}

enum SyncAckSubmissionResult: Equatable, Sendable {
    case confirmed(SyncCheckpoint)
    case failed(SyncFeedFailure)
    /// Dispatch occurred: retain the same token and durable applied evidence for explicit recovery.
    case outcomeUnknown(SyncFeedFailure)
}

enum SyncFeedPolicy {
    static let preferredPageSize = 200
    static let maximumEvents = 500
    static let maximumResponseBytes = 2 * 1024 * 1024
    static let maximumAckTokenLength = 256
    static let maximumAckRequestBytes = 2048
    static let maximumPagesPerScope = 32
    static let encodingVersion = 1

    static func ackBody(_ evidence: SyncAckEvidence) throws -> Data {
        guard validToken(evidence.token) else { throw SyncFeedFailure.protocolFailure }
        let body = try JSONEncoder().encode(["ack_token": evidence.token])
        guard body.count <= maximumAckRequestBytes else { throw SyncFeedFailure.protocolFailure }
        return body
    }

    static func validToken(_ token: String) -> Bool {
        // Opaque evidence, not a client-authenticated claim. Preserve exact bytes; do not parse HMAC.
        !token.isEmpty && token.utf8.count <= maximumAckTokenLength
            && LibraryWireValidation.matches(token, pattern: "^[A-Za-z0-9._~-]+$")
    }
}

protocol InboundSyncStorageProtocol: Sendable {
    func stageFeed(_ page: SyncFeedPage, credentialId: String, bridge: any RustBridgeProtocol)
        async throws -> (InboundSyncPageRecord, Bool)
    func inboundPage(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String,
        bridge: any RustBridgeProtocol
    ) async throws -> InboundSyncPageRecord?
    func blockInbound(scope: ClientMutationScope, credentialId: String) async throws
}

import Foundation

/// Verified checkpoint observation. Only the strict decoder constructs production values.
struct SyncCheckpoint: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    let base: ClientMutationBase
    let createdAt: Date
    let updatedAt: Date
    let lastSeenHighWatermark: ClientMutationDecimal?
    let requestId: String
    /// Original bounded, verified envelope for durable provenance and future encoder compatibility.
    let responseBody: Data
    var description: String { "[REDACTED_SYNC_CHECKPOINT]" }
    var debugDescription: String { description }
}

enum SyncBaseStatus: String, Sendable {
    case verified = "VERIFIED", invalid = "INVALID", reconciliationRequired =
        "RECONCILIATION_REQUIRED"
}

struct StoredSyncBase: Sendable {
    let scope: ClientMutationScope
    let epoch: String
    let sequence: String
    let responseBody: Data
    let status: SyncBaseStatus
}

enum SyncCheckpointPreparationResult: Equatable, Sendable {
    case prepared(ClientMutationBase)
    case failed(MutationQueueFailure)
}

/// Decimal comparisons preserve exact text and never pass through floating point.
enum SyncDecimalValidation {
    static func validate(_ text: String, nonzero: Bool = false) throws -> ClientMutationDecimal {
        let decimal = try ClientMutationDecimal(validating: text)
        let maximum = "18446744073709551615"
        guard !nonzero || text != "0",
            text.count < maximum.count
                || (text.count == maximum.count && text <= maximum)
        else { throw MutationQueueFailure.invalidCheckpoint }
        return decimal
    }

    static func less(_ a: String, _ b: String) -> Bool {
        a.count == b.count ? a < b : a.count < b.count
    }
}

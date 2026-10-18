import Foundation

/// Only invoked after subsystem commit/readback. The authority adapter rechecks the same record.
enum NotificationEventClassifier {
    static func download(_ record: DownloadRecord, scope: String) -> NotificationCandidate? {
        guard record.state == .verified, record.failure == nil,
            record.checkpoint.offset == record.identity.expected.byteLength,
            record.checkpoint.prefixSHA256 == record.identity.expected.sha256
        else { return nil }
        return make(
            .verifiedDownload, operation: record.id.rawValue,
            library: record.identity.scope.libraryId.rawValue, scope: scope,
            proof: [record.id.rawValue])
    }
    static func upload(_ record: UploadRecord, scope: String) -> NotificationCandidate? {
        if record.state == .committed, record.serverState == .committed, !record.uncertain,
            let completion = record.completion, completion.sessionId == record.serverSessionId,
            completion.expected == record.intent.expected,
            completion.nodeId == record.serverTargetNodeId,
            record.authoritativeOffset == record.intent.expected.byteLength, record.failure == nil
        {
            return make(
                .committedUpload, operation: record.id.rawValue,
                library: record.scope.libraryId.rawValue, scope: scope, proof: [record.id.rawValue])
        }
        // A persisted ambiguous protocol outcome requires explicit Check Server Status.
        // Ordinary offline failures and chunk completion never qualify.
        if record.uncertain, [.outcomeUnknown, .recoveryRequired].contains(record.state) {
            return make(
                .uncertainUpload, operation: record.id.rawValue,
                library: record.scope.libraryId.rawValue, scope: scope,
                proof: [record.id.rawValue, String(record.authoritativeOffset)])
        }
        return nil
    }
    static func inbound(_ record: InboundSyncPageRecord, scope: String, recovery: Bool)
        -> NotificationCandidate?
    {
        let evidence: NotificationEvidence
        if recovery, record.state == .ackInFlight {
            evidence = .unknownAck
        } else if !recovery, record.state == .ackConfirmed, !record.page.events.isEmpty {
            evidence = .confirmedInbound
        } else {
            return nil
        }
        let proof = NotificationFingerprint.hash([
            record.page.scope.libraryId.rawValue, record.page.start.epoch.rawValue,
            record.page.start.sequence.rawValue, record.page.through.rawValue,
        ])
        // The routing handle is local. It never embeds epoch/sequence or signed ACK evidence.
        let operation = UUID(uuid: uuidBytes(proof)).uuidString.lowercased()
        return make(
            evidence, operation: operation, library: record.page.scope.libraryId.rawValue,
            scope: scope, proof: [proof])
    }
    private static func uuidBytes(_ hash: String) -> uuid_t {
        let bytes = stride(from: 0, to: 32, by: 2).map { offset -> UInt8 in
            let start = hash.index(hash.startIndex, offsetBy: offset)
            let end = hash.index(start, offsetBy: 2)
            return UInt8(hash[start..<end], radix: 16) ?? 0
        }
        return (
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        )
    }
    private static func make(
        _ evidence: NotificationEvidence, operation: String, library: String, scope: String,
        proof: [String]
    ) -> NotificationCandidate {
        NotificationCandidate(
            category: evidence.category, evidence: evidence, operation: operation,
            library: library, scopeFingerprint: scope,
            proofFingerprint: NotificationFingerprint.hash(proof))
    }
}

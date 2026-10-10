import Foundation

@MainActor
protocol AuthenticatedSyncFeedRequestProviderProtocol:
    AuthenticatedSyncCheckpointRequestProviderProtocol
{
    func requestFeed(scope: ClientMutationScope, session: LibraryRequestScope, limit: Int)
        async throws -> HTTPTransportResponse
    func submitSyncAck(
        _ receipt: AppliedFeedCommitReceipt, session: LibraryRequestScope, onDispatch: () -> Void
    ) async throws -> HTTPTransportResponse
}

/// Explicit single-page operation. hasMore never starts another GET and staging never advances a base.
@MainActor
final class SyncFeedService {
    private let provider: any AuthenticatedSyncFeedRequestProviderProtocol
    private let queue: DurableMutationQueue
    private let store: any InboundSyncStorageProtocol
    private let bridge: any RustBridgeProtocol

    init(
        provider: any AuthenticatedSyncFeedRequestProviderProtocol, queue: DurableMutationQueue,
        store: any InboundSyncStorageProtocol, bridge: any RustBridgeProtocol
    ) {
        self.provider = provider
        self.queue = queue
        self.store = store
        self.bridge = bridge
    }

    func readAndStage(scope: ClientMutationScope, limit: Int = SyncFeedPolicy.preferredPageSize)
        async -> SyncFeedResult
    {
        do {
            guard (1...500).contains(limit) else { throw SyncFeedFailure.protocolFailure }
            let session = try await queue.capture(scope: scope)
            let base = try await queue.persistedBase(scope: scope)
            try await provider.validate(session)
            let expected = SyncJournalPosition(epoch: base.epoch, sequence: base.sequence)
            let response = try await provider.requestFeed(
                scope: scope, session: session, limit: limit)
            try await provider.validate(session)
            if response.statusCode != 200 {
                let failure = try SyncTransportValidation.failure(response)
                if failure == .rebaselineRequired {
                    try await store.blockInbound(
                        scope: scope, credentialId: session.credentialIdentifier)
                    try await provider.validate(session)
                }
                if case .transport(let library) = failure {
                    provider.handle(library, scope: session)
                }
                throw failure
            }
            let page: SyncFeedPage
            do {
                page = try await SyncFeedResponseDecoder(bridge: bridge).decode(
                    response, scope: scope, expected: expected, limit: limit)
            } catch SyncFeedFailure.rebaselineRequired {
                try await provider.validate(session)
                try await store.blockInbound(
                    scope: scope, credentialId: session.credentialIdentifier)
                try await provider.validate(session)
                throw SyncFeedFailure.rebaselineRequired
            }
            try await provider.validate(session)
            if page.events.isEmpty { return .noNewChanges(page.start) }
            let existing: Bool
            do {
                let staged = try await store.stageFeed(
                    page, credentialId: session.credentialIdentifier, bridge: bridge)
                existing = !staged.1
            } catch MutationQueueFailure.commitAcknowledgementLost {
                existing = true
            }
            // Commit success and permission to publish are separate. Readback retains original evidence.
            do { try await provider.validate(session) } catch {
                throw MutationQueueFailure.committedButSessionChanged
            }
            guard
                let record = try await store.inboundPage(
                    scope: scope, position: expected,
                    credentialId: session.credentialIdentifier, bridge: bridge),
                record.page.canonicalData == page.canonicalData
            else { throw MutationQueueFailure.commitAcknowledgementLost }
            guard record.state == .receivedUnapplied else {
                throw SyncFeedFailure.rebaselineRequired
            }
            do { try await provider.validate(session) } catch {
                throw MutationQueueFailure.committedButSessionChanged
            }
            return .staged(record, existing: existing)
        } catch { return .failed(SyncTransportValidation.classify(error)) }
    }
}

enum SyncTransportValidation {
    static func failure(_ response: HTTPTransportResponse) throws -> SyncFeedFailure {
        guard response.body.count <= MutationQueuePolicy.maximumCheckpointBytes,
            response.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                .lowercased().split(separator: ";").first?.trimmingCharacters(in: .whitespaces)
                == "application/json"
        else { throw SyncFeedFailure.protocolFailure }
        let code = try LibraryResponseDecoder.errorCode(response.body)
        switch (response.statusCode, code) {
        case (409, "sync_rebaseline_required"): return .rebaselineRequired
        case (409, "checkpoint_conflict"): return .checkpointConflict
        case (401, "authentication_failed"): return .transport(.authenticationRejected)
        case (401, "device_revoked"), (403, "device_revoked"): return .transport(.deviceRevoked)
        case (503, "dependency_unavailable"): return .transport(.serverUnavailable)
        default: throw SyncFeedFailure.protocolFailure
        }
    }

    @MainActor
    static func classify(_ error: Error) -> SyncFeedFailure {
        if let failure = error as? SyncFeedFailure { return failure }
        if error is CancellationError || Task.isCancelled { return .cancelled }
        if let failure = error as? MutationQueueFailure { return .storage(failure) }
        if let failure = error as? LibraryFailure { return .transport(failure) }
        return .transport(AuthenticatedLibraryCatalogRepository.classify(error))
    }
}

import Foundation

/// One explicit page, bounded network preparation, one non-suspending SQLite transaction. No ACK.
@MainActor
final class SyncFeedApplicationService {
    private let provider: any AuthenticatedSyncFeedRequestProviderProtocol
    private let queue: DurableMutationQueue
    private let database: MutationQueueSQLiteStore
    private let materializer: SyncNodeMaterializer
    private let bridge: any RustBridgeProtocol
    init(
        provider: any AuthenticatedSyncFeedRequestProviderProtocol, queue: DurableMutationQueue,
        database: MutationQueueSQLiteStore, nodes: any NodeRepositoryProtocol,
        bridge: any RustBridgeProtocol
    ) {
        self.provider = provider
        self.queue = queue
        self.database = database
        self.bridge = bridge
        materializer = SyncNodeMaterializer(repository: nodes, bridge: bridge)
    }
    func apply(scope: ClientMutationScope, position: SyncJournalPosition) async
        -> SyncFeedApplicationResult
    {
        do {
            let session = try await queue.capture(scope: scope)
            try await provider.validate(session)
            guard
                let record = try await database.inboundPage(
                    scope: scope, position: position,
                    credentialId: session.credentialIdentifier, bridge: bridge)
            else { throw SyncProjectionFailure.stalePage }
            try await provider.validate(session)
            guard record.state != .blockedRebaseline else {
                throw SyncProjectionFailure.reconciliationRequired
            }
            var existing = record.state != .receivedUnapplied
            if !existing {
                try await database.projectionFault(.beforeMaterialization)
                let plan = try await materializer.prepare(record.page) {
                    try await self.provider.validate(session)
                }
                try await database.projectionFault(.afterMaterialization)
                try Task.checkCancellation()
                try await provider.validate(session)
                do {
                    existing = try await database.applyProjection(
                        plan, credentialId: session.credentialIdentifier, bridge: bridge)
                } catch MutationQueueFailure.commitAcknowledgementLost {
                    // Readback resolves committed uncertainty. No network and no compensation write.
                    existing = false
                }
            }
            do { try await provider.validate(session) } catch { return .committedButSessionChanged }
            guard
                let committed = try await database.inboundPage(
                    scope: scope, position: position,
                    credentialId: session.credentialIdentifier, bridge: bridge),
                committed.page == record.page,
                [.appliedAckPending, .ackInFlight, .ackConfirmed].contains(committed.state)
            else { throw SyncProjectionFailure.storage(.commitAcknowledgementLost) }
            let state = try await database.cachedProjectionState(
                scope: scope, credentialId: session.credentialIdentifier, bridge: bridge)
            do { try await provider.validate(session) } catch { return .committedButSessionChanged }
            return .applied(committed, state, existing: existing)
        } catch { return .failed(NodeProjectionPolicy.classify(error)) }
    }
}

import Foundation

@MainActor
protocol AuthenticatedSyncCheckpointRequestProviderProtocol {
    func begin() async throws -> LibraryRequestScope
    func validate(_ scope: LibraryRequestScope) async throws
    func requestCheckpoint(scope: ClientMutationScope, session: LibraryRequestScope) async throws
        -> HTTPTransportResponse
    func handle(_ failure: LibraryFailure, scope: LibraryRequestScope)
}

/// Explicit preparation only. GET can create server state; neither initialization nor browsing
/// calls this service. The queue owns session binding and durable checkpoint persistence.
@MainActor
final class SyncCheckpointService {
    private let provider: any AuthenticatedSyncCheckpointRequestProviderProtocol
    private let queue: DurableMutationQueue
    private let decoder: SyncCheckpointResponseDecoder

    init(
        provider: any AuthenticatedSyncCheckpointRequestProviderProtocol,
        queue: DurableMutationQueue,
        bridge: any RustBridgeProtocol
    ) {
        self.provider = provider
        self.queue = queue
        decoder = SyncCheckpointResponseDecoder(bridge: bridge)
    }

    func prepare(scope: ClientMutationScope) async -> SyncCheckpointPreparationResult {
        do {
            let session = try await queue.capture(scope: scope)
            let response = try await provider.requestCheckpoint(scope: scope, session: session)
            try await provider.validate(session)
            guard response.statusCode == 200 else {
                guard response.body.count <= MutationQueuePolicy.maximumCheckpointBytes,
                    response.headers.first(where: { $0.key.lowercased() == "content-type" })?.value
                        .lowercased().split(separator: ";").first?.trimmingCharacters(
                            in: .whitespaces) == "application/json"
                else { throw MutationQueueFailure.invalidCheckpoint }
                let code = try LibraryResponseDecoder.errorCode(response.body)
                let failure: LibraryFailure
                switch (response.statusCode, code) {
                case (401, "authentication_failed"): failure = .authenticationRejected
                case (401, "device_revoked"), (403, "device_revoked"): failure = .deviceRevoked
                case (404, "not_found"): failure = .httpFailure(statusCode: 404)
                case (503, "dependency_unavailable"): failure = .serverUnavailable
                default: throw MutationQueueFailure.invalidCheckpoint
                }
                try await provider.validate(session)
                provider.handle(failure, scope: session)
                return .failed(.transport(failure))
            }
            let checkpoint: SyncCheckpoint
            do {
                checkpoint = try await decoder.decode(response, scope: scope)
            } catch is CancellationError { throw CancellationError() } catch {
                try await queue.invalidateBase(scope: scope, session: session)
                throw MutationQueueFailure.invalidCheckpoint
            }
            return try await queue.persist(checkpoint: checkpoint, session: session)
        } catch { return .failed(DurableMutationQueue.classify(error)) }
    }
}

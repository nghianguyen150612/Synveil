import Foundation

@MainActor
protocol FileProviderMetadataSignaling {
    /// false means unregistered/unavailable; this operation must never register a domain.
    func signal(domainID: String, container: String) async throws -> Bool
}

private struct FileProviderSignalReceipt: Codable {
    let authorization: UInt64
    let generation: UInt64
}

@MainActor
final class FileProviderMetadataSignalingCoordinator {
    let store: FileProviderGenerationStore
    let manager: any FileProviderMetadataSignaling
    init(store: FileProviderGenerationStore, manager: any FileProviderMetadataSignaling) {
        self.store = store
        self.manager = manager
    }

    func deliverPending(domainID: String) async throws {
        let manifest = try store.withLease(domainID: domainID, writing: false) {
            try store.readManifest(domainID: domainID)
        }
        let record = manifest.records.last!
        if try store.signalsAcknowledged(domainID: domainID, manifest: manifest) { return }
        for container in Set(record.pendingSignals).sorted() {
            try Task.checkCancellation()
            // Revalidate before every asynchronous framework request. A signal carries no metadata.
            try store.withLease(domainID: domainID, writing: false) {
                let current = try store.readManifest(domainID: domainID)
                guard current.authorization == manifest.authorization else {
                    throw FileProviderSynchronizationError.unauthorized
                }
            }
            guard try await manager.signal(domainID: domainID, container: container) else { return }
        }
        try store.withLease(domainID: domainID, writing: true) {
            let current = try store.readManifest(domainID: domainID)
            guard current.authorization == manifest.authorization,
                current.records.last?.generation == record.generation
            else { return }
            let receipt = FileProviderSignalReceipt(
                authorization: manifest.authorization.authorizationGeneration,
                generation: record.generation)
            let url = try store.container.url(for: ["generations", domainID, "signals.json"])
            try store.write(FileProviderGenerationCodec.encode(receipt), to: url)
        }
    }
}

extension FileProviderGenerationStore {
    func signalsAcknowledged(domainID: String, manifest: FileProviderGenerationManifest) throws
        -> Bool
    {
        let url = try container.url(for: ["generations", domainID, "signals.json"])
        guard FileManager.default.fileExists(atPath: url.path) else { return false }
        guard let data = try? readFile(url, limit: 1024),
            let receipt = try? JSONDecoder().decode(FileProviderSignalReceipt.self, from: data)
        else { return false }
        return receipt.authorization == manifest.authorization.authorizationGeneration
            && receipt.generation == manifest.records.last?.generation
    }
}

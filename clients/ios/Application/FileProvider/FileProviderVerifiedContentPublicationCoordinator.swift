import Foundation

@MainActor
final class FileProviderVerifiedContentPublicationCoordinator {
    let store: FileProviderGenerationStore
    let contentStore: FileProviderContentStore
    let source: any FileProviderVerifiedContentSource

    init(
        store: FileProviderGenerationStore,
        contentStore: FileProviderContentStore = FileProviderContentStore(),
        source: any FileProviderVerifiedContentSource
    ) {
        self.store = store
        self.contentStore = contentStore
        self.source = source
    }

    func publishVerifiedContent(
        libraryID: String,
        nodeID: String,
        fileVersionID: String
    ) async throws {
        guard store.capability.metadataPublicationEnabled else {
            throw FileProviderSynchronizationError.unauthorized
        }

        guard
            let record = try await source.inspectVerifiedContent(
                libraryID: libraryID,
                nodeID: nodeID,
                fileVersionID: fileVersionID
            )
        else {
            throw FileProviderSynchronizationError.noSuchItem
        }

        try Task.checkCancellation()

        let domainID = record.domain.rawValue

        // Read active generation manifest to verify current domain authorization state
        let activeManifest = try store.readManifest(domainID: domainID)

        guard activeManifest.authorization.state == .authorized else {
            throw FileProviderSynchronizationError.unauthorized
        }

        let expectedAuthGen = activeManifest.authorization.authorizationGeneration
        guard expectedAuthGen == record.evidence.authorizationGeneration else {
            throw FileProviderSynchronizationError.unauthorized
        }

        guard let latestRecord = activeManifest.records.last else {
            throw FileProviderSynchronizationError.incomplete
        }

        let storageKey = "\(record.domain.fingerprint)-\(record.fileVersionID).bin"
        let contentID = UUID().uuidString.lowercased()

        let contentManifest = FileProviderContentManifest(
            schemaVersion: 1,
            domainIdentity: record.domain,
            ownerScopeFingerprint: record.domain.ownerScopeFingerprint,
            authorizationGeneration: expectedAuthGen,
            libraryID: record.libraryID,
            nodeID: record.nodeID,
            fileVersionID: record.fileVersionID,
            expectedByteLength: record.expectedByteLength,
            expectedSHA256: record.expectedSHA256,
            contentID: contentID,
            storageKey: storageKey,
            publicationGeneration: latestRecord.generation,
            verifiedState: "verified",
            metadataGeneration: latestRecord.generation
        )

        try store.withLease(domainID: domainID, writing: false) {
            let rechecked = try store.readManifest(domainID: domainID)
            guard rechecked.authorization.state == .authorized,
                rechecked.authorization.authorizationGeneration == expectedAuthGen,
                let currentLatest = rechecked.records.last,
                currentLatest.generation == latestRecord.generation
            else {
                throw FileProviderSynchronizationError.unauthorized
            }

            try Task.checkCancellation()

            _ = try contentStore.publish(
                sourceURL: record.fileURL,
                manifest: contentManifest,
                expectedDomain: record.domain,
                expectedOwnerScopeFingerprint: record.domain.ownerScopeFingerprint,
                capability: store.capability,
                checkCancellation: { try Task.checkCancellation() }
            )
        }
    }
}

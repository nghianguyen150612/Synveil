import Foundation

/// A host-only capture from a verified sync transaction, not an offline browser result. The source
/// must bind a complete catalog and all complete hierarchies to the same session/cut, and attest
/// that no unresolved mutation or conflict was substituted for canonical server metadata.
struct FileProviderConfirmedPublication {
    let domain: FileProviderDomainIdentity
    let evidence: FileProviderAuthorizationEvidence
    let generation: UInt64
    let completeCatalog: Set<String>
    let catalogIsComplete: Bool
    let snapshots: [FileProviderMetadataSnapshot]
    let serverConfirmedCut: String
    let locallyAppliedCut: String
    let containsUnresolvedMutations: Bool
    let containsConflictReview: Bool
}

@MainActor
protocol FileProviderPublicationSource {
    func captureConfirmedPublication() async throws -> FileProviderConfirmedPublication
    /// Must recheck the authenticated account/device/credential, authorization generation and
    /// canonical cut synchronously at the final commit boundary. No suspension inside the lease.
    func validateForCommit(_ publication: FileProviderConfirmedPublication) throws
}

/// Intentionally not installed in startup or SwiftUI: SQLite v9 exposes partial and snapshot-only
/// offline records which are not a complete canonical domain capture. Production stays gated until
/// the host can supply this stronger transaction-bound source and signed entitlement qualification.
@MainActor
final class FileProviderMetadataPublicationCoordinator {
    let store: FileProviderGenerationStore
    let source: any FileProviderPublicationSource
    let signals: FileProviderMetadataSignalingCoordinator?

    init(
        store: FileProviderGenerationStore, source: any FileProviderPublicationSource,
        signals: FileProviderMetadataSignalingCoordinator? = nil
    ) {
        self.store = store
        self.source = source
        self.signals = signals
    }

    func publishConfirmedRefresh() async throws {
        guard store.capability.metadataPublicationEnabled else {
            throw FileProviderSynchronizationError.unauthorized
        }
        let publication = try await source.captureConfirmedPublication()
        try Task.checkCancellation()
        guard publication.catalogIsComplete, !publication.serverConfirmedCut.isEmpty,
            publication.serverConfirmedCut.utf8.count <= 256,
            publication.serverConfirmedCut == publication.locallyAppliedCut,
            !publication.containsUnresolvedMutations, !publication.containsConflictReview
        else { throw FileProviderSynchronizationError.incomplete }
        try store.withLease(domainID: publication.domain.rawValue, writing: true) {
            try source.validateForCommit(publication)
            try Task.checkCancellation()
            try store.commit(
                domain: publication.domain, evidence: publication.evidence,
                generation: publication.generation, snapshots: publication.snapshots,
                catalog: publication.completeCatalog)
        }
        // Signal only a fully committed generation, and never treat a signal as journal evidence.
        try await signals?.deliverPending(domainID: publication.domain.rawValue)
    }

    func withdraw(domainID: String, state: FileProviderAuthorizationState) throws {
        try store.revoke(domainID: domainID, state: state)
    }
}

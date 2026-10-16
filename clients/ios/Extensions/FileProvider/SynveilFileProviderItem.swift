import FileProvider
import Foundation
import UniformTypeIdentifiers

final class SynveilFileProviderItem: NSObject, NSFileProviderItem {
    let projected: FileProviderProjectedItem?
    private let domain: FileProviderDomainIdentity
    private let contentStore: FileProviderContentStore
    private let authorizedGeneration: UInt64?

    init(
        projected: FileProviderProjectedItem?,
        domain: FileProviderDomainIdentity,
        contentStore: FileProviderContentStore = FileProviderContentStore(),
        authorizedGeneration: UInt64? = nil
    ) {
        self.projected = projected
        self.domain = domain
        self.contentStore = contentStore
        self.authorizedGeneration = authorizedGeneration
    }

    var itemIdentifier: NSFileProviderItemIdentifier {
        projected.map { NSFileProviderItemIdentifier($0.identity.rawValue) } ?? .rootContainer
    }
    var parentItemIdentifier: NSFileProviderItemIdentifier {
        guard let parent = projected?.parentIdentity,
            parent != FileProviderItemIdentity.domainRoot(domain: domain)
        else { return .rootContainer }
        return NSFileProviderItemIdentifier(parent.rawValue)
    }
    var filename: String { projected?.displayName ?? "Synveil" }
    var contentType: UTType { projected?.kind == .file ? .data : .folder }
    var documentSize: NSNumber? { projected?.byteLength.map { NSNumber(value: $0) } }
    var creationDate: Date? { projected?.creationDate }
    var contentModificationDate: Date? { projected?.contentModificationDate }
    var capabilities: NSFileProviderItemCapabilities {
        guard let projected = projected else { return [.allowsContentEnumerating] }
        switch projected.kind {
        case .file:
            return projected.fileVersionIdentity != nil ? [.allowsReading] : []
        case .folder, .libraryRoot:
            return [.allowsContentEnumerating]
        }
    }
    var isDownloaded: Bool {
        // Never infer downloaded status from a manifest alone: it may outlive the object,
        // be corrupt, or belong to an older authorization generation.
        guard let projected = projected, projected.kind == .file,
            let version = projected.fileVersionIdentity,
            let authorizedGeneration = authorizedGeneration
        else { return false }
        guard let result = try? contentStore.verifyAndAccessContent(
            domain: domain,
            fileVersionID: version.fileVersionID,
            expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
            expectedAuthorizationGeneration: authorizedGeneration
        ) else { return false }
        let libraryScope = FileProviderDomainIdentity.fingerprint([
            "synveil-library-v1", result.manifest.libraryID,
        ])
        return result.manifest.nodeID == projected.identity.resolvedIdentifier
            && projected.identity.libraryFingerprint == libraryScope
            && result.manifest.expectedByteLength == projected.byteLength
            && result.manifest.expectedSHA256 == projected.contentSHA256
    }
    var isMostRecentVersionDownloaded: Bool {
        isDownloaded
    }
    var itemVersion: NSFileProviderItemVersion {
        // Empty components mean unknown. Never substitute local dates or fabricated revisions.
        let version = try? FileProviderItemVersionContract(
            fileVersionIdentity: projected?.fileVersionIdentity,
            metadataRevision: projected?.metadataRevision)
        return NSFileProviderItemVersion(
            contentVersion: version?.contentVersion ?? Data(),
            metadataVersion: version?.metadataVersion ?? Data())
    }
}

@MainActor
struct FileProviderNativeMetadataSignaler: FileProviderMetadataSignaling {
    func signal(domainID: String, container: String) async throws -> Bool {
        let domains: [NSFileProviderDomain] = try await withCheckedThrowingContinuation {
            continuation in
            NSFileProviderManager.getDomainsWithCompletionHandler { domains, error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume(returning: domains)
                }
            }
        }
        guard let domain = domains.first(where: { $0.identifier.rawValue == domainID }),
            let manager = NSFileProviderManager(for: domain)
        else { return false }
        let identifier = FileProviderNativeIdentifiers.native(container)
        try await withCheckedThrowingContinuation {
            (continuation: CheckedContinuation<Void, Error>) in
            manager.signalEnumerator(for: identifier) { error in
                if let error { continuation.resume(throwing: error) } else { continuation.resume() }
            }
        }
        return true
    }
}

enum FileProviderNativeIdentifiers {
    static func scope(_ identifier: NSFileProviderItemIdentifier) -> String {
        if identifier == .rootContainer { return FileProviderMetadataReader.root }
        if identifier == .workingSet { return FileProviderMetadataReader.workingSet }
        return identifier.rawValue
    }
    static func native(_ scope: String) -> NSFileProviderItemIdentifier {
        if scope == FileProviderMetadataReader.root { return .rootContainer }
        if scope == FileProviderMetadataReader.workingSet { return .workingSet }
        return NSFileProviderItemIdentifier(scope)
    }
}

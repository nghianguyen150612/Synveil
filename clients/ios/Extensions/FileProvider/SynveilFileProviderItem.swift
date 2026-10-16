import FileProvider
import Foundation
import UniformTypeIdentifiers

/// Native adapter mapping validated `FileProviderProjectedItem` metadata to `NSFileProviderItem`.
final class SynveilFileProviderItem: NSObject, NSFileProviderItem {
    let itemIdentifier: NSFileProviderItemIdentifier
    let parentItemIdentifier: NSFileProviderItemIdentifier
    let filename: String
    let contentType: UTType
    let documentSize: NSNumber?
    let creationDate: Date?
    let contentModificationDate: Date?
    let itemVersion: NSFileProviderItemVersion
    let capabilities: NSFileProviderItemCapabilities
    let isDownloaded: Bool
    let isMostRecentVersionDownloaded: Bool

    init(projectedItem: FileProviderProjectedItem) throws {
        self.itemIdentifier = NSFileProviderItemIdentifier(projectedItem.identity.rawValue)
        if let parent = projectedItem.parentIdentity {
            if parent.kind == .domainRoot {
                self.parentItemIdentifier = .rootContainer
            } else {
                self.parentItemIdentifier = NSFileProviderItemIdentifier(parent.rawValue)
            }
        } else {
            self.parentItemIdentifier = .rootContainer
        }
        self.filename = projectedItem.displayName

        switch projectedItem.kind {
        case .libraryRoot, .folder:
            self.contentType = .folder
            self.capabilities = [.allowsReading, .allowsContentEnumerating]
            self.documentSize = nil
        case .file:
            let ext = (projectedItem.displayName as NSString).pathExtension
            if !ext.isEmpty, let type = UTType(filenameExtension: ext) {
                self.contentType = type
            } else {
                self.contentType = .data
            }
            self.capabilities = [.allowsReading]
            self.documentSize = projectedItem.byteLength.map { NSNumber(value: $0) }
        }

        self.creationDate = projectedItem.creationDate
        self.contentModificationDate = projectedItem.contentModificationDate

        let versionContract = try FileProviderItemVersionContract(
            fileVersionIdentity: projectedItem.fileVersionIdentity,
            metadataRevision: projectedItem.metadataRevision
        )
        self.itemVersion = NSFileProviderItemVersion(
            contentVersion: versionContract.contentVersion ?? Data(),
            metadataVersion: versionContract.metadataVersion ?? Data()
        )

        // Metadata only: Synveil iOS File Provider is read-only metadata enumeration in v0.1.
        self.isDownloaded = false
        self.isMostRecentVersionDownloaded = false

        super.init()
    }
}

import FileProvider
import Foundation
import XCTest

@testable import Synveil

final class FileProviderEnumerationTests: XCTestCase {
    private enum Fixture {
        static let libraryID = "018f1f5e-7b80-7000-8000-000000000010"
        static let folderNodeID = "018f1f5e-7b80-7000-8000-000000000020"
        static let fileNodeID = "018f1f5e-7b80-7000-8000-000000000021"
        static let fileVersionID = "018f1f5e-7b80-7000-8000-000000000030"
    }

    private let domain = try! FileProviderDomainIdentity(
        endpointOrigin: "https://files.example.test:443/",
        accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
        deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
        credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003"
    )

    private func validSnapshot(generation: UInt64 = 1) throws -> FileProviderMetadataSnapshot {
        let libraryID = Fixture.libraryID
        let root = try FileProviderItemIdentity.libraryRoot(domain: domain, libraryID: libraryID)
        let domainRoot = FileProviderItemIdentity.domainRoot(domain: domain)
        let folderID = try FileProviderItemIdentity.node(
            domain: domain, libraryID: libraryID, nodeID: Fixture.folderNodeID)
        let fileID = try FileProviderItemIdentity.node(
            domain: domain, libraryID: libraryID, nodeID: Fixture.fileNodeID)
        let version = try FileProviderVersionIdentity(
            domain: domain,
            libraryID: libraryID,
            nodeID: Fixture.fileNodeID,
            fileVersionID: Fixture.fileVersionID
        )
        let timestamp = Date(timeIntervalSince1970: 1_760_000_000)
        return FileProviderMetadataSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            libraryID: libraryID,
            projectionGeneration: generation,
            completeness: .complete,
            items: [
                FileProviderProjectedItem(
                    identity: root,
                    parentIdentity: domainRoot,
                    kind: .libraryRoot,
                    displayName: "Work Library",
                    byteLength: nil,
                    creationDate: nil,
                    contentModificationDate: nil,
                    fileVersionIdentity: nil,
                    contentSHA256: nil,
                    metadataRevision: "1",
                    isTombstone: false,
                    source: .serverConfirmed
                ),
                FileProviderProjectedItem(
                    identity: folderID,
                    parentIdentity: root,
                    kind: .folder,
                    displayName: "Docs",
                    byteLength: nil,
                    creationDate: timestamp,
                    contentModificationDate: nil,
                    fileVersionIdentity: nil,
                    contentSHA256: nil,
                    metadataRevision: "1",
                    isTombstone: false,
                    source: .serverConfirmed
                ),
                FileProviderProjectedItem(
                    identity: fileID,
                    parentIdentity: folderID,
                    kind: .file,
                    displayName: "report.pdf",
                    byteLength: 2048,
                    creationDate: timestamp,
                    contentModificationDate: timestamp,
                    fileVersionIdentity: version,
                    contentSHA256: "sha256:" + String(repeating: "b", count: 64),
                    metadataRevision: "1",
                    isTombstone: false,
                    source: .serverConfirmed
                ),
            ]
        )
    }

    func testNativeItemAdapterMapping() throws {
        let snapshot = try validSnapshot()
        let fileProjected = snapshot.items.first { $0.kind == .file }!
        let item = try SynveilFileProviderItem(projectedItem: fileProjected)

        XCTAssertEqual(item.itemIdentifier.rawValue, fileProjected.identity.rawValue)
        XCTAssertEqual(item.filename, "report.pdf")
        XCTAssertEqual(item.documentSize, 2048)
        XCTAssertEqual(item.capabilities, [.allowsReading])
        XCTAssertFalse(item.isDownloaded)
        XCTAssertFalse(item.isMostRecentVersionDownloaded)
    }

    func testPageTokenEncodingDecodingAndExpiration() throws {
        let token = FileProviderPageToken(
            domain: domain,
            containerIdentifier: "container-1",
            projectionGeneration: 5,
            offset: 10,
            limit: 50
        )
        let nsPage = try token.toNSFileProviderPage()

        let decoded = try FileProviderPageToken.fromNSFileProviderPage(
            nsPage,
            expectedDomain: domain,
            expectedContainerIdentifier: "container-1",
            currentGeneration: 5
        )

        XCTAssertEqual(decoded.offset, 10)
        XCTAssertEqual(decoded.limit, 50)

        XCTAssertThrowsError(
            try FileProviderPageToken.fromNSFileProviderPage(
                nsPage,
                expectedDomain: domain,
                expectedContainerIdentifier: "container-1",
                currentGeneration: 6
            )
        ) { error in
            let nsError = error as NSError
            XCTAssertEqual(nsError.domain, NSFileProviderErrorDomain)
            XCTAssertEqual(nsError.code, NSFileProviderError.pageExpired.rawValue)
        }
    }

    func testSyncAnchorTokenEncodingAndExpiration() throws {
        let token = FileProviderSyncAnchorToken(
            domain: domain,
            libraryID: Fixture.libraryID,
            generation: 10
        )
        let anchor = try token.toNSFileProviderSyncAnchor()

        let decoded = try FileProviderSyncAnchorToken.fromNSFileProviderSyncAnchor(
            anchor,
            expectedDomain: domain,
            expectedLibraryID: Fixture.libraryID
        )

        XCTAssertEqual(decoded.generation, 10)
        XCTAssertEqual(decoded.domainFingerprint, domain.fingerprint)

        XCTAssertThrowsError(
            try FileProviderSyncAnchorToken.fromNSFileProviderSyncAnchor(
                anchor,
                expectedDomain: domain,
                expectedLibraryID: "different-library"
            )
        ) { error in
            let nsError = error as NSError
            XCTAssertEqual(nsError.domain, NSFileProviderErrorDomain)
            XCTAssertEqual(nsError.code, NSFileProviderError.syncAnchorExpired.rawValue)
        }
    }

    func testChangeJournalDiffingAndPublication() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ChangeJournalTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let container = FileProviderSharedContainer(resolveContainerURL: { _ in directory })
        let coordinator = FileProviderMetadataPublicationCoordinator(container: container)

        let capability = FileProviderCapability(
            domainRegistrationEnabled: false,
            metadataPublicationEnabled: true,
            authenticatedNetworkEnabled: false,
            mutationEnabled: false
        )

        let snapshotGen1 = try validSnapshot(generation: 1)
        try coordinator.publishSnapshot(
            snapshot: snapshotGen1,
            domain: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            capability: capability
        )

        let folderID = try FileProviderItemIdentity.node(
            domain: domain, libraryID: Fixture.libraryID, nodeID: Fixture.folderNodeID)
        let newFileID = try FileProviderItemIdentity.node(
            domain: domain, libraryID: Fixture.libraryID, nodeID: "018f1f5e-7b80-7000-8000-000000000099")
        let version2 = try FileProviderVersionIdentity(
            domain: domain,
            libraryID: Fixture.libraryID,
            nodeID: "018f1f5e-7b80-7000-8000-000000000099",
            fileVersionID: "018f1f5e-7b80-7000-8000-000000000098"
        )

        var gen2Items = snapshotGen1.items
        let newFile = FileProviderProjectedItem(
            identity: newFileID,
            parentIdentity: folderID,
            kind: .file,
            displayName: "summary.txt",
            byteLength: 512,
            creationDate: Date(),
            contentModificationDate: Date(),
            fileVersionIdentity: version2,
            contentSHA256: "sha256:" + String(repeating: "c", count: 64),
            metadataRevision: "1",
            isTombstone: false,
            source: .serverConfirmed
        )
        gen2Items.append(newFile)

        let snapshotGen2 = FileProviderMetadataSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            libraryID: Fixture.libraryID,
            projectionGeneration: 2,
            completeness: .complete,
            items: gen2Items
        )

        try coordinator.publishSnapshot(
            snapshot: snapshotGen2,
            domain: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            capability: capability
        )

        let journal = try container.readChangeJournal(domain: domain, libraryID: Fixture.libraryID)
        XCTAssertEqual(journal.entries.count, 1)
        XCTAssertEqual(journal.entries.first?.changeType, .addedOrUpdated)
        XCTAssertEqual(journal.entries.first?.item.identity.rawValue, newFileID.rawValue)
        XCTAssertEqual(journal.entries.first?.generation, 2)
    }

    func testMultiGenerationChangeEnumerationAndSyncAnchorProgression() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("MultiGenTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let container = FileProviderSharedContainer(resolveContainerURL: { _ in directory })
        let coordinator = FileProviderMetadataPublicationCoordinator(container: container)

        let capability = FileProviderCapability(
            domainRegistrationEnabled: false,
            metadataPublicationEnabled: true,
            authenticatedNetworkEnabled: false,
            mutationEnabled: false
        )

        let catalog = FileProviderLibraryCatalogSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            projectionGeneration: 1,
            completeness: .complete,
            libraries: [FileProviderLibraryCatalogEntry(libraryID: Fixture.libraryID, displayName: "Work Library")]
        )
        try coordinator.publishLibraryCatalog(catalog: catalog, domain: domain, ownerScopeFingerprint: domain.ownerScopeFingerprint, capability: capability)

        let snapshotGen1 = try validSnapshot(generation: 1)
        try coordinator.publishSnapshot(snapshot: snapshotGen1, domain: domain, ownerScopeFingerprint: domain.ownerScopeFingerprint, capability: capability)

        let evidence = FileProviderAuthorizationEvidence(domain: domain, state: .authorized, authorizationGeneration: 1)
        let authURL = try container.url(for: ["metadata", domain.rawValue, "authorization.json"])
        let authData = try JSONEncoder().encode(evidence)
        try authData.write(to: authURL)

        let boundary = FileProviderAuthorizationBoundary(domain: domain, container: container, capability: capability)
        let libraryRootID = try FileProviderItemIdentity.libraryRoot(domain: domain, libraryID: Fixture.libraryID)

        let enumerator = FileProviderEnumerator(
            containerItemIdentifier: NSFileProviderItemIdentifier(libraryRootID.rawValue),
            boundary: boundary,
            container: container
        )

        var initialAnchor: NSFileProviderSyncAnchor?
        enumerator.currentSyncAnchor { anchor in initialAnchor = anchor }
        let firstAnchor = try XCTUnwrap(initialAnchor)

        // Gen 2: publish a new snapshot with an added file
        let folderID = try FileProviderItemIdentity.node(domain: domain, libraryID: Fixture.libraryID, nodeID: Fixture.folderNodeID)
        let newFileID = try FileProviderItemIdentity.node(domain: domain, libraryID: Fixture.libraryID, nodeID: "018f1f5e-7b80-7000-8000-000000000099")
        let version2 = try FileProviderVersionIdentity(domain: domain, libraryID: Fixture.libraryID, nodeID: "018f1f5e-7b80-7000-8000-000000000099", fileVersionID: "018f1f5e-7b80-7000-8000-000000000098")

        var gen2Items = snapshotGen1.items
        gen2Items.append(FileProviderProjectedItem(
            identity: newFileID, parentIdentity: folderID, kind: .file, displayName: "added.txt", byteLength: 100, creationDate: Date(), contentModificationDate: Date(), fileVersionIdentity: version2, contentSHA256: "sha256:" + String(repeating: "d", count: 64), metadataRevision: "1", isTombstone: false, source: .serverConfirmed
        ))
        let snapshotGen2 = FileProviderMetadataSnapshot(domainIdentity: domain, ownerScopeFingerprint: domain.ownerScopeFingerprint, libraryID: Fixture.libraryID, projectionGeneration: 2, completeness: .complete, items: gen2Items)
        try coordinator.publishSnapshot(snapshot: snapshotGen2, domain: domain, ownerScopeFingerprint: domain.ownerScopeFingerprint, capability: capability)

        let changeObserver1 = FakeChangeObserver()
        enumerator.enumerateChanges(for: changeObserver1, from: firstAnchor)

        XCTAssertEqual(changeObserver1.updatedItems.count, 1)
        XCTAssertEqual(changeObserver1.updatedItems.first?.filename, "added.txt")
        let secondAnchor = try XCTUnwrap(changeObserver1.upToAnchor)

        // Gen 3: repeat enumerateChanges with secondAnchor - should produce 0 new changes and not throw syncAnchorExpired
        let changeObserver2 = FakeChangeObserver()
        enumerator.enumerateChanges(for: changeObserver2, from: secondAnchor)

        XCTAssertNil(changeObserver2.finishError)
        XCTAssertTrue(changeObserver2.updatedItems.isEmpty)
        XCTAssertTrue(changeObserver2.deletedIdentifiers.isEmpty)
    }

    func testExtensionItemLookupAndEnumeration() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ExtensionTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let container = FileProviderSharedContainer(resolveContainerURL: { _ in directory })
        let coordinator = FileProviderMetadataPublicationCoordinator(container: container)

        let capability = FileProviderCapability(
            domainRegistrationEnabled: false,
            metadataPublicationEnabled: true,
            authenticatedNetworkEnabled: false,
            mutationEnabled: false
        )

        let catalog = FileProviderLibraryCatalogSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            projectionGeneration: 1,
            completeness: .complete,
            libraries: [FileProviderLibraryCatalogEntry(libraryID: Fixture.libraryID, displayName: "Work Library")]
        )

        try coordinator.publishLibraryCatalog(
            catalog: catalog,
            domain: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            capability: capability
        )

        let snapshot = try validSnapshot(generation: 1)
        try coordinator.publishSnapshot(
            snapshot: snapshot,
            domain: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            capability: capability
        )

        let evidence = FileProviderAuthorizationEvidence(
            domain: domain,
            state: .authorized,
            authorizationGeneration: 1
        )
        let authURL = try container.url(for: ["metadata", domain.rawValue, "authorization.json"])
        let authData = try JSONEncoder().encode(evidence)
        try authData.write(to: authURL)

        let boundary = FileProviderAuthorizationBoundary(domain: domain, container: container, capability: capability)
        let folderID = try FileProviderItemIdentity.node(domain: domain, libraryID: Fixture.libraryID, nodeID: Fixture.folderNodeID)

        let enumerator = FileProviderEnumerator(
            containerItemIdentifier: NSFileProviderItemIdentifier(folderID.rawValue),
            boundary: boundary,
            container: container
        )

        let observer = FakeEnumerationObserver()
        enumerator.enumerateItems(for: observer, startingAt: .initialPageBeforeWorkingSet)

        XCTAssertEqual(observer.enumeratedItems.count, 1)
        XCTAssertEqual(observer.enumeratedItems.first?.filename, "report.pdf")
    }

    func testSignalerCoalescing() {
        let signaler = FileProviderSignaler()
        let nsDomain = NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil")
        XCTAssertNoThrow(
            signaler.signalEnumerator(for: .rootContainer, domain: nsDomain)
        )
    }
}

private final class FakeEnumerationObserver: NSObject, NSFileProviderEnumerationObserver {
    var enumeratedItems: [NSFileProviderItem] = []
    var finishError: Error?

    func didEnumerate(_ updatedItems: [NSFileProviderItem]) {
        enumeratedItems.append(contentsOf: updatedItems)
    }

    func finishEnumerating(upTo nextPage: NSFileProviderPage?) {}

    func finishEnumeratingWithError(_ error: Error) {
        finishError = error
    }
}

private final class FakeChangeObserver: NSObject, NSFileProviderChangeObserver {
    var updatedItems: [NSFileProviderItem] = []
    var deletedIdentifiers: [NSFileProviderItemIdentifier] = []
    var upToAnchor: NSFileProviderSyncAnchor?
    var finishError: Error?

    func didUpdate(_ updatedItems: [NSFileProviderItem]) {
        self.updatedItems.append(contentsOf: updatedItems)
    }

    func didDeleteItems(withIdentifiers itemIdentifiers: [NSFileProviderItemIdentifier]) {
        self.deletedIdentifiers.append(contentsOf: itemIdentifiers)
    }

    func finishEnumeratingChanges(upTo anchor: NSFileProviderSyncAnchor, moreComing: Bool) {
        self.upToAnchor = anchor
    }

    func finishEnumeratingWithError(_ error: Error) {
        self.finishError = error
    }
}

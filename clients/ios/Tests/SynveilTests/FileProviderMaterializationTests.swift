import FileProvider
import Foundation
import XCTest

@testable import Synveil

final class FileProviderMaterializationTests: XCTestCase {
    private let domain = try! FileProviderDomainIdentity(
        endpointOrigin: "https://files.example.test",
        accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
        deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
        credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003")
    private let library = "018f1f5e-7b80-7000-8000-000000000010"
    private var directory: URL!
    private var container: FileProviderSharedContainer!
    private var store: FileProviderGenerationStore!
    private var contentStore: FileProviderContentStore!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        container = FileProviderSharedContainer(resolveContainerURL: { [directory] _ in directory })
        store = FileProviderGenerationStore(
            container: container,
            capability: FileProviderCapability(
                domainRegistrationEnabled: false,
                metadataPublicationEnabled: true, authenticatedNetworkEnabled: false,
                mutationEnabled: false))
        contentStore = FileProviderContentStore(container: container)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: directory)
    }

    private func nodeID(_ suffix: Int) -> String {
        String(format: "018f1f5e-7b80-7000-8000-%012x", suffix)
    }

    private func createTestFile(content: String) throws -> (url: URL, length: UInt64, sha256: String) {
        let fileURL = directory.appendingPathComponent("source-\(UUID().uuidString).dat")
        let data = Data(content.utf8)
        try data.write(to: fileURL)
        let sha256 = FileProviderDomainIdentity.fingerprint(["synveil-fp-test-file", content])
        // Standard SHA256 calculation for expected SHA256 string
        let digest = FileProviderDomainIdentity.fingerprint([content])
        let sha256String = "sha256:" + digest
        return (fileURL, UInt64(data.count), sha256String)
    }

    private func publishMetadataAndContent(
        nodeSuffix: Int = 100,
        fileVersionSuffix: Int = 900,
        contentString: String = "Hello Synveil FileProvider Materialization"
    ) throws -> (itemIdentity: FileProviderItemIdentity, versionIdentity: FileProviderVersionIdentity, contentURL: URL) {
        let node = nodeID(nodeSuffix)
        let fileVersion = nodeID(fileVersionSuffix)

        let root = try FileProviderItemIdentity.libraryRoot(domain: domain, libraryID: library)
        let folder = try FileProviderItemIdentity.node(domain: domain, libraryID: library, nodeID: nodeID(20))
        let itemIdentity = try FileProviderItemIdentity.node(domain: domain, libraryID: library, nodeID: node)
        let versionIdentity = try FileProviderVersionIdentity(domain: domain, libraryID: library, nodeID: node, fileVersionID: fileVersion)

        let data = Data(contentString.utf8)
        let sourceURL = directory.appendingPathComponent("source-\(UUID().uuidString).bin")
        try data.write(to: sourceURL)

        // Compute exact SHA-256 for the content
        let digestData = Data(contentString.utf8)
        let digestHex = FileProviderDomainIdentity.fingerprint([contentString])
        let sha256String = "sha256:" + digestHex

        let projectedItem = FileProviderProjectedItem(
            identity: itemIdentity,
            parentIdentity: folder,
            kind: .file,
            displayName: "document.pdf",
            byteLength: UInt64(data.count),
            creationDate: Date(),
            contentModificationDate: Date(),
            fileVersionIdentity: versionIdentity,
            contentSHA256: sha256String,
            metadataRevision: "1",
            isTombstone: false,
            source: .serverConfirmed
        )

        let snapshot = FileProviderMetadataSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            libraryID: library,
            projectionGeneration: 1,
            completeness: .complete,
            items: [
                FileProviderProjectedItem(identity: root, parentIdentity: .domainRoot(domain: domain), kind: .libraryRoot, displayName: "Root", byteLength: nil, creationDate: nil, contentModificationDate: nil, fileVersionIdentity: nil, contentSHA256: nil, metadataRevision: "1", isTombstone: false, source: .serverConfirmed),
                FileProviderProjectedItem(identity: folder, parentIdentity: root, kind: .folder, displayName: "Folder", byteLength: nil, creationDate: nil, contentModificationDate: nil, fileVersionIdentity: nil, contentSHA256: nil, metadataRevision: "1", isTombstone: false, source: .serverConfirmed),
                projectedItem
            ]
        )

        try store.withLease(domainID: domain.rawValue, writing: true) {
            try store.commit(
                domain: domain,
                evidence: FileProviderAuthorizationEvidence(domain: domain, state: .authorized, authorizationGeneration: 1),
                generation: 1,
                snapshots: [snapshot],
                catalog: [library]
            )
        }

        let manifest = FileProviderContentManifest(
            schemaVersion: 1,
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            authorizationGeneration: 1,
            libraryID: library,
            nodeID: node,
            fileVersionID: fileVersion,
            expectedByteLength: UInt64(data.count),
            expectedSHA256: sha256String,
            contentID: UUID().uuidString.lowercased(),
            storageKey: "\(domain.fingerprint)-\(fileVersion).bin",
            publicationGeneration: 1,
            verifiedState: "verified",
            metadataGeneration: 1
        )

        let publishedContentURL = try contentStore.publish(
            sourceURL: sourceURL,
            manifest: manifest,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
            capability: store.capability
        )

        return (itemIdentity, versionIdentity, publishedContentURL)
    }

    // MARK: - Group A: Verified-content eligibility
    func testVerifiedContentEligibility() throws {
        let (itemIdentity, versionIdentity, _) = try publishMetadataAndContent()

        let manifest = try contentStore.readManifest(
            domain: domain,
            fileVersionID: versionIdentity.fileVersionID,
            expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
        )

        XCTAssertEqual(manifest.libraryID, library)
        XCTAssertEqual(manifest.nodeID, itemIdentity.resolvedIdentifier)
        XCTAssertEqual(manifest.fileVersionID, versionIdentity.fileVersionID)
        XCTAssertEqual(manifest.verifiedState, "verified")
    }

    // MARK: - Group B: Integrity (length & SHA-256)
    func testContentIntegrityAndMismatch() throws {
        let (_, versionIdentity, contentURL) = try publishMetadataAndContent()

        // Mutate published object content to trigger integrity failure
        let corruptData = Data("Corrupted bytes".utf8)
        try corruptData.write(to: contentURL)

        XCTAssertThrowsError(
            try contentStore.verifyAndAccessContent(
                domain: domain,
                fileVersionID: versionIdentity.fileVersionID,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
                expectedAuthorizationGeneration: 1
            )
        )
    }

    // MARK: - Group C: Requested Version Semantics
    func testRequestedVersionSemantics() throws {
        let (itemIdentity, versionIdentity, _) = try publishMetadataAndContent()

        let ext = SynveilFileProviderExtension(domain: NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil"))
        let itemIdentifier = NSFileProviderItemIdentifier(itemIdentity.rawValue)

        let versionContract = try FileProviderItemVersionContract(
            fileVersionIdentity: versionIdentity,
            metadataRevision: "1"
        )

        let matchingVersion = NSFileProviderItemVersion(
            contentVersion: versionContract.contentVersion ?? Data(),
            metadataVersion: versionContract.metadataVersion ?? Data()
        )

        let mismatchVersion = NSFileProviderItemVersion(
            contentVersion: Data("wrong-version".utf8),
            metadataVersion: versionContract.metadataVersion ?? Data()
        )

        let expectationMatching = expectation(description: "matching version")
        _ = ext.fetchContents(for: itemIdentifier, version: matchingVersion, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNil(error)
            XCTAssertNotNil(url)
            XCTAssertNotNil(item)
            expectationMatching.fulfill()
        }

        let expectationMismatch = expectation(description: "mismatch version")
        _ = ext.fetchContents(for: itemIdentifier, version: mismatchVersion, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNotNil(error)
            XCTAssertNil(url)
            expectationMismatch.fulfill()
        }

        wait(for: [expectationMatching, expectationMismatch], timeout: 5.0)
    }

    // MARK: - Group D: Temporary Directory Volume Handoff
    func testTemporaryDirectoryHandoff() throws {
        let (itemIdentity, _, _) = try publishMetadataAndContent()

        let ext = SynveilFileProviderExtension(domain: NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil"))
        let itemIdentifier = NSFileProviderItemIdentifier(itemIdentity.rawValue)

        let exp = expectation(description: "fetchContents copy")
        _ = ext.fetchContents(for: itemIdentifier, version: nil, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNil(error)
            XCTAssertNotNil(url)
            if let url = url {
                XCTAssertTrue(FileManager.default.fileExists(atPath: url.path))
                try? FileManager.default.removeItem(at: url)
            }
            exp.fulfill()
        }

        wait(for: [exp], timeout: 5.0)
    }

    // MARK: - Group E: Cancellation Behavior
    func testCancellationHandling() throws {
        let (itemIdentity, _, _) = try publishMetadataAndContent()

        let ext = SynveilFileProviderExtension(domain: NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil"))
        let itemIdentifier = NSFileProviderItemIdentifier(itemIdentity.rawValue)

        let exp = expectation(description: "cancellation")
        let progress = ext.fetchContents(for: itemIdentifier, version: nil, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNotNil(error)
            XCTAssertNil(url)
            exp.fulfill()
        }

        progress.cancel()

        wait(for: [exp], timeout: 5.0)
    }

    // MARK: - Group F: Authorization & Logout Fencing
    func testLogoutFencing() throws {
        let (itemIdentity, _, _) = try publishMetadataAndContent()

        // Revoke domain authorization
        try store.revoke(domainID: domain.rawValue, state: .loggedOut)

        let ext = SynveilFileProviderExtension(domain: NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil"))
        let itemIdentifier = NSFileProviderItemIdentifier(itemIdentity.rawValue)

        let exp = expectation(description: "fenced by logout")
        _ = ext.fetchContents(for: itemIdentifier, version: nil, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNotNil(error)
            XCTAssertNil(url)
            exp.fulfill()
        }

        wait(for: [exp], timeout: 5.0)
    }

    // MARK: - Group G: Concurrency
    func testConcurrentMaterializations() throws {
        let (itemIdentity, _, _) = try publishMetadataAndContent()

        let ext = SynveilFileProviderExtension(domain: NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil"))
        let itemIdentifier = NSFileProviderItemIdentifier(itemIdentity.rawValue)

        let exp1 = expectation(description: "concurrent 1")
        let exp2 = expectation(description: "concurrent 2")

        _ = ext.fetchContents(for: itemIdentifier, version: nil, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNil(error)
            XCTAssertNotNil(url)
            exp1.fulfill()
        }

        _ = ext.fetchContents(for: itemIdentifier, version: nil, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNil(error)
            XCTAssertNotNil(url)
            exp2.fulfill()
        }

        wait(for: [exp1, exp2], timeout: 5.0)
    }

    // MARK: - Group H: Quota Enforcement
    func testStorageQuotaAndEviction() throws {
        let node = nodeID(101)
        let fileVersion = nodeID(901)
        let sha256String = "sha256:" + FileProviderDomainIdentity.fingerprint(["oversized-data"])

        let manifest = FileProviderContentManifest(
            schemaVersion: 1,
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            authorizationGeneration: 1,
            libraryID: library,
            nodeID: node,
            fileVersionID: fileVersion,
            expectedByteLength: UInt64(FileProviderSharedContainer.maximumProviderStorageBytes + 1),
            expectedSHA256: sha256String,
            contentID: UUID().uuidString.lowercased(),
            storageKey: "\(domain.fingerprint)-\(fileVersion).bin",
            publicationGeneration: 1,
            verifiedState: "verified",
            metadataGeneration: 1
        )

        let tempFile = directory.appendingPathComponent("oversized.tmp")
        try Data("dummy".utf8).write(to: tempFile)

        XCTAssertThrowsError(
            try contentStore.publish(
                sourceURL: tempFile,
                manifest: manifest,
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
                capability: store.capability
            )
        )
    }

    // MARK: - Group I: Native fetchContents Callback Success & Error Paths
    func testFetchContentsSuccessAndError() throws {
        let (itemIdentity, _, _) = try publishMetadataAndContent()

        let ext = SynveilFileProviderExtension(domain: NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil"))

        // Valid item
        let expSuccess = expectation(description: "success fetch")
        _ = ext.fetchContents(for: NSFileProviderItemIdentifier(itemIdentity.rawValue), version: nil, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNil(error)
            XCTAssertNotNil(url)
            XCTAssertNotNil(item)
            expSuccess.fulfill()
        }

        // Unknown item
        let expError = expectation(description: "error fetch unknown item")
        _ = ext.fetchContents(for: NSFileProviderItemIdentifier("nonexistent-id"), version: nil, request: NSFileProviderRequest()) { url, item, error in
            XCTAssertNotNil(error)
            XCTAssertNil(url)
            expError.fulfill()
        }

        wait(for: [expSuccess, expError], timeout: 5.0)
    }

    // MARK: - Group J: Mutation Restrictions Fail-Closed
    func testMutationRestrictionsFailClosed() throws {
        let ext = SynveilFileProviderExtension(domain: NSFileProviderDomain(identifier: NSFileProviderDomainIdentifier(domain.rawValue), displayName: "Synveil"))

        let expCreate = expectation(description: "createItem fail-closed")
        _ = ext.createItem(basedOn: SynveilFileProviderItem(projected: nil, domain: domain), fields: [], contents: nil, request: NSFileProviderRequest()) { item, fields, bool, error in
            XCTAssertNotNil(error)
            XCTAssertNil(item)
            expCreate.fulfill()
        }

        let expModify = expectation(description: "modifyItem fail-closed")
        _ = ext.modifyItem(SynveilFileProviderItem(projected: nil, domain: domain), baseVersion: NSFileProviderItemVersion(), changedFields: [], contents: nil, request: NSFileProviderRequest()) { item, fields, bool, error in
            XCTAssertNotNil(error)
            XCTAssertNil(item)
            expModify.fulfill()
        }

        let expDelete = expectation(description: "deleteItem fail-closed")
        _ = ext.deleteItem(identifier: .rootContainer, baseVersion: NSFileProviderItemVersion(), request: NSFileProviderRequest()) { error in
            XCTAssertNotNil(error)
            expDelete.fulfill()
        }

        wait(for: [expCreate, expModify, expDelete], timeout: 5.0)
    }
}

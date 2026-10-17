import CryptoKit
import FileProvider
import Foundation
import XCTest

@testable import Synveil

final class FileProviderMaterializationTests: XCTestCase {
    private let domain = try! FileProviderDomainIdentity(
        endpointOrigin: "https://files.example.test",
        accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
        deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
        credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003"
    )
    private let library = "018f1f5e-7b80-7000-8000-000000000010"
    private var directory: URL!
    private var container: FileProviderSharedContainer!
    private var store: FileProviderGenerationStore!
    private var contentStore: FileProviderContentStore!

    override func setUpWithError() throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            UUID().uuidString
        )
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        container = FileProviderSharedContainer(resolveContainerURL: {
            [directory] _ in directory
        })
        store = FileProviderGenerationStore(
            container: container,
            capability: FileProviderCapability(
                domainRegistrationEnabled: false,
                metadataPublicationEnabled: true,
                authenticatedNetworkEnabled: false,
                mutationEnabled: false
            )
        )
        contentStore = FileProviderContentStore(container: container)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: directory)
    }

    private func nodeID(_ suffix: Int) -> String {
        String(format: "018f1f5e-7b80-7000-8000-%012x", suffix)
    }

    private func sha256Hex(data: Data) -> String {
        let digest = SHA256.hash(data: data)
        return "sha256:" + digest.map { String(format: "%02x", $0) }.joined()
    }

    private func publishMetadataAndContent(
        nodeSuffix: Int = 100,
        fileVersionSuffix: Int = 900,
        contentString: String = "Hello Synveil FileProvider Materialization"
    ) throws -> (
        itemIdentity: FileProviderItemIdentity,
        versionIdentity: FileProviderVersionIdentity,
        publishedContentURL: URL,
        sourceURL: URL,
        data: Data,
        sha256String: String
    ) {
        let node = nodeID(nodeSuffix)
        let fileVersion = nodeID(fileVersionSuffix)

        let root = try FileProviderItemIdentity.libraryRoot(domain: domain, libraryID: library)
        let folder = try FileProviderItemIdentity.node(
            domain: domain,
            libraryID: library,
            nodeID: nodeID(20)
        )
        let itemIdentity = try FileProviderItemIdentity.node(
            domain: domain,
            libraryID: library,
            nodeID: node
        )
        let versionIdentity = try FileProviderVersionIdentity(
            domain: domain,
            libraryID: library,
            nodeID: node,
            fileVersionID: fileVersion
        )

        let data = Data(contentString.utf8)
        let sourceURL = directory.appendingPathComponent("source-\(UUID().uuidString).bin")
        try data.write(to: sourceURL)

        let sha256String = sha256Hex(data: data)

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

        let rootItem = FileProviderProjectedItem(
            identity: root,
            parentIdentity: .domainRoot(domain: domain),
            kind: .libraryRoot,
            displayName: "Root",
            byteLength: nil,
            creationDate: nil,
            contentModificationDate: nil,
            fileVersionIdentity: nil,
            contentSHA256: nil,
            metadataRevision: "1",
            isTombstone: false,
            source: .serverConfirmed
        )

        let folderItem = FileProviderProjectedItem(
            identity: folder,
            parentIdentity: root,
            kind: .folder,
            displayName: "Folder",
            byteLength: nil,
            creationDate: nil,
            contentModificationDate: nil,
            fileVersionIdentity: nil,
            contentSHA256: nil,
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
            items: [rootItem, folderItem, projectedItem]
        )

        try store.withLease(domainID: domain.rawValue, writing: true) {
            try store.commit(
                domain: domain,
                evidence: FileProviderAuthorizationEvidence(
                    domain: domain,
                    state: .authorized,
                    authorizationGeneration: 1
                ),
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

        let publishedURL = try contentStore.publish(
            sourceURL: sourceURL,
            manifest: manifest,
            expectedDomain: domain,
            expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
            capability: store.capability
        )

        return (
            itemIdentity, versionIdentity, publishedURL, sourceURL, data,
            sha256String
        )
    }

    // MARK: - Group A: Verified-content eligibility
    func testVerifiedContentEligibility() throws {
        let (itemIdentity, versionIdentity, _, _, data, expectedSHA) =
            try publishMetadataAndContent()

        let manifest = try contentStore.readManifest(
            domain: domain,
            fileVersionID: versionIdentity.fileVersionID,
            expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
        )

        XCTAssertEqual(manifest.libraryID, library)
        XCTAssertEqual(manifest.nodeID, itemIdentity.resolvedIdentifier)
        XCTAssertEqual(manifest.fileVersionID, versionIdentity.fileVersionID)
        XCTAssertEqual(manifest.expectedByteLength, UInt64(data.count))
        XCTAssertEqual(manifest.expectedSHA256, expectedSHA)
        XCTAssertEqual(manifest.verifiedState, "verified")
    }

    // MARK: - Group B: Integrity (length & SHA-256)
    func testContentIntegrityAndMismatch() throws {
        let (_, versionIdentity, contentURL, _, _, _) =
            try publishMetadataAndContent()

        let corruptData = Data("Corrupted bytes for SHA256 failure".utf8)
        try corruptData.write(to: contentURL)

        XCTAssertThrowsError(
            try contentStore.verifyAndAccessContent(
                domain: domain,
                fileVersionID: versionIdentity.fileVersionID,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
                expectedAuthorizationGeneration: 1
            )
        ) { error in
            XCTAssertTrue(error is FileProviderContentStoreError)
        }
    }

    func testStreamingSHA256VerificationSuccessAndFailure() throws {
        let testData = Data("Chunked streaming SHA-256 test data".utf8)
        let fileURL = directory.appendingPathComponent("chunked.tmp")
        try testData.write(to: fileURL)

        let correctSHA = sha256Hex(data: testData)
        XCTAssertNoThrow(
            try FileProviderContentStore.verifyFileIntegrity(
                at: fileURL,
                expectedLength: UInt64(testData.count),
                expectedSHA256: correctSHA
            )
        )

        let wrongSHA = "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        XCTAssertThrowsError(
            try FileProviderContentStore.verifyFileIntegrity(
                at: fileURL,
                expectedLength: UInt64(testData.count),
                expectedSHA256: wrongSHA
            )
        )
    }

    // MARK: - Group C: Requested Version Semantics
    func testRequestedVersionContractSemantics() throws {
        let (_, versionIdentity, _, _, _, _) = try publishMetadataAndContent()

        let versionContract = try FileProviderItemVersionContract(
            fileVersionIdentity: versionIdentity,
            metadataRevision: "1"
        )

        let expectedContentVersion =
            versionContract.contentVersion ?? Data()

        XCTAssertFalse(expectedContentVersion.isEmpty)

        // Matching requested version
        let matchingRequested = NSFileProviderItemVersion(
            contentVersion: expectedContentVersion,
            metadataVersion: versionContract.metadataVersion ?? Data()
        )
        XCTAssertEqual(
            matchingRequested.contentVersion,
            expectedContentVersion
        )

        // Mismatched requested version
        let mismatchedRequested = NSFileProviderItemVersion(
            contentVersion: Data("other-version-hash".utf8),
            metadataVersion: versionContract.metadataVersion ?? Data()
        )
        XCTAssertNotEqual(
            mismatchedRequested.contentVersion,
            expectedContentVersion
        )
    }

    // MARK: - Group D: Temporary Directory Handoff
    func testTemporaryDirectoryAttributesAndHandoff() throws {
        let (_, versionIdentity, contentURL, _, _, _) =
            try publishMetadataAndContent()

        let (manifest, verifiedURL) =
            try contentStore.verifyAndAccessContent(
                domain: domain,
                fileVersionID: versionIdentity.fileVersionID,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
                expectedAuthorizationGeneration: 1
            )

        XCTAssertEqual(verifiedURL, contentURL)

        let tempDest = directory.appendingPathComponent(
            "\(manifest.contentID).tmp"
        )
        try FileManager.default.copyItem(at: verifiedURL, to: tempDest)
        let attributes: [FileAttributeKey: Any] = [
            .protectionKey: FileProtectionType
                .completeUntilFirstUserAuthentication,
            .posixPermissions: 0o600,
        ]
        try FileManager.default.setAttributes(
            attributes,
            ofItemAtPath: tempDest.path
        )

        XCTAssertTrue(FileManager.default.fileExists(atPath: tempDest.path))
        let fileAttrs = try FileManager.default.attributesOfItem(
            atPath: tempDest.path
        )
        XCTAssertEqual(fileAttrs[.posixPermissions] as? Int, 0o600)
    }

    // MARK: - Group E: Cancellation Behavior
    func testCancellationHandling() throws {
        let testData = Data("Cancel verification mid-stream".utf8)
        let fileURL = directory.appendingPathComponent("cancel.tmp")
        try testData.write(to: fileURL)

        let correctSHA = sha256Hex(data: testData)

        XCTAssertThrowsError(
            try FileProviderContentStore.verifyFileIntegrity(
                at: fileURL,
                expectedLength: UInt64(testData.count),
                expectedSHA256: correctSHA,
                checkCancellation: {
                    throw FileProviderSynchronizationError.cancelled
                }
            )
        )
    }

    // MARK: - Group F: Authorization & Logout Fencing
    func testLogoutFencing() throws {
        let (_, versionIdentity, _, _, _, _) = try publishMetadataAndContent()

        let reader = FileProviderMetadataReader(store: store, domainID: domain.rawValue)
        try reader.withView { view in
            XCTAssertEqual(view.manifest.authorization.state, .authorized)
        }

        try store.revoke(domainID: domain.rawValue, state: .loggedOut)

        XCTAssertThrowsError(
            try reader.withView { _ in }
        ) { error in
            XCTAssertEqual(error as? FileProviderSynchronizationError, .unauthorized)
        }
    }

    // MARK: - Group G: Concurrency
    func testConcurrentContentReads() throws {
        let (_, versionIdentity, _, _, _, _) = try publishMetadataAndContent()

        let queue = DispatchQueue(
            label: "test.concurrent.read",
            attributes: .concurrent
        )
        let exp1 = expectation(description: "read 1")
        let exp2 = expectation(description: "read 2")

        queue.async {
            do {
                _ = try self.contentStore.verifyAndAccessContent(
                    domain: self.domain,
                    fileVersionID: versionIdentity.fileVersionID,
                    expectedOwnerScopeFingerprint: self.domain
                        .ownerScopeFingerprint,
                    expectedAuthorizationGeneration: 1
                )
                exp1.fulfill()
            } catch { XCTFail("Read 1 failed: \(error)") }
        }

        queue.async {
            do {
                _ = try self.contentStore.verifyAndAccessContent(
                    domain: self.domain,
                    fileVersionID: versionIdentity.fileVersionID,
                    expectedOwnerScopeFingerprint: self.domain
                        .ownerScopeFingerprint,
                    expectedAuthorizationGeneration: 1
                )
                exp2.fulfill()
            } catch { XCTFail("Read 2 failed: \(error)") }
        }

        wait(for: [exp1, exp2], timeout: 5.0)
    }

    // MARK: - Group H: Quota Enforcement
    func testStorageQuotaAndEviction() throws {
        let node = nodeID(101)
        let fileVersion = nodeID(901)
        let rawData = Data("oversized-content-test".utf8)
        let sha256String = sha256Hex(data: rawData)

        let manifest = FileProviderContentManifest(
            schemaVersion: 1,
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            authorizationGeneration: 1,
            libraryID: library,
            nodeID: node,
            fileVersionID: fileVersion,
            expectedByteLength: UInt64(
                FileProviderSharedContainer.maximumProviderStorageBytes + 1
            ),
            expectedSHA256: sha256String,
            contentID: UUID().uuidString.lowercased(),
            storageKey: "\(domain.fingerprint)-\(fileVersion).bin",
            publicationGeneration: 1,
            verifiedState: "verified",
            metadataGeneration: 1
        )

        let tempFile = directory.appendingPathComponent("oversized.tmp")
        try rawData.write(to: tempFile)

        XCTAssertThrowsError(
            try contentStore.publish(
                sourceURL: tempFile,
                manifest: manifest,
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
                capability: store.capability
            )
        ) { error in
            XCTAssertEqual(
                error as? FileProviderContentStoreError,
                .storageQuotaExceeded
            )
        }
    }

    // MARK: - Group I: Item Capabilities & Downloaded Status
    func testItemCapabilitiesAndDownloadedStatus() throws {
        let (itemIdentity, versionIdentity, _, _, data, _) =
            try publishMetadataAndContent()

        let projectedItem = FileProviderProjectedItem(
            identity: itemIdentity,
            parentIdentity: try! FileProviderItemIdentity.libraryRoot(
                domain: domain,
                libraryID: library
            ),
            kind: .file,
            displayName: "document.pdf",
            byteLength: UInt64(data.count),
            creationDate: Date(),
            contentModificationDate: Date(),
            fileVersionIdentity: versionIdentity,
            contentSHA256: sha256Hex(data: data),
            metadataRevision: "1",
            isTombstone: false,
            source: .serverConfirmed
        )

        let item = SynveilFileProviderItem(
            projected: projectedItem,
            domain: domain,
            container: container
        )
        XCTAssertTrue(item.capabilities.contains(.allowsReading))
        XCTAssertFalse(
            item.capabilities.contains(.allowsContentEnumerating)
        )
        XCTAssertTrue(item.isDownloaded)
    }

    // MARK: - Group J: Error Mapping Restrictions Fail-Closed
    func testErrorMappingFailClosed() throws {
        let errNoSuch = FileProviderErrorMapper.map(
            FileProviderContentStoreError.manifestNotFound
        )
        XCTAssertEqual(errNoSuch.domain, NSFileProviderErrorDomain)
        XCTAssertEqual(errNoSuch.code, NSFileProviderError.noSuchItem.rawValue)

        let errSync = FileProviderErrorMapper.map(
            FileProviderContentStoreError.sha256Mismatch
        )
        XCTAssertEqual(errSync.domain, NSFileProviderErrorDomain)
        XCTAssertEqual(
            errSync.code,
            NSFileProviderError.cannotSynchronize.rawValue
        )

        let errCancel = FileProviderErrorMapper.map(
            FileProviderContentStoreError.cancelled
        )
        XCTAssertEqual(errCancel.domain, NSCocoaErrorDomain)
        XCTAssertEqual(errCancel.code, NSUserCancelledError)

        let errUnsupported = FileProviderErrorMapper.unsupported()
        XCTAssertEqual(errUnsupported.domain, NSFileProviderErrorDomain)
        XCTAssertEqual(
            errUnsupported.code,
            NSFileProviderError.cannotSynchronize.rawValue
        )
    }
}

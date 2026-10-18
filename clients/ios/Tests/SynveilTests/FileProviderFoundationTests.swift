import Foundation
import XCTest

@testable import Synveil

final class FileProviderFoundationTests: XCTestCase {
    private enum Fixture {
        static let libraryID = "018f1f5e-7b80-7000-8000-000000000010"
        static let alternateLibraryID = "018f1f5e-7b80-7000-8000-000000000011"
        static let folderNodeID = "018f1f5e-7b80-7000-8000-000000000020"
        static let fileNodeID = "018f1f5e-7b80-7000-8000-000000000021"
        static let stableNodeID = "018f1f5e-7b80-7000-8000-000000000022"
        static let fileVersionID = "018f1f5e-7b80-7000-8000-000000000030"
        static let replacementVersionID = "018f1f5e-7b80-7000-8000-000000000031"
        static let orphanNodeID = "018f1f5e-7b80-7000-8000-000000000040"
    }

    private let domain = try! FileProviderDomainIdentity(
        endpointOrigin: "https://files.example.test:443/",
        accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
        deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
        credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003"
    )

    private let alternateDomain = try! FileProviderDomainIdentity(
        endpointOrigin: "https://other.example.test",
        accountIdentity: "018f1f5e-7b80-7000-8000-000000000004",
        deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
        credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003"
    )

    private var publicationCapability: FileProviderCapability {
        FileProviderCapability(
            domainRegistrationEnabled: false,
            metadataPublicationEnabled: true,
            authenticatedNetworkEnabled: false,
            mutationEnabled: false
        )
    }

    private func validSnapshot(
        source: FileProviderProjectionSource = .serverConfirmed,
        completeness: FileProviderProjectionCompleteness = .complete,
        schemaVersion: Int = FileProviderMetadataSnapshot.currentSchemaVersion
    ) throws -> FileProviderMetadataSnapshot {
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
            schemaVersion: schemaVersion,
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            libraryID: libraryID,
            projectionGeneration: 7,
            completeness: completeness,
            items: [
                FileProviderProjectedItem(
                    identity: root,
                    parentIdentity: domainRoot,
                    kind: .libraryRoot,
                    displayName: "Work",
                    byteLength: nil,
                    creationDate: nil,
                    contentModificationDate: nil,
                    fileVersionIdentity: nil,
                    contentSHA256: nil,
                    metadataRevision: "library-revision-9",
                    isTombstone: false,
                    source: source
                ),
                FileProviderProjectedItem(
                    identity: folderID,
                    parentIdentity: root,
                    kind: .folder,
                    displayName: "Folder",
                    byteLength: nil,
                    creationDate: timestamp,
                    contentModificationDate: nil,
                    fileVersionIdentity: nil,
                    contentSHA256: nil,
                    metadataRevision: "3",
                    isTombstone: false,
                    source: source
                ),
                FileProviderProjectedItem(
                    identity: fileID,
                    parentIdentity: folderID,
                    kind: .file,
                    displayName: "notes.txt",
                    byteLength: 14,
                    creationDate: nil,
                    contentModificationDate: timestamp,
                    fileVersionIdentity: version,
                    contentSHA256: "sha256:" + String(repeating: "a", count: 64),
                    metadataRevision: "11",
                    isTombstone: false,
                    source: source
                ),
            ]
        )
    }

    func testDomainIdentityIsStableScopedAndCredentialGenerationBound() throws {
        let canonicalEquivalent = try FileProviderDomainIdentity(
            endpointOrigin: "HTTPS://FILES.EXAMPLE.TEST",
            accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
            deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
            credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003"
        )
        let credentialReplacement = try FileProviderDomainIdentity(
            endpointOrigin: "https://files.example.test",
            accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
            deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
            credentialGeneration: "018f1f5e-7b80-7000-8000-000000000005"
        )
        let otherAccount = try FileProviderDomainIdentity(
            endpointOrigin: "https://files.example.test",
            accountIdentity: "018f1f5e-7b80-7000-8000-000000000004",
            deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
            credentialGeneration: "018f1f5e-7b80-7000-8000-000000000003"
        )

        XCTAssertEqual(domain.rawValue, canonicalEquivalent.rawValue)
        XCTAssertNotEqual(domain.rawValue, credentialReplacement.rawValue)
        XCTAssertNotEqual(domain.rawValue, otherAccount.rawValue)
        XCTAssertNotEqual(domain.rawValue, alternateDomain.rawValue)
        XCTAssertFalse(domain.rawValue.contains("files.example.test"))
        XCTAssertFalse(domain.rawValue.contains("018f1f5e-7b80-7000-8000-000000000001"))
    }

    func testDomainIdentityRejectsNonCanonicalOrSecretScopeValues() {
        XCTAssertThrowsError(
            try FileProviderDomainIdentity(
                endpointOrigin: "https://user:password@example.test",
                accountIdentity: "account",
                deviceIdentity: "device",
                credentialGeneration: "generation"
            ))
        XCTAssertThrowsError(
            try FileProviderDomainIdentity(
                endpointOrigin: "http://files.example.test",
                accountIdentity: "account",
                deviceIdentity: "device",
                credentialGeneration: "generation"
            ))
        XCTAssertThrowsError(
            try FileProviderDomainIdentity(
                endpointOrigin: "https://files.example.test",
                accountIdentity: "account",
                deviceIdentity: "device",
                credentialGeneration: "svd1_sensitive-bearer"
            ))
    }

    func testItemIdentitySurvivesRenameAndMoveAndSeparatesLibraryAndDomain() throws {
        let first = try FileProviderItemIdentity.node(
            domain: domain,
            libraryID: Fixture.libraryID,
            nodeID: Fixture.stableNodeID
        )
        let afterRenameAndMove = try FileProviderItemIdentity.node(
            domain: domain,
            libraryID: Fixture.libraryID,
            nodeID: Fixture.stableNodeID
        )
        let otherLibrary = try FileProviderItemIdentity.node(
            domain: domain,
            libraryID: Fixture.alternateLibraryID,
            nodeID: Fixture.stableNodeID
        )
        let otherDomain = try FileProviderItemIdentity.node(
            domain: alternateDomain,
            libraryID: Fixture.libraryID,
            nodeID: Fixture.stableNodeID
        )

        XCTAssertEqual(first, afterRenameAndMove)
        XCTAssertNotEqual(first.rawValue, otherLibrary.rawValue)
        XCTAssertNotEqual(first.rawValue, otherDomain.rawValue)
        XCTAssertEqual(
            try FileProviderItemIdentity(
                rawValue: first.rawValue,
                expectedDomain: domain,
                expectedLibraryID: Fixture.libraryID
            ),
            first)
        XCTAssertThrowsError(
            try FileProviderItemIdentity(
                rawValue: first.rawValue,
                expectedDomain: domain,
                expectedLibraryID: Fixture.alternateLibraryID
            ))
        XCTAssertThrowsError(
            try FileProviderItemIdentity(rawValue: first.rawValue, expectedDomain: alternateDomain))
        XCTAssertThrowsError(
            try FileProviderItemIdentity(rawValue: "sfpi1.node...", expectedDomain: domain))
    }

    func testFileVersionIdentityIsImmutableAndDistinctFromNodeIdentity() throws {
        let first = try FileProviderVersionIdentity(
            domain: domain,
            libraryID: Fixture.libraryID,
            nodeID: Fixture.fileNodeID,
            fileVersionID: Fixture.fileVersionID
        )
        let replacement = try FileProviderVersionIdentity(
            domain: domain,
            libraryID: Fixture.libraryID,
            nodeID: Fixture.fileNodeID,
            fileVersionID: Fixture.replacementVersionID
        )
        let node = try FileProviderItemIdentity.node(
            domain: domain,
            libraryID: Fixture.libraryID,
            nodeID: Fixture.fileNodeID
        )

        XCTAssertNotEqual(first.rawValue, replacement.rawValue)
        XCTAssertFalse(first.rawValue.contains(node.rawValue))
        XCTAssertEqual(first.fileVersionID, Fixture.fileVersionID)

        let firstComponents = try FileProviderItemVersionContract(
            fileVersionIdentity: first, metadataRevision: "11")
        let changedComponents = try FileProviderItemVersionContract(
            fileVersionIdentity: replacement, metadataRevision: "12")
        let noEvidence = try FileProviderItemVersionContract(
            fileVersionIdentity: nil, metadataRevision: nil)
        XCTAssertEqual(firstComponents.contentVersion?.count, 32)
        XCTAssertEqual(firstComponents.metadataVersion?.count, 32)
        XCTAssertNotEqual(firstComponents.contentVersion, changedComponents.contentVersion)
        XCTAssertNotEqual(firstComponents.metadataVersion, changedComponents.metadataVersion)
        XCTAssertNil(noEvidence.contentVersion)
        XCTAssertNil(noEvidence.metadataVersion)
    }

    func testProductionRegistrationAndAllRemoteCapabilitiesAreDisabled() async throws {
        let manager = FakeFileProviderDomainManager(existing: ["existing-domain"])
        let coordinator = FileProviderDomainRegistrationCoordinator(manager: manager)
        XCTAssertFalse(FileProviderCapability.production.permitsAnyProductionOperation)
        let existing = try await coordinator.inspectExistingDomains()
        XCTAssertEqual(existing, ["existing-domain"])

        do {
            try await coordinator.register(domain)
            XCTFail("Production registration must remain disabled")
        } catch let error as FileProviderDomainManagementError {
            XCTAssertEqual(error, .registrationDisabled)
        }
        let addCount = await manager.addCount
        XCTAssertEqual(addCount, 0)
        XCTAssertEqual(
            FileProviderDomainRegistrationCoordinator.classify(
                FileProviderDomainManagementError.registrationDisabled),
            .disabled)
    }

    func testInjectedDomainCoordinatorSupportsFutureRegisterDisconnectAndRemove() async throws {
        let enabled = FileProviderCapability(
            domainRegistrationEnabled: true,
            metadataPublicationEnabled: false,
            authenticatedNetworkEnabled: false,
            mutationEnabled: false
        )
        let manager = FakeFileProviderDomainManager()
        let coordinator = FileProviderDomainRegistrationCoordinator(
            capability: enabled, manager: manager)

        try await coordinator.register(domain)
        do {
            try await coordinator.register(domain)
            XCTFail("Duplicate domain should be classified")
        } catch let error as FileProviderDomainManagementError {
            XCTAssertEqual(error, .domainAlreadyRegistered)
        }
        try await coordinator.disconnect(identifier: domain.rawValue)
        try await coordinator.remove(identifier: domain.rawValue)
        let addCount = await manager.addCount
        let disconnectCount = await manager.disconnectCount
        let removeCount = await manager.removeCount
        XCTAssertEqual(addCount, 1)
        XCTAssertEqual(disconnectCount, 1)
        XCTAssertEqual(removeCount, 1)
    }

    func testAuthorizationEvidenceIsNonSecretAndRequiresExactScope() throws {
        let evidence = FileProviderAuthorizationEvidence(
            domain: domain, state: .authorized, authorizationGeneration: 4)
        let encoded = try JSONEncoder().encode(evidence)
        let text = try XCTUnwrap(String(data: encoded, encoding: .utf8))

        XCTAssertNil(FileProviderAuthorizationBoundary.sharedKeychainAccessGroup)
        XCTAssertFalse(FileProviderAuthorizationBoundary.extensionMayReadDeviceBearer)
        XCTAssertFalse(text.contains("svd1_"))
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: evidence, expectedDomain: domain, protectedDataAvailable: true),
            .denied(.productionCapabilityDisabled))
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: nil, expectedDomain: domain, protectedDataAvailable: true),
            .denied(.missingEvidence))
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: evidence, expectedDomain: alternateDomain, protectedDataAvailable: true),
            .denied(.wrongDomain))
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: evidence, expectedDomain: domain, protectedDataAvailable: false),
            .denied(.protectedDataUnavailable))
    }

    func testAuthorizationDeniesWrongCredentialGenerationLogoutRevocationAndAccountSwitch() throws {
        let authorizedEvidence = FileProviderAuthorizationEvidence(
            domain: domain, state: .authorized, authorizationGeneration: 5)
        let wrongCredential = try corruptedEvidence(
            authorizedEvidence, replacing: "credentialGenerationFingerprint",
            with: String(repeating: "0", count: 64))
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: wrongCredential, expectedDomain: domain, protectedDataAvailable: true),
            .denied(.wrongCredentialGeneration))
        let zeroGeneration = try corruptedEvidence(
            authorizedEvidence, replacing: "authorizationGeneration", with: 0)
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: zeroGeneration, expectedDomain: domain, protectedDataAvailable: true),
            .denied(.invalidAuthorizationGeneration))

        for (state, denial) in [
            (FileProviderAuthorizationState.loggedOut, FileProviderAuthorizationDenial.loggedOut),
            (.revoked, .revoked),
            (.accountSwitched, .accountSwitched),
        ] {
            let evidence = FileProviderAuthorizationEvidence(
                domain: domain, state: state, authorizationGeneration: 6)
            XCTAssertEqual(
                FileProviderAuthorizationBoundary.evaluate(
                    evidence: evidence, expectedDomain: domain, protectedDataAvailable: true),
                .denied(denial))
        }
    }

    func testCredentialReplacementAndExtensionRelaunchStartWithoutAuthorization() throws {
        let replacement = try FileProviderDomainIdentity(
            endpointOrigin: "https://files.example.test",
            accountIdentity: "018f1f5e-7b80-7000-8000-000000000001",
            deviceIdentity: "018f1f5e-7b80-7000-8000-000000000002",
            credentialGeneration: "018f1f5e-7b80-7000-8000-000000000005")
        let staleEvidence = FileProviderAuthorizationEvidence(
            domain: domain, state: .authorized, authorizationGeneration: 7)
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: staleEvidence, expectedDomain: replacement, protectedDataAvailable: true),
            .denied(.wrongDomain))

        let relaunchedProcess = FileProviderProcessLifecycle()
        XCTAssertEqual(relaunchedProcess.state, .cold)
        XCTAssertFalse(relaunchedProcess.mayAccessApplicationSession)
        XCTAssertFalse(relaunchedProcess.mayStartAuthenticatedNetwork)
        XCTAssertFalse(relaunchedProcess.maySubmitBackgroundTransfers)
        XCTAssertEqual(
            FileProviderAuthorizationBoundary.evaluate(
                evidence: nil, expectedDomain: domain, protectedDataAvailable: true),
            .denied(.missingEvidence))
    }

    func testLifecycleInitializationCancellationAndInvalidationAreLocalAndIdempotent() {
        let lifecycle = FileProviderProcessLifecycle()
        XCTAssertFalse(lifecycle.canContinueRequest())
        lifecycle.launch()
        lifecycle.launch()
        XCTAssertEqual(lifecycle.state, .active)
        XCTAssertTrue(lifecycle.canContinueRequest())
        lifecycle.invalidate()
        lifecycle.invalidate()
        XCTAssertEqual(lifecycle.state, .invalidated)
        XCTAssertFalse(lifecycle.canContinueRequest())
        lifecycle.launch()
        XCTAssertFalse(lifecycle.canContinueRequest())
    }

    func testSnapshotProjectionIsVersionedCanonicalAndDeterministicallyOrdered() throws {
        let snapshot = try validSnapshot()
        let reversed = FileProviderMetadataSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            libraryID: snapshot.libraryID,
            projectionGeneration: snapshot.projectionGeneration,
            completeness: .complete,
            items: snapshot.items.reversed()
        )
        let data = try snapshot.encoded(
            expectedDomain: domain, expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint)
        let reversedData = try reversed.encoded(
            expectedDomain: domain, expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint)
        XCTAssertEqual(data, reversedData)

        let decoded = try FileProviderMetadataSnapshot.decode(
            data, expectedDomain: domain,
            expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint)
        let identities = decoded.items.map(\.identity.rawValue)
        XCTAssertEqual(identities, identities.sorted())
        XCTAssertNil(decoded.items.first(where: { $0.kind == .libraryRoot })?.creationDate)
        let file = decoded.items.first(where: { $0.kind == .file })
        XCTAssertEqual(file?.byteLength, 14)
        XCTAssertEqual(file?.fileVersionIdentity?.fileVersionID, Fixture.fileVersionID)
        XCTAssertFalse(String(decoding: data, as: UTF8.self).contains("svd1_"))
    }

    func testSnapshotRejectsFutureSchemaCorruptionWrongOwnerAndUnresolvedConflict() throws {
        let future = try validSnapshot(schemaVersion: 2)
        XCTAssertThrowsError(
            try future.encoded(
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
            )
        ) { XCTAssertEqual($0 as? FileProviderMetadataContractError, .unsupportedSchema) }
        XCTAssertThrowsError(
            try FileProviderMetadataSnapshot.decode(
                Data("not-json".utf8), expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
            )
        ) { XCTAssertEqual($0 as? FileProviderMetadataContractError, .corruptSnapshot) }
        XCTAssertThrowsError(
            try validSnapshot().validated(
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: "wrong-scope"
            )
        ) { XCTAssertEqual($0 as? FileProviderMetadataContractError, .ownerScopeMismatch) }
        let conflict = try validSnapshot(source: .conflictReview)
        XCTAssertThrowsError(
            try conflict.validated(
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
            )
        ) { XCTAssertEqual($0 as? FileProviderMetadataContractError, .nonCanonicalSource) }
    }

    func testPartialProjectionAndInvalidParentageAreRejected() throws {
        let partial = try validSnapshot(completeness: .partial)
        XCTAssertThrowsError(
            try partial.validated(
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
            )
        ) { XCTAssertEqual($0 as? FileProviderMetadataContractError, .incompleteOrUnbounded) }

        let snapshot = try validSnapshot()
        let orphan = try FileProviderItemIdentity.node(
            domain: domain,
            libraryID: snapshot.libraryID,
            nodeID: Fixture.orphanNodeID
        )
        let brokenItems = snapshot.items.map { item in
            guard item.kind == .file else { return item }
            return FileProviderProjectedItem(
                identity: item.identity,
                parentIdentity: orphan,
                kind: item.kind,
                displayName: item.displayName,
                byteLength: item.byteLength,
                creationDate: item.creationDate,
                contentModificationDate: item.contentModificationDate,
                fileVersionIdentity: item.fileVersionIdentity,
                contentSHA256: item.contentSHA256,
                metadataRevision: item.metadataRevision,
                isTombstone: item.isTombstone,
                source: item.source
            )
        }
        let brokenSnapshot = FileProviderMetadataSnapshot(
            domainIdentity: domain,
            ownerScopeFingerprint: domain.ownerScopeFingerprint,
            libraryID: snapshot.libraryID,
            projectionGeneration: snapshot.projectionGeneration,
            completeness: .complete,
            items: brokenItems
        )
        XCTAssertThrowsError(
            try brokenSnapshot.validated(
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
            )
        ) { XCTAssertEqual($0 as? FileProviderMetadataContractError, .invalidParent) }
    }

    func testAppGroupResolutionAndUnsafeOrSymlinkPathsFailClosed() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let requestedGroup = GroupIdentifierRecorder()
        let container = FileProviderSharedContainer(resolveContainerURL: { identifier in
            requestedGroup.record(identifier)
            return directory
        })
        let resolved = try container.url(for: ["metadata", "safe.json"])
        XCTAssertEqual(requestedGroup.value, "group.com.synveil.ios")
        XCTAssertTrue(resolved.path.hasPrefix(directory.path + "/Synveil/FileProvider/v1/"))
        XCTAssertThrowsError(try container.url(for: ["..", "escape"])) {
            XCTAssertEqual($0 as? FileProviderSharedContainerError, .unsafePath)
        }
        XCTAssertThrowsError(try container.url(for: ["bad/name"])) {
            XCTAssertEqual($0 as? FileProviderSharedContainerError, .unsafePath)
        }
        let disabledWriter = FileProviderMetadataSnapshotStore(container: container)
        XCTAssertThrowsError(
            try disabledWriter.publish(
                validSnapshot(),
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint
            )
        ) { XCTAssertEqual($0 as? FileProviderSharedContainerError, .publicationFailed) }

        let missingEntitlement = FileProviderSharedContainer(resolveContainerURL: { _ in nil })
        XCTAssertThrowsError(try missingEntitlement.url(for: [])) {
            XCTAssertEqual($0 as? FileProviderSharedContainerError, .missingEntitlement)
        }
        let unavailable = FileProviderSharedContainer(resolveContainerURL: { _ in
            directory.appendingPathComponent("unavailable", isDirectory: true)
        })
        XCTAssertThrowsError(try unavailable.url(for: [])) {
            XCTAssertEqual($0 as? FileProviderSharedContainerError, .containerUnavailable)
        }

        let outside = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: outside) }
        let link = directory.appendingPathComponent("Synveil")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: outside)
        XCTAssertThrowsError(try container.url(for: ["metadata"])) {
            XCTAssertEqual($0 as? FileProviderSharedContainerError, .symbolicLink)
        }
    }

    func testSnapshotPublicationFailurePreservesPreviousGeneration() throws {
        let directory = try temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let writer = FileProviderMetadataSnapshotStore(
            container: FileProviderSharedContainer(resolveContainerURL: { _ in directory }))
        let first = try validSnapshot()
        try writer.publish(
            first, expectedDomain: domain,
            expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
            capability: publicationCapability)
        XCTAssertEqual(
            try writer.read(
                domain: domain,
                libraryID: Fixture.libraryID,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint),
            try first.validated(
                expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint))
        let destination = try FileProviderSharedContainer(
            resolveContainerURL: { _ in directory }
        ).snapshotURL(domain: domain, libraryID: Fixture.libraryID)
        let priorBytes = try Data(contentsOf: destination)

        let failingStore = FileProviderMetadataSnapshotStore(
            container: FileProviderSharedContainer(
                resolveContainerURL: { _ in directory },
                beforeAtomicReplace: { throw InjectedPublicationFailure.failed }
            ))
        XCTAssertThrowsError(
            try failingStore.publish(
                first, expectedDomain: domain,
                expectedOwnerScopeFingerprint: domain.ownerScopeFingerprint,
                capability: publicationCapability
            )
        ) { XCTAssertEqual($0 as? FileProviderSharedContainerError, .publicationFailed) }
        XCTAssertEqual(try Data(contentsOf: destination), priorBytes)
        let stagedFiles = try FileManager.default.contentsOfDirectory(
            atPath: destination.deletingLastPathComponent().path
        ).filter { $0.hasPrefix(".snapshot-") }
        XCTAssertTrue(stagedFiles.isEmpty)
    }

    func testUnsupportedOperationsNeverReportSuccess() {
        XCTAssertEqual(
            FileProviderOperationPolicy.disposition(for: .enumerate), .authorizationUnavailable)
        XCTAssertEqual(
            FileProviderOperationPolicy.disposition(for: .fetchContents), .authorizationUnavailable)
        for operation in [FileProviderOperation.create, .modify, .delete] {
            XCTAssertEqual(FileProviderOperationPolicy.disposition(for: operation), .unsupported)
        }
        XCTAssertFalse(FileProviderCapability.production.authenticatedNetworkEnabled)
        XCTAssertFalse(FileProviderCapability.production.mutationEnabled)
    }

    private func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "SynveilFileProviderTests-\(UUID().uuidString)", isDirectory: true
            )
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func corruptedEvidence(
        _ evidence: FileProviderAuthorizationEvidence,
        replacing key: String,
        with replacement: Any
    ) throws -> FileProviderAuthorizationEvidence {
        let encoded = try JSONEncoder().encode(evidence)
        var object = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        object[key] = replacement
        let corrupted = try JSONSerialization.data(withJSONObject: object)
        return try JSONDecoder().decode(FileProviderAuthorizationEvidence.self, from: corrupted)
    }
}

private enum InjectedPublicationFailure: Error {
    case failed
}

private final class GroupIdentifierRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var storedValue: String?

    var value: String? {
        lock.lock()
        defer { lock.unlock() }
        return storedValue
    }

    func record(_ value: String) {
        lock.lock()
        defer { lock.unlock() }
        storedValue = value
    }
}

private actor FakeFileProviderDomainManager: FileProviderDomainManaging {
    private var identifiers: Set<String>
    private(set) var addCount = 0
    private(set) var disconnectCount = 0
    private(set) var removeCount = 0

    init(existing: Set<String> = []) {
        identifiers = existing
    }

    func registeredDomainIdentifiers() async throws -> Set<String> {
        identifiers
    }

    func addDomain(_ identity: FileProviderDomainIdentity) async throws {
        identifiers.insert(identity.rawValue)
        addCount += 1
    }

    func disconnectDomain(identifier: String) async throws {
        disconnectCount += 1
    }

    func removeDomain(identifier: String) async throws {
        identifiers.remove(identifier)
        removeCount += 1
    }
}

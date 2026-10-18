import CryptoKit
import Foundation
import XCTest

@testable import Synveil

@MainActor
final class PhotoResourceTransferTests: XCTestCase {
    func testAutomaticBackupDefaultsOffAndRequiresExplicitConsent() async throws {
        let photoStore = try makePhotoStore()
        let profileId = UUID()
        let defaults = try await photoStore.fetchConfiguration(profileId: profileId)
        XCTAssertFalse(defaults.discoveryEnabled)
        XCTAssertFalse(defaults.automaticForegroundBackupEnabled)
        XCTAssertFalse(defaults.automaticBackupConsentConfirmed)
        XCTAssertEqual(defaults.maximumConcurrentOperations, 1)

        let incomplete = PhotoBackupConfiguration(
            targetLibraryIdentifier: "library",
            targetParentIdentifier: "parent",
            automaticForegroundBackupEnabled: true,
            automaticBackupConsentConfirmed: false,
            discoveryEnabled: true)
        do {
            try await photoStore.saveConfiguration(incomplete, profileId: profileId)
            XCTFail("Automatic backup must require an independent explicit confirmation")
        } catch {}

        let confirmed = PhotoBackupConfiguration(
            targetLibraryIdentifier: "library",
            targetParentIdentifier: "parent",
            automaticForegroundBackupEnabled: true,
            automaticBackupConsentConfirmed: true,
            discoveryEnabled: true)
        try await photoStore.saveConfiguration(confirmed, profileId: profileId)
        let saved = try await photoStore.fetchConfiguration(profileId: profileId)
        XCTAssertTrue(saved.canAutomaticallyBackUpInForeground)
        XCTAssertEqual(saved.policyRevision, 1)
    }

    func testDurableQueueDeduplicatesCallbacksButKeepsNewSourceRevision() async throws {
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "queue-owner")
        let selection = makeSelection()
        let observation = makeObservation(selection: selection, generation: 4)
        let config = activeAutomaticConfiguration()
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(config, profileId: authorization.profileId)
        try await photoStore.commitObservations([observation], authorization: authorization)
        let candidate = makeCandidate(
            authorization: authorization, observation: observation, selection: selection,
            configuration: config, deviceId: "queue-device")

        let firstInsert = try await photoStore.enqueueBackupCandidates(
            [candidate], authorization: authorization, maximumPending: 100)
        let duplicateInsert = try await photoStore.enqueueBackupCandidates(
            [candidate], authorization: authorization, maximumPending: 100)
        let originalCandidates = try await photoStore.backupCandidates(
            authorization: authorization, limit: 100)
        XCTAssertEqual(firstInsert, 1)
        XCTAssertEqual(duplicateInsert, 0)
        XCTAssertEqual(originalCandidates.count, 1)

        let revisedSelection = makeSelection(
            sourceModificationDate: Date(timeIntervalSince1970: 20))
        let revisedObservation = makeObservation(selection: revisedSelection, generation: 5)
        try await photoStore.commitObservations([revisedObservation], authorization: authorization)
        let revised = makeCandidate(
            authorization: authorization, observation: revisedObservation,
            selection: revisedSelection, configuration: config, deviceId: "queue-device")
        let revisedInsert = try await photoStore.enqueueBackupCandidates(
            [revised], authorization: authorization, maximumPending: 100)
        XCTAssertEqual(revisedInsert, 1)
        let candidates = try await photoStore.backupCandidates(
            authorization: authorization, limit: 100)
        XCTAssertEqual(candidates.count, 2)
        XCTAssertEqual(Set(candidates.map(\.sourceRevision)).count, 2)
    }

    func testCompletedDiscoveryCursorRemainsAtEndForSameScanGeneration() async throws {
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "cursor-owner")
        let config = activeAutomaticConfiguration()
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(config, profileId: authorization.profileId)

        try await photoStore.saveBackupDiscoveryCursor(
            "last-local-identifier", authorization: authorization, deviceId: "cursor-device",
            policyRevision: config.policyRevision, scanGeneration: 41)
        let firstCursor = try await photoStore.backupDiscoveryCursor(
            authorization: authorization, deviceId: "cursor-device",
            policyRevision: config.policyRevision, scanGeneration: 41)
        let page = try await photoStore.fetchObservationPage(
            profileId: authorization.profileId, ownerId: authorization.ownerId,
            scanGeneration: 41, after: firstCursor, limit: 500)
        let resumedCursor = try await photoStore.backupDiscoveryCursor(
            authorization: authorization, deviceId: "cursor-device",
            policyRevision: config.policyRevision, scanGeneration: 41)

        XCTAssertEqual(firstCursor, "last-local-identifier")
        XCTAssertTrue(page.isEmpty)
        XCTAssertEqual(resumedCursor, "last-local-identifier")
    }

    func testQueueCapacityRejectsWholeCandidateBatchWithoutAdvancingWork() async throws {
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "capacity-owner")
        let selection = makeSelection()
        let observation = makeObservation(selection: selection, generation: 10)
        let config = activeAutomaticConfiguration()
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(config, profileId: authorization.profileId)
        try await photoStore.commitObservations([observation], authorization: authorization)
        let first = makeCandidate(
            authorization: authorization, observation: observation, selection: selection,
            configuration: config, deviceId: "capacity-device")
        let secondSelection = PhotoResourceSelection(
            localIdentifier: selection.localIdentifier, resourceIndex: 1,
            resourceType: selection.resourceType, role: selection.role,
            rendition: selection.rendition, uniformTypeIdentifier: selection.uniformTypeIdentifier,
            sourceModificationDate: selection.sourceModificationDate)
        let second = makeCandidate(
            authorization: authorization, observation: observation, selection: secondSelection,
            configuration: config, deviceId: "capacity-device")

        do {
            _ = try await photoStore.enqueueBackupCandidates(
                [first, second], authorization: authorization, maximumPending: 1)
            XCTFail("A partially accepted candidate batch must not commit at queue capacity")
        } catch let failure as PhotoResourceTransferFailure {
            XCTAssertEqual(failure, .queueCapacityReached)
        }
        let rejectedCandidates = try await photoStore.backupCandidates(
            authorization: authorization, limit: 10)
        XCTAssertTrue(rejectedCandidates.isEmpty)

        let retry = try await photoStore.enqueueBackupCandidates(
            [first], authorization: authorization, maximumPending: 1)
        XCTAssertEqual(retry, 1)
    }

    func testQueueLeaseRecoveryReturnsInterruptedExtractionToQueue() async throws {
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "lease-owner")
        let selection = makeSelection()
        let observation = makeObservation(selection: selection, generation: 6)
        let config = activeAutomaticConfiguration()
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(config, profileId: authorization.profileId)
        try await photoStore.commitObservations([observation], authorization: authorization)
        let candidate = makeCandidate(
            authorization: authorization, observation: observation, selection: selection,
            configuration: config, deviceId: "lease-device")
        _ = try await photoStore.enqueueBackupCandidates(
            [candidate], authorization: authorization, maximumPending: 100)
        let expiry = Date().addingTimeInterval(60)
        let claimResult = try await photoStore.claimNextBackupCandidate(
            authorization: authorization, deviceId: "lease-device", claimId: UUID(),
            leaseUntil: expiry, policyRevision: config.policyRevision)
        let claimed = try XCTUnwrap(claimResult)
        let extracting = claimed.replacing(
            state: .extracting, claimId: claimed.claimId,
            claimExpiresAt: claimed.claimExpiresAt,
            claimPreviousState: claimed.claimPreviousState)
        try await photoStore.transitionBackupCandidate(
            extracting, expectedState: .claimed, authorization: authorization)

        try await photoStore.recoverExpiredBackupClaims(
            authorization: authorization, before: expiry.addingTimeInterval(1))
        let recovered = try await photoStore.backupCandidates(
            authorization: authorization, limit: 100)
        XCTAssertEqual(recovered.first?.state, .queued)
        XCTAssertEqual(recovered.first?.safeFailure, .stagingCorrupt)
        XCTAssertNil(recovered.first?.claimId)
    }

    func testUnknownUploadLeaseRecoveryPreservesOriginalP045Identity() async throws {
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "upload-lease-owner")
        let selection = makeSelection()
        let observation = makeObservation(selection: selection, generation: 7)
        let config = activeAutomaticConfiguration()
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(config, profileId: authorization.profileId)
        try await photoStore.commitObservations([observation], authorization: authorization)
        let candidate = makeCandidate(
            authorization: authorization, observation: observation, selection: selection,
            configuration: config, deviceId: "upload-lease-device")
        _ = try await photoStore.enqueueBackupCandidates(
            [candidate], authorization: authorization, maximumPending: 100)
        let expiry = Date().addingTimeInterval(60)
        let claimResult = try await photoStore.claimNextBackupCandidate(
            authorization: authorization, deviceId: "upload-lease-device", claimId: UUID(),
            leaseUntil: expiry, policyRevision: config.policyRevision)
        let claimed = try XCTUnwrap(claimResult)
        let uploadId = UploadOperationId()
        let uploading = claimed.replacing(
            state: .uploading, claimId: claimed.claimId,
            claimExpiresAt: claimed.claimExpiresAt,
            claimPreviousState: claimed.claimPreviousState,
            extractionOperationId: UUID(), uploadOperationId: uploadId.rawValue)
        try await photoStore.transitionBackupCandidate(
            uploading, expectedState: .claimed, authorization: authorization)

        try await photoStore.recoverExpiredBackupClaims(
            authorization: authorization, before: expiry.addingTimeInterval(1))
        let recovered = try await photoStore.backupCandidates(
            authorization: authorization, limit: 100)
        XCTAssertEqual(recovered.first?.state, .reconciliationPending)
        XCTAssertEqual(recovered.first?.uploadOperationId, uploadId.rawValue)
        XCTAssertEqual(recovered.first?.safeFailure, .uploadOutcomeUnknown)
    }

    func testQueueAdoptionReconcilesExactUploadAfterAuthorizationGenerationChanges() async throws {
        let photoStore = try makePhotoStore()
        let profileId = UUID()
        let oldAuthorization = PhotoAuthorizationSession(
            profileId: profileId, ownerId: "restart-owner", sessionRevision: 3)
        let selection = makeSelection()
        let observation = makeObservation(selection: selection, generation: 8)
        let config = activeAutomaticConfiguration()
        try await photoStore.activateAuthorization(oldAuthorization)
        try await photoStore.saveConfiguration(config, profileId: profileId)
        try await photoStore.commitObservations([observation], authorization: oldAuthorization)
        let candidate = makeCandidate(
            authorization: oldAuthorization, observation: observation, selection: selection,
            configuration: config, deviceId: "restart-device")
        _ = try await photoStore.enqueueBackupCandidates(
            [candidate], authorization: oldAuthorization, maximumPending: 100)
        let claimResult = try await photoStore.claimNextBackupCandidate(
            authorization: oldAuthorization, deviceId: "restart-device", claimId: UUID(),
            leaseUntil: Date().addingTimeInterval(60), policyRevision: config.policyRevision)
        let claimed = try XCTUnwrap(claimResult)
        let uploadId = UploadOperationId()
        let uploading = claimed.replacing(
            state: .uploading, claimId: claimed.claimId,
            claimExpiresAt: claimed.claimExpiresAt,
            claimPreviousState: claimed.claimPreviousState,
            uploadOperationId: uploadId.rawValue, verifiedByteCount: 4,
            verifiedSHA256: digest(Data([1, 2, 3, 4])))
        try await photoStore.transitionBackupCandidate(
            uploading, expectedState: .claimed, authorization: oldAuthorization)

        let newAuthorization = PhotoAuthorizationSession(
            profileId: profileId, ownerId: "restart-owner", sessionRevision: 4)
        try await photoStore.activateAuthorization(newAuthorization)
        try await photoStore.adoptRecoverableBackupCandidates(
            authorization: newAuthorization, deviceId: "restart-device")

        let adoptedCandidates = try await photoStore.backupCandidates(
            authorization: newAuthorization, limit: 10)
        let adopted = try XCTUnwrap(adoptedCandidates.first)
        XCTAssertEqual(adopted.authorizationGeneration, newAuthorization.generation)
        XCTAssertEqual(adopted.state, .reconciliationPending)
        XCTAssertEqual(adopted.uploadOperationId, uploadId.rawValue)
        XCTAssertEqual(adopted.destinationLibraryIdentifier, "library-for-tests")

        let recoveredClaim = try await photoStore.claimNextBackupCandidate(
            authorization: newAuthorization, deviceId: "restart-device", claimId: UUID(),
            leaseUntil: Date().addingTimeInterval(60), policyRevision: config.policyRevision)
        XCTAssertEqual(recoveredClaim?.uploadOperationId, uploadId.rawValue)
    }

    func testResourceSelectionIdentitySurvivesSQLiteTimestampRoundTrip() {
        let sourceDate = Date()
        let restoredDate = Date(timeIntervalSince1970: sourceDate.timeIntervalSince1970)

        XCTAssertEqual(
            makeSelection(sourceModificationDate: sourceDate),
            makeSelection(sourceModificationDate: restoredDate))
    }

    func testPhotoKitChunksBecomeVerifiedPrivateStage() async throws {
        let fixture = try await queueFixture(self, initializeBase: false)
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "photo-owner-transfer")
        let selection = makeSelection()
        let bytes = Data((0..<220_123).map { UInt8(truncatingIfNeeded: $0 &* 29) })
        let chunks = [Data(bytes.prefix(65_536)), Data(bytes.dropFirst(65_536))]
        let source = FixturePhotoResourceSource(selection: selection, chunks: chunks)
        let context = PhotoTransferSessionContext(
            authorization: authorization, deviceId: "device-transfer",
            configuration: PhotoBackupConfiguration(isOptedIn: true))
        let contextProvider = FixedPhotoTransferContextProvider(context: context)
        let permission = FixedPhotoPermission(.authorizedLimited)
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(
            context.configuration, profileId: authorization.profileId)
        try await photoStore.commitObservations(
            [makeObservation(selection: selection, generation: 8)], authorization: authorization)

        let photoStage = try makePhotoStaging()
        let coordinator = PhotoResourceExtractionCoordinator(
            source: source, permissionAdapter: permission,
            contextProvider: contextProvider, store: photoStore, staging: photoStage,
            mutationStore: fixture.database, maximumResourceBytes: 1_000_000)
        let record = try await coordinator.prepareSelection(
            selection: selection, observationGeneration: 8)

        XCTAssertEqual(record.state, .verifiedReady)
        XCTAssertEqual(record.byteCount, UInt64(bytes.count))
        XCTAssertEqual(record.sha256, digest(bytes))
        let persisted = try await photoStore.extraction(
            operationId: record.operationId, authorization: authorization)
        XCTAssertEqual(persisted?.policyVersion, 1)
        XCTAssertEqual(persisted?.maximumResourceBytes, 1_000_000)
        XCTAssertEqual(persisted?.userInitiated, true)
        let staged = try await photoStage.verify(
            stagingId: record.stagingId, byteCount: UInt64(bytes.count),
            sha256: digest(bytes))
        XCTAssertEqual(try Data(contentsOf: staged.url), bytes)
        do {
            _ = try await coordinator.prepareSelection(
                selection: selection, observationGeneration: 8)
            XCTFail("An unresolved resource must retain its exact preparation identity")
        } catch {
            XCTAssertEqual(error as? PhotoResourceTransferFailure, .existingUnresolvedOperation)
        }
    }

    func testPhotoStagingFreeSpaceReservationFailsBeforeFileCreation() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(
            "photo_stage_reserve_\(UUID().uuidString)", isDirectory: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let shared = try DownloadStagingStore(
            root: root, freeBytes: { _ in DownloadPolicy.freeReserve - 1 })
        let stage = PhotoResourceStagingCoordinator(
            uploadStaging: UploadStagingStore(shared: shared))

        do {
            _ = try await stage.begin(
                stagingId: UploadOperationId(), maximumBytes: 1_024,
                durableReservations: [])
            XCTFail("The shared free-space reserve must be enforced before extraction")
        } catch {
            XCTAssertEqual(error as? PhotoResourceTransferFailure, .insufficientStorage)
        }
        XCTAssertTrue(try FileManager.default.contentsOfDirectory(atPath: root.path).isEmpty)
    }

    func testOldAuthorizationGenerationCannotReadOrRetainReadyStage() async throws {
        let photoStore = try makePhotoStore()
        let profileId = UUID()
        let first = PhotoAuthorizationSession(profileId: profileId, ownerId: "same-owner")
        let second = PhotoAuthorizationSession(profileId: profileId, ownerId: "same-owner")
        let selection = makeSelection()
        try await photoStore.activateAuthorization(first)
        try await photoStore.saveConfiguration(
            PhotoBackupConfiguration(isOptedIn: true), profileId: profileId)
        try await photoStore.commitObservations(
            [makeObservation(selection: selection, generation: 2)], authorization: first)
        let intent = PhotoResourceExtractionIntent(
            profileId: profileId, ownerId: first.ownerId, deviceId: "device",
            selection: selection, observationGeneration: 2, authorization: first,
            maximumResourceBytes: 100, userInitiated: true)
        var record = try await photoStore.createExtraction(
            intent, stagingId: UUID().uuidString.lowercased())
        record = record.replacing(state: .eligible)
        try await photoStore.transitionExtraction(
            record, expectedState: .discovered, authorization: first)
        record = record.replacing(state: .extracting)
        try await photoStore.transitionExtraction(
            record, expectedState: .eligible, authorization: first)
        record = record.replacing(state: .verifying)
        try await photoStore.transitionExtraction(
            record, expectedState: .extracting, authorization: first)
        record = record.replacing(
            state: .verifiedReady, byteCount: 1, sha256: digest(Data([0x7f])))
        try await photoStore.transitionExtraction(
            record, expectedState: .verifying, authorization: first)

        try await photoStore.activateAuthorization(second)

        let currentRecord = try await photoStore.extraction(
            operationId: record.operationId, authorization: second)
        let currentRecords = try await photoStore.extractions(authorization: second)
        let retainedStageIds = try await photoStore.extractionStagingIds()
        XCTAssertNil(currentRecord)
        XCTAssertTrue(currentRecords.isEmpty)
        XCTAssertTrue(retainedStageIds.isEmpty)
        do {
            try await photoStore.transitionExtraction(
                record.replacing(
                    state: .handoffPending,
                    relatedUploadOperationId: UUID().uuidString.lowercased()),
                expectedState: .verifiedReady, authorization: first)
            XCTFail("A replaced authorization must not advance the old resource")
        } catch {
            XCTAssertEqual(error as? PhotoResourceTransferFailure, .staleAuthorization)
        }
    }

    func testUnavailablePhotoKitSourcePreservesItsObservationAndDiscardsPartialBytes() async throws
    {
        let fixture = try await queueFixture(self, initializeBase: false)
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "unavailable-owner")
        let selection = makeSelection()
        let context = PhotoTransferSessionContext(
            authorization: authorization, deviceId: "unavailable-device",
            configuration: PhotoBackupConfiguration(isOptedIn: true))
        let observation = makeObservation(selection: selection, generation: 3)
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(
            context.configuration, profileId: authorization.profileId)
        try await photoStore.commitObservations([observation], authorization: authorization)
        let source = FixturePhotoResourceSource(
            selection: selection, chunks: [Data([0x01, 0x02])],
            failure: .sourceUnavailable)
        let stage = try makePhotoStaging()
        let coordinator = PhotoResourceExtractionCoordinator(
            source: source, permissionAdapter: FixedPhotoPermission(.authorizedFull),
            contextProvider: FixedPhotoTransferContextProvider(context: context),
            store: photoStore, staging: stage, mutationStore: fixture.database)

        do {
            _ = try await coordinator.prepareSelection(
                selection: selection, observationGeneration: 3)
            XCTFail("An unavailable local original must not become READY")
        } catch {
            XCTAssertEqual(error as? PhotoResourceTransferFailure, .sourceUnavailable)
        }
        let extractions = try await photoStore.extractions(authorization: authorization)
        let observations = try await photoStore.fetchObservations(
            profileId: authorization.profileId, ownerId: authorization.ownerId)
        let retainedStages = try await photoStore.extractionStagingIds()
        XCTAssertEqual(extractions.first?.state, .sourceUnavailable)
        XCTAssertEqual(observations, [observation])
        XCTAssertTrue(retainedStages.isEmpty)
    }

    func testLogoutQueuedBehindSQLiteCommitBecomesAnEffectiveWriteBarrier() async throws {
        let reachedCommit = DispatchSemaphore(value: 0)
        let allowCommit = DispatchSemaphore(value: 0)
        // Keep the transaction gated until the assertion observes the revocation barrier.
        // A wall-clock timeout here can expire while a loaded simulator is scheduling the test,
        // letting the commit finish before revocation starts and making the assertion flaky.
        defer { allowCommit.signal() }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(
            "photo_commit_fence_\(UUID().uuidString).sqlite")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        let photoStore = try PhotoObservationSQLiteStore(databaseURL: url) { point in
            guard point == .beforeObservationCommit else { return }
            reachedCommit.signal()
            allowCommit.wait()
        }
        let authorization = PhotoAuthorizationSession(profileId: UUID(), ownerId: "commit-owner")
        try await photoStore.activateAuthorization(authorization)
        let observation = PhotoAssetObservation(
            localIdentifier: "asset-during-commit", assetKind: .stillPhoto,
            creationDate: nil, modificationDate: nil, pixelWidth: 1, pixelHeight: 1,
            isFavorite: false, isHidden: false, discoveryGeneration: 1, observedAt: Date(),
            permissionScope: .authorizedFull, sourceAvailability: .available)
        let commit = Task.detached {
            try await photoStore.commitObservations([observation], authorization: authorization)
        }

        XCTAssertEqual(reachedCommit.wait(timeout: .now() + 3), .success)
        let revokeStarted = DispatchSemaphore(value: 0)
        let revoke = Task.detached {
            revokeStarted.signal()
            return photoStore.revokeCurrentAuthorizationSynchronously()
        }
        XCTAssertEqual(revokeStarted.wait(timeout: .now() + 3), .success)
        XCTAssertEqual(revokeDoneWait(revoke), .timedOut)
        allowCommit.signal()
        try await commit.value
        let barrierCompleted = await revoke.value
        XCTAssertTrue(barrierCompleted)

        do {
            try await photoStore.commitObservations([observation], authorization: authorization)
            XCTFail("Writes submitted after the synchronous revocation barrier must fail")
        } catch {
            XCTAssertEqual(error as? PhotoResourceTransferFailure, .staleAuthorization)
        }
        let committed = try await photoStore.fetchObservations(
            profileId: authorization.profileId, ownerId: authorization.ownerId)
        XCTAssertEqual(committed.count, 1)
    }

    func testLogoutRejectsAuthorizationActivationFromAnOlderSessionRevision() async throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(
            "photo_activation_fence_\(UUID().uuidString).sqlite")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        let photoStore = try PhotoObservationSQLiteStore(databaseURL: url)
        let profileId = UUID()
        let oldSession = PhotoAuthorizationSession(
            profileId: profileId, ownerId: "activation-owner", sessionRevision: 12)
        XCTAssertTrue(
            photoStore.revokeCurrentAuthorizationSynchronously(throughSessionRevision: 12))

        do {
            try await photoStore.activateAuthorization(oldSession)
            XCTFail(
                "A delayed observation start must not reactivate a session invalidated by logout")
        } catch {
            XCTAssertEqual(error as? PhotoResourceTransferFailure, .staleAuthorization)
        }

        let newSession = PhotoAuthorizationSession(
            profileId: profileId, ownerId: "activation-owner", sessionRevision: 13)
        try await photoStore.activateAuthorization(newSession)
        try await photoStore.commitObservations(
            [
                PhotoAssetObservation(
                    localIdentifier: "fresh-session-asset", assetKind: .stillPhoto,
                    creationDate: nil, modificationDate: nil, pixelWidth: 1, pixelHeight: 1,
                    isFavorite: false, isHidden: false, discoveryGeneration: 1, observedAt: Date(),
                    permissionScope: .authorizedFull, sourceAvailability: .available)
            ],
            authorization: newSession)
        let observations = try await photoStore.fetchObservations(
            profileId: profileId, ownerId: "activation-owner")
        XCTAssertEqual(observations.count, 1)
    }

    func testLogoutBetweenFinalHashAndP045HandoffDoesNotStartUpload() async throws {
        let fixture = try await queueFixture(self, initializeBase: false)
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "handoff-owner")
        let selection = makeSelection()
        let bytes = Data([1, 3, 3, 7])
        let source = FixturePhotoResourceSource(selection: selection, chunks: [bytes])
        let context = PhotoTransferSessionContext(
            authorization: authorization, deviceId: "handoff-device",
            configuration: PhotoBackupConfiguration(isOptedIn: true))
        let contextProvider = FixedPhotoTransferContextProvider(context: context)
        let permission = FixedPhotoPermission(.authorizedFull)
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(
            context.configuration, profileId: authorization.profileId)
        try await photoStore.commitObservations(
            [makeObservation(selection: selection, generation: 5)], authorization: authorization)
        let photoStage = try makePhotoStaging()
        let extraction = PhotoResourceExtractionCoordinator(
            source: source, permissionAdapter: permission,
            contextProvider: contextProvider, store: photoStore, staging: photoStage,
            mutationStore: fixture.database)
        let ready = try await extraction.prepareSelection(
            selection: selection, observationGeneration: 5)
        let uploadSpy = ForegroundUploadSpy()
        let handoff = PhotoResourceUploadHandoffCoordinator(
            source: source, permissionAdapter: permission, contextProvider: contextProvider,
            store: photoStore, staging: photoStage, uploads: uploadSpy,
            beforeUploadStart: {
                try? await photoStore.revokeAuthorization(authorization)
                contextProvider.replace(nil)
            })
        let library = try await LibraryId.validated(queueUUID(81), using: QueueValidator())
        let parent = try await NodeId.validated(queueUUID(82), using: QueueValidator())
        let result = await handoff.uploadSelected(
            extractionId: ready.operationId,
            target: .createFile(library: library, parent: parent, name: "still.jpg"),
            libraryStatus: .active,
            progress: { _ in })

        XCTAssertEqual(result.failure, .staleAuthorization)
        XCTAssertEqual(uploadSpy.startCount, 0)
    }

    func testExplicitHandoffUsesExactP045IntentAndReusesItsOperationId() async throws {
        let fixture = try await queueFixture(self, initializeBase: false)
        let photoStore = try makePhotoStore()
        let authorization = PhotoAuthorizationSession(
            profileId: UUID(), ownerId: "explicit-handoff-owner")
        let selection = makeSelection()
        let bytes = Data([0x43, 0x4f, 0x44, 0x45])
        let source = FixturePhotoResourceSource(selection: selection, chunks: [bytes])
        let context = PhotoTransferSessionContext(
            authorization: authorization, deviceId: "explicit-device",
            configuration: PhotoBackupConfiguration(isOptedIn: true))
        let contextProvider = FixedPhotoTransferContextProvider(context: context)
        let permission = FixedPhotoPermission(.authorizedFull)
        try await photoStore.activateAuthorization(authorization)
        try await photoStore.saveConfiguration(
            context.configuration, profileId: authorization.profileId)
        try await photoStore.commitObservations(
            [makeObservation(selection: selection, generation: 4)], authorization: authorization)
        let photoStage = try makePhotoStaging()
        let extraction = PhotoResourceExtractionCoordinator(
            source: source, permissionAdapter: permission,
            contextProvider: contextProvider, store: photoStore, staging: photoStage,
            mutationStore: fixture.database)
        let ready = try await extraction.prepareSelection(
            selection: selection, observationGeneration: 4)
        let uploadSpy = ForegroundUploadSpy()
        let handoff = PhotoResourceUploadHandoffCoordinator(
            source: source, permissionAdapter: permission, contextProvider: contextProvider,
            store: photoStore, staging: photoStage, uploads: uploadSpy)
        let library = try await LibraryId.validated(queueUUID(83), using: QueueValidator())
        let parent = try await NodeId.validated(queueUUID(84), using: QueueValidator())
        let target = UploadTarget.createFile(library: library, parent: parent, name: "selected.jpg")

        let first = await handoff.uploadSelected(
            extractionId: ready.operationId, target: target, libraryStatus: .active,
            progress: { _ in })
        let expected = try UploadExpectedContent(
            byteLength: UInt64(bytes.count), sha256: digest(bytes))
        let pending = try await photoStore.extraction(
            operationId: ready.operationId, authorization: authorization)
        let second = await handoff.uploadSelected(
            extractionId: ready.operationId, target: target, libraryStatus: .active,
            progress: { _ in })

        XCTAssertEqual(first.failure, nil)
        XCTAssertEqual(second.failure, nil)
        XCTAssertEqual(uploadSpy.startCount, 2)
        XCTAssertEqual(uploadSpy.targets, [target, target])
        XCTAssertEqual(uploadSpy.expectedContents, [expected, expected])
        XCTAssertEqual(uploadSpy.backgroundPolicies, [false, false])
        XCTAssertEqual(uploadSpy.operationIds.count, 2)
        XCTAssertEqual(uploadSpy.operationIds[0], uploadSpy.operationIds[1])
        XCTAssertEqual(pending?.state, .handoffPending)
    }

    func testPhotoDiscoveryContinuationVisitsMoreThanOneThousandAssetsExactlyOnce() async throws {
        let photoStore = try makePhotoStore()
        let profileId = UUID()
        let ownerId = "paged-owner"
        try await photoStore.saveConfiguration(
            PhotoBackupConfiguration(isOptedIn: true), profileId: profileId)
        let service = PagedPhotoDiscovery()
        let coordinator = PhotoLibraryObservationCoordinator(
            discoveryService: service, store: photoStore,
            permissionAdapter: FixedPhotoPermission(.authorizedFull))

        let result = try await coordinator.startObservation(profileId: profileId, ownerId: ownerId)

        XCTAssertEqual(result, .completed(totalObserved: 1_001, generation: 1))
        let pageCount = await service.pageCount
        XCTAssertEqual(pageCount, 2)
        let saved = try await photoStore.fetchObservations(profileId: profileId, ownerId: ownerId)
        XCTAssertEqual(saved.count, 1_001)
        XCTAssertEqual(Set(saved.map(\.localIdentifier)).count, 1_001)
    }

    private func makePhotoStore() throws -> PhotoObservationSQLiteStore {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(
            "photo_transfer_\(UUID().uuidString).sqlite")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return try PhotoObservationSQLiteStore(databaseURL: url)
    }

    private func makePhotoStaging() throws -> PhotoResourceStagingCoordinator {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(
            "photo_stage_\(UUID().uuidString)", isDirectory: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let shared = try DownloadStagingStore(root: root, freeBytes: { _ in 2_000_000_000 })
        return PhotoResourceStagingCoordinator(uploadStaging: UploadStagingStore(shared: shared))
    }

    private func makeSelection(
        sourceModificationDate: Date? = Date(timeIntervalSince1970: 10)
    ) -> PhotoResourceSelection {
        PhotoResourceSelection(
            localIdentifier: "device-asset-1", resourceIndex: 0, resourceType: "1",
            role: .primaryImage, rendition: .original,
            uniformTypeIdentifier: "public.jpeg",
            sourceModificationDate: sourceModificationDate)
    }

    private func makeObservation(
        selection: PhotoResourceSelection, generation: Int64
    ) -> PhotoAssetObservation {
        PhotoAssetObservation(
            localIdentifier: selection.localIdentifier, assetKind: .stillPhoto,
            creationDate: nil, modificationDate: selection.sourceModificationDate,
            pixelWidth: 20, pixelHeight: 20, isFavorite: false, isHidden: false,
            discoveryGeneration: generation, observedAt: Date(),
            permissionScope: .authorizedFull, sourceAvailability: .available)
    }

    private func activeAutomaticConfiguration() -> PhotoBackupConfiguration {
        PhotoBackupConfiguration(
            targetLibraryIdentifier: "library-for-tests",
            targetParentIdentifier: "parent-for-tests",
            automaticForegroundBackupEnabled: true,
            automaticBackupConsentConfirmed: true,
            discoveryEnabled: true)
    }

    private func makeCandidate(
        authorization: PhotoAuthorizationSession,
        observation: PhotoAssetObservation,
        selection: PhotoResourceSelection,
        configuration: PhotoBackupConfiguration,
        deviceId: String
    ) -> PhotoBackupCandidate {
        PhotoBackupCandidate(
            profileId: authorization.profileId, ownerId: authorization.ownerId,
            deviceId: deviceId, clientLibraryScope: observation.permissionScope.rawValue,
            localIdentifier: observation.localIdentifier,
            sourceRevision: PhotoBackupCandidate.sourceRevision(for: observation),
            selection: selection, observationGeneration: observation.discoveryGeneration,
            policyRevision: configuration.policyRevision,
            authorizationGeneration: authorization.generation,
            destinationLibraryIdentifier: configuration.targetLibraryIdentifier ?? "",
            destinationParentIdentifier: configuration.targetParentIdentifier ?? "")
    }

    private func digest(_ data: Data) -> String {
        "sha256:" + SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    private func revokeDoneWait(_ task: Task<Bool, Never>) -> DispatchTimeoutResult {
        let finished = DispatchSemaphore(value: 0)
        Task.detached {
            _ = await task.value
            finished.signal()
        }
        return finished.wait(timeout: .now() + 0.1)
    }
}

private struct FixedPhotoPermission: PhotoLibraryPermissionChecking {
    let permission: PhotoPermissionState
    init(_ permission: PhotoPermissionState) { self.permission = permission }
    func currentPermissionState() async -> PhotoPermissionState { permission }
    func requestPermission() async -> PhotoPermissionState { permission }
}

private struct FixturePhotoResourceSource: PhotoResourceExtractionSource {
    let selection: PhotoResourceSelection
    let chunks: [Data]
    let failure: PhotoResourceTransferFailure?

    init(
        selection: PhotoResourceSelection,
        chunks: [Data],
        failure: PhotoResourceTransferFailure? = nil
    ) {
        self.selection = selection
        self.chunks = chunks
        self.failure = failure
    }

    func selections(
        for localIdentifier: String, includingEditedRenditions: Bool
    ) async throws -> [PhotoResourceSelection] {
        localIdentifier == selection.localIdentifier ? [selection] : []
    }

    func currentModificationDate(for localIdentifier: String) async throws -> Date? {
        selection.sourceModificationDate
    }

    func extract(
        _ selected: PhotoResourceSelection, maximumBytes: UInt64,
        receive: @escaping @Sendable (Data) throws -> Void
    ) async throws -> UInt64 {
        var total: UInt64 = 0
        for chunk in chunks {
            try Task.checkCancellation()
            try receive(chunk)
            total += UInt64(chunk.count)
        }
        if let failure { throw failure }
        return total
    }
}

@MainActor
private final class FixedPhotoTransferContextProvider: PhotoTransferContextProviding {
    private var context: PhotoTransferSessionContext?
    init(context: PhotoTransferSessionContext?) { self.context = context }
    func currentContext() async -> PhotoTransferSessionContext? { context }
    func replace(_ context: PhotoTransferSessionContext?) { self.context = context }
}

@MainActor
private final class ForegroundUploadSpy: PhotoForegroundUploadCoordinatorProtocol {
    private(set) var startCount = 0
    var hasActiveForegroundOperation = false
    private(set) var targets: [UploadTarget] = []
    private(set) var expectedContents: [UploadExpectedContent] = []
    private(set) var backgroundPolicies: [Bool] = []
    private(set) var operationIds: [UploadOperationId] = []
    func start(
        selectedURL: URL, target: UploadTarget?, library: LibraryId, parent: NodeId?,
        libraryStatus: LibraryStatus, backgroundEnabled: Bool,
        stableOperationId: UploadOperationId?, expectedSourceContent: UploadExpectedContent?,
        progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> FileUploadResult {
        guard let target, let expectedSourceContent, let stableOperationId else {
            return .stopped(nil, .invalidMetadata)
        }
        startCount += 1
        targets.append(target)
        expectedContents.append(expectedSourceContent)
        backgroundPolicies.append(backgroundEnabled)
        operationIds.append(stableOperationId)
        return .stopped(nil, .cancelled)
    }
    func resume(
        id: UploadOperationId, library: LibraryId, libraryStatus: LibraryStatus,
        backgroundEnabled: Bool, progress: @escaping @MainActor (UploadProgress) -> Void
    ) async -> FileUploadResult { .stopped(nil, .cancelled) }
    func reconcileExisting(
        id: UploadOperationId, library: LibraryId
    ) async -> Result<UploadRecord, UploadFailure> { .failure(.invalidMetadata) }
    func pauseUserTransfer() async throws {}
    func pauseUserTransfer(id: UploadOperationId) async throws {}
}

private actor PagedPhotoDiscovery: PhotoLibraryDiscoveryProtocol {
    private(set) var pageCount = 0
    private let snapshotId = UUID()

    func discoverAssets(
        configuration: PhotoBackupConfiguration, generation: Int64, batchSize: Int
    ) async throws -> PhotoDiscoveryBatchResult {
        try await discoverAssetPage(
            configuration: configuration, generation: generation,
            batchSize: batchSize, continuation: nil)
    }

    func discoverAssetPage(
        configuration: PhotoBackupConfiguration, generation: Int64, batchSize: Int,
        continuation: PhotoDiscoveryContinuation?
    ) async throws -> PhotoDiscoveryBatchResult {
        pageCount += 1
        let start = continuation?.nextOffset ?? 0
        let count = min(1_001 - start, 1_000)
        let observations = (start..<(start + count)).map { index in
            PhotoAssetObservation(
                localIdentifier: "paged-asset-\(index)", assetKind: .stillPhoto,
                creationDate: nil, modificationDate: nil, pixelWidth: 1, pixelHeight: 1,
                isFavorite: false, isHidden: false, discoveryGeneration: generation,
                observedAt: Date(), permissionScope: .authorizedFull,
                sourceAvailability: .available)
        }
        let isComplete = start + count == 1_001
        return PhotoDiscoveryBatchResult(
            observations: observations, isComplete: isComplete, totalInScope: 1_001,
            continuation: isComplete
                ? nil
                : PhotoDiscoveryContinuation(
                    generation: generation, snapshotId: snapshotId,
                    nextOffset: start + count, totalInScope: 1_001))
    }

    func discoverResourceDescriptors(
        for localIdentifier: String
    ) async throws -> [PhotoAssetResourceDescriptor] { [] }
}

private extension Result where Failure == PhotoResourceTransferFailure {
    var failure: Failure? {
        guard case .failure(let error) = self else { return nil }
        return error
    }
}

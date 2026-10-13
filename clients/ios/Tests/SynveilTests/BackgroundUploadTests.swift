import CryptoKit
import Foundation
import SwiftUI
import UIKit
import XCTest

@testable import Synveil

@MainActor
final class BackgroundUploadTests: XCTestCase {
    func testTerminalStatesCannotScheduleOrResume() {
        for state in [BackgroundUploadState.quarantined, .failedTerminal, .aborted] {
            for next in BackgroundUploadState.allCases where next != state && next != .quarantined {
                XCTAssertFalse(state.permits(next, explicitResume: true), "\(state) -> \(next)")
            }
        }
    }

    func testPauseRequiresExplicitResume() {
        XCTAssertFalse(BackgroundUploadState.paused.permits(.preparing))
        XCTAssertFalse(BackgroundUploadState.paused.permits(.reconciling))
        XCTAssertTrue(BackgroundUploadState.paused.permits(.preparing, explicitResume: true))
        XCTAssertFalse(BackgroundUploadState.foregroundOnly.permits(.preparing))
    }

    func testLegalChunkLifecycleAndIllegalReplay() throws {
        var job = BackgroundUploadJob(
            operationId: UploadOperationId().rawValue, generation: 0,
            authorized: true, state: .preparing, chunk: nil, resolved: true,
            taskIdentifier: nil, evidence: nil)
        for state in [
            BackgroundUploadState.queued, .scheduled, .transferring, .awaitingEvidence,
            .reconciling, .completedChunk, .preparing,
        ] {
            try job.transition(state)
        }
        XCTAssertThrowsError(try job.transition(.scheduled))
        let asyncValue1 = BackgroundUploadState.awaitingEvidence.permits(.preparing)
        XCTAssertFalse(asyncValue1)
        XCTAssertFalse(BackgroundUploadState.scheduled.permits(.completedChunk))
    }

    func testFutureOrUnknownPersistedStateFailsClosed() throws {
        let json = Data(
            "{\"operationId\":\"\(UploadOperationId().rawValue)\",\"generation\":1,\"authorized\":true,\"state\":\"future\",\"resolved\":true}"
                .utf8)
        XCTAssertThrowsError(try JSONDecoder().decode(BackgroundUploadJob.self, from: json))
    }

    func test204RequiresExactCanonicalOffsetAndRequestId() throws {
        let chunk = chunkIntent(length: 4)
        XCTAssertEqual(try evidence(offset: "4").acceptedOffset(chunk: chunk, expected: 4), 4)
        for offset in ["04", "3", "5", "-1", "4,4", ""] {
            XCTAssertThrowsError(
                try evidence(offset: offset).acceptedOffset(chunk: chunk, expected: 4))
        }
        XCTAssertThrowsError(
            try evidence(offset: "4", requestId: "bad\nrequest").acceptedOffset(
                chunk: chunk, expected: 4))
        XCTAssertThrowsError(
            try evidence(offset: "4", status: 200).acceptedOffset(chunk: chunk, expected: 4))
        XCTAssertThrowsError(
            try evidence(offset: "4", bodyEmpty: false).acceptedOffset(chunk: chunk, expected: 4))
        XCTAssertThrowsError(
            try evidence(offset: "4", failure: .timeout).acceptedOffset(chunk: chunk, expected: 4))
    }

    func testExplicitOptInSchedulesFileAndNeverCompletes() async throws {
        let f = try await fixture()
        let record = try await start(f)
        XCTAssertEqual(f.transport.submissions.count, 1)
        XCTAssertEqual(f.transport.resumed.count, 1)
        XCTAssertEqual(f.provider.completionCalls, 0)
        XCTAssertEqual(record.authoritativeOffset, 0)
        let jobValue = try await f.queue.database.backgroundJob(record.id)
        let job = try XCTUnwrap(jobValue)
        XCTAssertTrue(job.authorized)
        XCTAssertTrue(job.blocksForeground)
        XCTAssertEqual(job.state, .scheduled)
        let file = try XCTUnwrap(f.transport.files.first)
        XCTAssertEqual(try Data(contentsOf: file), f.bytes)
        XCTAssertEqual(job.chunk?.sha256, digest(f.bytes))
        XCTAssertEqual(f.transport.submissions.first?.body, nil)
        XCTAssertEqual(f.transport.submissions.first?.method, .patch)
    }

    func testDisabledAuthorizationNeverSubmitsBackgroundTask() async throws {
        let f = try await fixture()
        _ = await f.coordinator.start(
            selectedURL: f.source,
            target: f.target, library: f.queue.scope.libraryId, libraryStatus: .active,
            progress: { _ in })
        XCTAssertTrue(f.transport.submissions.isEmpty)
        XCTAssertEqual(f.provider.completionCalls, 1)
    }

    func testUnresolvedIntentBlocksForegroundMutationAcrossReopen() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let blocked = try await reopened.backgroundBlocks(record.id)
        XCTAssertTrue(blocked)
        do {
            try await reopened.updateUpload(record, state: .uploading, uncertain: true)
            XCTFail("Foreground must not overlap a system PATCH")
        } catch { XCTAssertEqual(error as? UploadFailure, .duplicateOperation) }
        let restored = try await reopened.backgroundJob(record.id)
        XCTAssertEqual(restored?.chunk?.offset, 0)
        XCTAssertEqual(restored?.taskIdentifier, f.transport.tasks.first?.identifier)
    }

    func testSuccessfulCallbackRequiresGETThenAwaitsExplicitFinalization() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let task = try XCTUnwrap(f.transport.tasks.first)
        f.provider.acceptBackground(to: UInt64(f.bytes.count))
        f.transport.tasks = []
        let accepted = await f.background.receive(
            task, evidence: evidence(offset: String(f.bytes.count)))
        XCTAssertTrue(accepted)
        await f.background.waitForRecoveryAction()
        let updated = try await read(f, record.id)
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(updated.authoritativeOffset, UInt64(f.bytes.count))
        XCTAssertEqual(updated.state, .readyToComplete)
        XCTAssertEqual(job?.state, .awaitingFinalization)
        XCTAssertEqual(f.provider.statusCount, 1)
        XCTAssertEqual(f.provider.completionCalls, 0)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testPauseKeepsIdentityAndLateEvidenceCannotResume() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let task = try XCTUnwrap(f.transport.tasks.first)
        try await f.background.pause(record.id)
        let paused = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(paused?.state, .paused)
        XCTAssertEqual(paused?.authorized, false)
        let asyncValue2 = await f.background.receive(
            task, evidence: evidence(offset: String(f.bytes.count)))
        XCTAssertTrue(asyncValue2)
        await f.background.waitForRecoveryAction()
        let saved = try await read(f, record.id)
        XCTAssertEqual(saved.authoritativeOffset, 0)
        XCTAssertEqual(saved.serverSessionId, record.serverSessionId)
        XCTAssertEqual(f.provider.statusCount, 0)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testLostSuccessfulResponseRecoversOriginalIdentity() async throws {
        let f = try await fixture()
        let record = try await start(f)
        f.provider.acceptBackground(to: UInt64(f.bytes.count))
        f.transport.tasks = []
        let recovered = BackgroundUploadCoordinator(
            repository: f.repository,
            database: try MutationQueueSQLiteStore(url: f.queue.url), staging: f.staging,
            transport: f.transport)
        await recovered.restore()
        let saved = try await read(f, record.id)
        XCTAssertEqual(saved.authoritativeOffset, UInt64(f.bytes.count))
        XCTAssertEqual(saved.intent.idempotencyKey, record.intent.idempotencyKey)
        XCTAssertEqual(f.provider.createBodies.count, 1)
        XCTAssertEqual(f.provider.completionCalls, 0)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testPartialAcceptanceUsesGETOffsetForNextChunk() async throws {
        let f = try await fixture()
        let record = try await start(f)
        f.provider.acceptBackground(to: 123)
        let task = try XCTUnwrap(f.transport.tasks.first)
        f.transport.tasks = []
        let persisted = await f.background.receive(
            task, evidence: evidence(offset: "0", failure: .timeout))
        XCTAssertTrue(persisted)
        await f.background.waitForRecoveryAction()
        let saved = try await read(f, record.id)
        XCTAssertEqual(saved.authoritativeOffset, 123)
        XCTAssertEqual(f.transport.submissions.count, 2)
        XCTAssertEqual(f.transport.submissions.last?.headers["Upload-Offset"], "123")
        let nextFile = try XCTUnwrap(f.transport.files.last)
        XCTAssertEqual(try Data(contentsOf: nextFile), f.bytes.subdata(in: 123..<f.bytes.count))
        XCTAssertEqual(f.provider.completionCalls, 0)
    }

    func testZeroProgressRecoveryDoesNotRetryPATCH() async throws {
        let f = try await fixture()
        let record = try await start(f)
        f.transport.tasks = []
        await f.background.restore()
        await f.background.restore()
        XCTAssertEqual(f.transport.submissions.count, 1)
        let saved = try await read(f, record.id)
        XCTAssertEqual(saved.authoritativeOffset, 0)
    }

    func testMalformed204CanOnlyRecoverThroughGET() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let task = try XCTUnwrap(f.transport.tasks.first)
        f.provider.acceptBackground(to: UInt64(f.bytes.count))
        f.transport.tasks = []
        let asyncValue3 = await f.background.receive(task, evidence: evidence(offset: "999999"))
        XCTAssertTrue(asyncValue3)
        await f.background.waitForRecoveryAction()
        let saved = try await read(f, record.id)
        XCTAssertEqual(saved.authoritativeOffset, UInt64(f.bytes.count))
        XCTAssertEqual(f.provider.statusCount, 1)
    }

    func testValid204ConflictingWithStatusCannotAdvance() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let task = try XCTUnwrap(f.transport.tasks.first)
        f.provider.acceptBackground(to: 10)
        f.transport.tasks = []
        let asyncValue4 = await f.background.receive(
            task, evidence: evidence(offset: String(f.bytes.count)))
        XCTAssertTrue(asyncValue4)
        await f.background.waitForRecoveryAction()
        let saved = try await read(f, record.id)
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(saved.authoritativeOffset, 0)
        XCTAssertTrue(job?.blocksForeground == true)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testDuplicateCallbackIsIdempotent() async throws {
        let f = try await fixture()
        let record = try await start(f)
        try await f.background.pause(record.id)
        let task = try XCTUnwrap(f.transport.originalTask)
        let e = evidence(offset: "20")
        let asyncValue5 = await f.background.receive(task, evidence: e)
        XCTAssertTrue(asyncValue5)
        let asyncValue6 = await f.background.receive(task, evidence: e)
        XCTAssertTrue(asyncValue6)
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.evidence, e)
        XCTAssertEqual(job?.state, .paused)
    }

    func testDifferentDuplicateEvidenceQuarantines() async throws {
        let f = try await fixture()
        let record = try await start(f)
        try await f.background.pause(record.id)
        let task = try XCTUnwrap(f.transport.originalTask)
        let asyncValue7 = await f.background.receive(task, evidence: evidence(offset: "20"))
        XCTAssertTrue(asyncValue7)
        let asyncValue8 = await f.background.receive(task, evidence: evidence(offset: "21"))
        XCTAssertTrue(asyncValue8)
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .quarantined)
    }

    func testUnknownTaskIsCancelledOnRestore() async throws {
        let f = try await fixture()
        f.transport.tasks = [
            BackgroundSystemTask(identifier: 44, intentId: UploadOperationId().rawValue)
        ]
        await f.background.restore()
        XCTAssertEqual(f.transport.cancelled, [44])
        XCTAssertTrue(f.provider.createBodies.isEmpty)
    }

    func testDuplicateSystemMappingQuarantinesWithoutPATCH() async throws {
        let f = try await fixture()
        let record = try await start(f)
        var duplicate = try XCTUnwrap(f.transport.tasks.first)
        duplicate = BackgroundSystemTask(
            identifier: 2, intentId: duplicate.intentId,
            requestURL: duplicate.requestURL, method: duplicate.method,
            offset: duplicate.offset, length: duplicate.length)
        f.transport.tasks.append(duplicate)
        await f.background.restore()
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .quarantined)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testCopiedIntentOnWrongOriginCannotBeAdopted() async throws {
        let f = try await fixture()
        let record = try await start(f)
        f.transport.tasks[0].requestURL = URL(
            string: "https://other.example/api/v1/upload-sessions/\(record.serverSessionId!)")
        await f.background.restore()
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .quarantined)
    }

    func testMissingBindingEnumeratesAndAdoptsExistingTaskOnce() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let originalValue = try await f.queue.database.backgroundJob(record.id)
        var queued = try XCTUnwrap(originalValue)
        queued.state = .queued
        queued.taskIdentifier = nil
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        let hex = try encoder.encode(queued).map { String(format: "%02x", $0) }.joined()
        try queueRawSQL(
            f.queue.url,
            "UPDATE background_upload_jobs SET metadata=X'\(hex)' WHERE operation_id='\(record.id.rawValue)'"
        )
        await f.background.restore()
        await f.background.restore()
        let mapped = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(mapped?.taskIdentifier, 1)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testCASRejectsStaleGenerationAndPausedSnapshot() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let priorValue = try await f.queue.database.backgroundJob(record.id)
        let prior = try XCTUnwrap(priorValue)
        try await f.background.pause(record.id)
        var stale = prior
        stale.state = .awaitingEvidence
        do {
            try await f.queue.database.saveBackgroundJob(stale, replacing: prior)
            XCTFail("Stale callback must fail CAS")
        } catch { XCTAssertEqual(error as? UploadFailure, .duplicateOperation) }
        stale.generation += 1
        do {
            try await f.queue.database.saveBackgroundJob(stale, replacing: prior)
            XCTFail("Unresolved generations cannot advance")
        } catch { XCTAssertEqual(error as? UploadFailure, .invalidMetadata) }
    }

    func testLogoutFencesDelayedCallbackAndRetainsChunk() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let task = try XCTUnwrap(f.transport.tasks.first)
        f.background.invalidateSession()
        try await f.queue.database.quarantineSessions()
        await f.background.waitForInvalidation()
        let asyncValue9 = await f.background.receive(
            task, evidence: evidence(offset: String(f.bytes.count)))
        XCTAssertTrue(asyncValue9)
        await f.background.waitForRecoveryAction()
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .quarantined)
        XCTAssertEqual(f.provider.statusCount, 0)
        XCTAssertTrue(FileManager.default.fileExists(atPath: f.transport.files[0].path))
    }

    func testInterruptedGETPreservesRangeAndNoRetry() async throws {
        let f = try await fixture()
        let record = try await start(f)
        f.provider.statusFailure = .timeout
        f.transport.tasks = []
        await f.background.restore()
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .reconciling)
        XCTAssertTrue(job?.blocksForeground == true)
        XCTAssertEqual(job?.chunk?.length, f.bytes.count)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testAuthenticationFailureNeverContinues() async throws {
        let f = try await fixture()
        let record = try await start(f)
        f.provider.statusFailure = .unauthenticated
        f.transport.tasks = []
        await f.background.restore()
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .quarantined)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testLifecycleAcknowledgementWaitsForDurableEvidenceExactlyOnce() async {
        let delivery = BackgroundUploadEventDelivery()
        let gate = QueueGate()
        var count = 0
        delivery.handler = { _, _ in
            await gate.arrive()
            return true
        }
        delivery.installCompletion { count += 1 }
        delivery.receive(
            BackgroundSystemTask(identifier: 1, intentId: nil), evidence: evidence(offset: "1"))
        delivery.finishEvents()
        await gate.wait()
        XCTAssertEqual(count, 0)
        await gate.release()
        await delivery.waitForLocalPersistence()
        XCTAssertEqual(count, 1)
        delivery.finishEvents()
        XCTAssertEqual(count, 1)
    }

    func testFailedCallbackPersistenceWithholdsSystemAcknowledgement() async {
        let delivery = BackgroundUploadEventDelivery()
        var count = 0
        delivery.handler = { _, _ in false }
        delivery.installCompletion { count += 1 }
        delivery.receive(
            BackgroundSystemTask(identifier: 1, intentId: nil), evidence: evidence(offset: "1"))
        delivery.finishEvents()
        await delivery.waitForLocalPersistence()
        XCTAssertEqual(count, 0)
        delivery.handler = { _, _ in true }
        delivery.processPendingEvents()
        await delivery.waitForLocalPersistence()
        XCTAssertEqual(count, 1)
    }

    func testV7MigrationPreservesUploadAndMutationEvidence() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let mutation = try await queueEnqueued(f.queue)
        try queueRawSQL(f.queue.url, "DROP TABLE background_upload_orphans")
        try queueRawSQL(f.queue.url, "DROP TABLE background_upload_jobs")
        try queueRawSQL(f.queue.url, "PRAGMA user_version=7")
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        XCTAssertEqual(try queueRawScalar(f.queue.url, "PRAGMA user_version"), "8")
        let rows = try await reopened.records(scope: f.queue.scope, limit: 10)
        XCTAssertEqual(rows.first?.request, mutation.mutation.requestBody)
        let saved = try await reopened.upload(
            scope: record.scope, credentialId: record.credentialId,
            id: record.id, bridge: QueueValidator())
        XCTAssertEqual(saved?.intent, record.intent)
        let asyncValue10 = try await reopened.backgroundInventory().isEmpty
        XCTAssertTrue(asyncValue10)
    }

    func testIntentBeforeCommitFailureCannotSubmitTask() async throws {
        let injector = QueueFaultInjector()
        let f = try await fixture(fault: injector)
        let record = try await createPaused(f)
        let session = try await f.provider.begin()
        injector.arm(.beforeCommit)
        do {
            try await f.background.schedule(record, session: session, explicitAuthorization: true)
            XCTFail("Expected failure")
        } catch {}
        XCTAssertTrue(f.transport.submissions.isEmpty)
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let asyncValue11 = try await reopened.backgroundJob(record.id)
        XCTAssertEqual(asyncValue11?.state, .paused)
    }

    func testIntentAfterCommitFailureRetainsDurableStateWithoutSubmission() async throws {
        let injector = QueueFaultInjector()
        let f = try await fixture(fault: injector)
        let record = try await createPaused(f)
        let session = try await f.provider.begin()
        injector.arm(.afterCommit)
        do {
            try await f.background.schedule(record, session: session, explicitAuthorization: true)
        } catch {}
        XCTAssertTrue(f.transport.submissions.isEmpty)
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let job = try await reopened.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .preparing)
    }

    func testBindingFailureLeavesOneSuspendedTaskAndRecoverableIntent() async throws {
        let injector = QueueFaultInjector()
        let f = try await fixture(fault: injector)
        f.transport.afterSubmit = { injector.arm(.beforeCommit) }
        _ = await f.coordinator.start(
            selectedURL: f.source, target: f.target,
            library: f.queue.scope.libraryId, libraryStatus: .active, backgroundEnabled: true,
            progress: { _ in })
        XCTAssertEqual(f.transport.submissions.count, 1)
        XCTAssertTrue(f.transport.resumed.isEmpty)
        f.transport.afterSubmit = nil
        await f.background.restore()
        XCTAssertEqual(f.transport.submissions.count, 1)
        XCTAssertEqual(f.transport.resumed, [1])
    }

    func testCallbackFailureKeepsOffsetAndCanPersistAfterReopen() async throws {
        let injector = QueueFaultInjector()
        let f = try await fixture(fault: injector)
        let record = try await start(f)
        try await f.background.pause(record.id)
        let task = try XCTUnwrap(f.transport.originalTask)
        injector.arm(.beforeCommit)
        let asyncValue12 = await f.background.receive(task, evidence: evidence(offset: "2"))
        XCTAssertFalse(asyncValue12)
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let job = try await reopened.backgroundJob(record.id)
        XCTAssertNil(job?.evidence)
        XCTAssertTrue(job?.blocksForeground == true)
        let asyncValue13 = await f.background.receive(task, evidence: evidence(offset: "2"))
        XCTAssertTrue(asyncValue13)
    }

    func testBackgroundPayloadSymlinkIsRejected() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let jobValue = try await f.queue.database.backgroundJob(record.id)
        let chunk = try XCTUnwrap(jobValue?.chunk)
        let file = try XCTUnwrap(f.transport.files.first)
        try FileManager.default.removeItem(at: file)
        try FileManager.default.createSymbolicLink(at: file, withDestinationURL: f.source)
        do {
            _ = try await f.staging.backgroundChunkURL(chunk)
            XCTFail("Symlink must fail")
        } catch {}
    }

    func testPayloadMutationFailsDigest() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let jobValue = try await f.queue.database.backgroundJob(record.id)
        let chunk = try XCTUnwrap(jobValue?.chunk)
        let file = try XCTUnwrap(f.transport.files.first)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
        try Data(repeating: 0, count: chunk.length).write(to: file)
        do {
            _ = try await f.staging.backgroundChunkURL(chunk)
            XCTFail("Digest mismatch must fail")
        } catch {
            XCTAssertEqual(error as? UploadFailure, .integrityMismatch)
        }
    }

    func testBackgroundPayloadCountsAgainstSharedQuota() async throws {
        let f = try await fixture()
        let record = try await createPaused(f)
        let huge = DownloadStorageReservation(
            operationId: DownloadOperationId(), bytes: DownloadPolicy.quota)
        do {
            _ = try await f.staging.prepareBackgroundChunk(
                record, id: UploadOperationId(),
                offset: 0, count: f.bytes.count, durable: [huge])
            XCTFail("Shared quota must include background chunks")
        } catch { XCTAssertEqual(UploadPolicy.classify(error), .stagingQuotaExceeded) }
    }

    func testBackgroundUIRendersSmallDisplayAndAccessibilityText() async throws {
        let f = try await fixture()
        _ = try await start(f)
        let host = UIHostingController(
            rootView: List {
                FileUploadControls(
                    coordinator: f.coordinator, controller: f.queue.controller,
                    library: f.queue.scope.libraryId, libraryStatus: .active)
            }.environment(\.dynamicTypeSize, .accessibility5))
        host.loadViewIfNeeded()
        host.view.frame = CGRect(x: 0, y: 0, width: 320, height: 640)
        host.view.layoutIfNeeded()
        XCTAssertEqual(host.view.bounds.width, 320)
        XCTAssertTrue(BackgroundUploadState.scheduled.label.contains("system"))
        let asyncValue14 = BackgroundUploadState.awaitingFinalization.label.contains("Complete")
        XCTAssertTrue(asyncValue14)
    }

    func testActivityIsInformationalAndCannotAdvanceOffset() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let task = try XCTUnwrap(f.transport.tasks.first)
        let persisted = await f.background.recordActivity(task)
        XCTAssertTrue(persisted)
        let job = try await f.queue.database.backgroundJob(record.id)
        let saved = try await read(f, record.id)
        XCTAssertEqual(job?.state, .transferring)
        XCTAssertEqual(saved.authoritativeOffset, 0)
        XCTAssertNil(job?.evidence)
    }

    func testAcceptedOffsetBeforeCommitFailureKeepsExclusionAfterReopen() async throws {
        let injector = QueueFaultInjector()
        let f = try await fixture(fault: injector)
        let record = try await start(f)
        f.transport.tasks = []
        f.provider.acceptBackground(to: UInt64(f.bytes.count))
        f.provider.onStatusResponse = { injector.arm(.beforeCommit) }
        await f.background.restore()
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let saved = try await read(f, record.id)
        let job = try await reopened.backgroundJob(record.id)
        XCTAssertEqual(saved.authoritativeOffset, 0)
        XCTAssertTrue(job?.blocksForeground == true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: f.transport.files[0].path))
        f.provider.onStatusResponse = nil
        await f.background.restore()
        let recovered = try await read(f, record.id)
        XCTAssertEqual(recovered.authoritativeOffset, UInt64(f.bytes.count))
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testAcceptedOffsetAfterCommitFailureIsAtomicAndCleanupRecoverable() async throws {
        let injector = QueueFaultInjector()
        let f = try await fixture(fault: injector)
        let record = try await start(f)
        f.transport.tasks = []
        f.provider.acceptBackground(to: UInt64(f.bytes.count))
        f.provider.onStatusResponse = { injector.arm(.afterCommit) }
        await f.background.restore()
        let reopened = try MutationQueueSQLiteStore(url: f.queue.url)
        let job = try await reopened.backgroundJob(record.id)
        let saved = try await read(f, record.id)
        XCTAssertEqual(saved.authoritativeOffset, UInt64(f.bytes.count))
        XCTAssertFalse(job?.blocksForeground == true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: f.transport.files[0].path))
        f.provider.onStatusResponse = nil
        await f.background.restore()
        XCTAssertFalse(FileManager.default.fileExists(atPath: f.transport.files[0].path))
        XCTAssertEqual(f.provider.completionCalls, 0)
    }

    func testPayloadRangeBoundsRejectInvalidInput() async throws {
        let f = try await fixture()
        let record = try await createPaused(f)
        for count in [-1, 0, UploadPolicy.chunkBytes + 1] {
            do {
                _ = try await f.staging.prepareBackgroundChunk(
                    record, id: UploadOperationId(), offset: 0, count: count, durable: [])
                XCTFail("Invalid range must fail")
            } catch { XCTAssertEqual(error as? UploadFailure, .invalidMetadata) }
        }
    }

    func testRequestMetadataContainsNoBearerOrProviderPath() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let jobValue = try await f.queue.database.backgroundJob(record.id)
        let job = try XCTUnwrap(jobValue)
        let metadata = String(decoding: try JSONEncoder().encode(job), as: UTF8.self)
        XCTAssertFalse(metadata.contains(queueBearer))
        XCTAssertFalse(metadata.contains(f.source.path))
        XCTAssertFalse(metadata.contains("Authorization"))
        XCTAssertEqual(f.transport.tasks[0].intentId, job.chunk?.id)
        XCTAssertNotEqual(f.transport.tasks[0].intentId, record.intent.idempotencyKey.rawValue)
    }

    func testViewModelShowsBackgroundStateAndInvalidationFencesControls() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let model = FileUploadViewModel(
            coordinator: f.coordinator, controller: f.queue.controller,
            library: f.queue.scope.libraryId, libraryStatus: .active)
        await model.refreshBackgroundState()
        XCTAssertEqual(model.backgroundJobs[record.id]?.state, .scheduled)
        XCTAssertEqual(model.displayedProgress(for: record).acknowledgedBytes, 0)
        model.invalidate()
        XCTAssertFalse(model.isCurrent)
        XCTAssertFalse(model.isAvailable)
    }

    func testAnotherAccountCannotAdoptDelayedEvidence() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let task = try XCTUnwrap(f.transport.tasks.first)
        let other = try await queueScope(device: 191, owner: 190)
        await f.queue.credentials.replace(try queueSession(other, credential: 192))
        f.transport.tasks = []
        let persisted = await f.background.receive(
            task, evidence: evidence(offset: String(f.bytes.count)))
        XCTAssertTrue(persisted)
        await f.background.waitForRecoveryAction()
        let job = try await f.queue.database.backgroundJob(record.id)
        let saved = try await read(f, record.id)
        XCTAssertEqual(job?.state, .quarantined)
        XCTAssertEqual(saved.authoritativeOffset, 0)
        XCTAssertEqual(f.provider.statusCount, 0)
    }

    func testMissingCredentialCancelsAndQuarantinesRestoredTask() async throws {
        let f = try await fixture()
        let record = try await start(f)
        try await f.queue.credentials.delete()
        await f.background.restore()
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(job?.state, .quarantined)
        XCTAssertEqual(f.transport.cancelled, [1])
        XCTAssertEqual(f.provider.statusCount, 0)
    }

    func testPauseBetweenBindingAndNativeResumeIsFenced() async throws {
        let f = try await fixture()
        f.transport.beforeResume = { f.background.invalidateSession() }
        _ = await f.coordinator.start(
            selectedURL: f.source, target: f.target,
            library: f.queue.scope.libraryId, libraryStatus: .active, backgroundEnabled: true,
            progress: { _ in })
        await f.background.waitForInvalidation()
        XCTAssertTrue(f.transport.resumed.isEmpty)
        XCTAssertEqual(f.transport.submissions.count, 1)
        XCTAssertEqual(f.provider.completionCalls, 0)
    }

    func testDelayedOSCancellationCannotReleasePATCHExclusion() async throws {
        let f = try await fixture()
        let record = try await start(f)
        f.transport.keepsCancelledTask = true
        let session = try await f.provider.begin()
        do {
            try await f.background.prepareForeground(record, session: session)
            XCTFail("An outstanding system task must block foreground dispatch")
        } catch { XCTAssertEqual(error as? UploadFailure, .completionUnknown) }
        let job = try await f.queue.database.backgroundJob(record.id)
        XCTAssertTrue(job?.blocksForeground == true)
        XCTAssertEqual(f.provider.statusCount, 0)
        XCTAssertEqual(f.transport.submissions.count, 1)
    }

    func testApplicationBootstrapRunsWithoutSceneAndOnlyOnce() async {
        let bootstrap = BackgroundUploadApplicationBootstrap()
        var calls = 0
        bootstrap.start()  // UIKit launch can precede app composition.
        bootstrap.install { calls += 1 }
        await bootstrap.waitForStartup()
        bootstrap.start()
        await bootstrap.waitForStartup()
        XCTAssertEqual(calls, 1)
    }

    func testExplicitAbortCancelsLocalTaskThenAbortsOriginalSession() async throws {
        let f = try await fixture()
        let record = try await start(f)
        let result = await f.coordinator.abort(id: record.id, library: f.queue.scope.libraryId)
        guard case .success(let aborted) = result else {
            return XCTFail("Explicit abort should retain its original session")
        }
        XCTAssertEqual(aborted.state, .aborted)
        XCTAssertEqual(aborted.serverSessionId, record.serverSessionId)
        XCTAssertEqual(f.transport.cancelled, [1])
        XCTAssertEqual(f.provider.createBodies.count, 1)
        XCTAssertEqual(f.provider.completionCalls, 0)
        XCTAssertTrue(FileManager.default.fileExists(atPath: f.source.path))
    }

    func testUserPauseDuringSubmissionWithdrawsDurableAuthorization() async throws {
        let f = try await fixture()
        f.transport.beforeResume = {
            do { try await f.coordinator.pauseUserTransfer() } catch {
                XCTFail("Pause persistence failed")
            }
        }
        let result = await f.coordinator.start(
            selectedURL: f.source, target: f.target,
            library: f.queue.scope.libraryId, libraryStatus: .active, backgroundEnabled: true,
            progress: { _ in })
        guard case .scheduled(let record) = result else {
            return XCTFail("The durable job should remain inspectable")
        }
        let paused = try await f.queue.database.backgroundJob(record.id)
        XCTAssertEqual(paused?.state, .paused)
        XCTAssertEqual(paused?.authorized, false)
        XCTAssertTrue(f.transport.resumed.isEmpty)
        await f.background.restore()
        XCTAssertTrue(f.transport.resumed.isEmpty)
        XCTAssertEqual(f.transport.submissions.count, 1)
        XCTAssertEqual(f.provider.completionCalls, 0)
    }

    func testFutureMetadataEncodingAndUnknownFieldsFailClosed() throws {
        var job = BackgroundUploadJob(
            operationId: UploadOperationId().rawValue, generation: 0,
            authorized: true, state: .preparing, chunk: nil, resolved: true,
            taskIdentifier: nil, evidence: nil)
        let encoded = try JSONEncoder().encode(job)
        XCTAssertEqual(try BackgroundUploadJob.decodeStored(encoded), job)
        job.encodingVersion = 2
        XCTAssertThrowsError(try BackgroundUploadJob.decodeStored(JSONEncoder().encode(job)))
        var object = try XCTUnwrap(JSONSerialization.jsonObject(with: encoded) as? [String: Any])
        object["futureAuthorization"] = true
        XCTAssertThrowsError(
            try BackgroundUploadJob.decodeStored(JSONSerialization.data(withJSONObject: object)))
    }

    private func evidence(
        offset: String, requestId: String = "background-patch-01",
        status: Int = 204, bodyEmpty: Bool = true, failure: UploadFailure? = nil
    ) -> BackgroundUploadEvidence {
        BackgroundUploadEvidence(
            taskIdentifier: 1, status: status, offset: offset,
            requestId: requestId, bodyWasEmpty: bodyEmpty, failure: failure?.rawValue)
    }

    private func chunkIntent(length: Int) -> BackgroundChunkIntent {
        BackgroundChunkIntent(
            id: UploadOperationId().rawValue, offset: 0, length: length,
            sha256: digest(Data(repeating: 0, count: length)))
    }

    private func digest(_ bytes: Data) -> String {
        "sha256:" + SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    }

    private func settle() async { for _ in 0..<100 { await Task.yield() } }

    private func read(_ f: BackgroundFixture, _ id: UploadOperationId) async throws -> UploadRecord
    {
        let saved = try await f.queue.database.upload(
            scope: f.queue.scope, credentialId: queueUUID(92),
            id: id, bridge: QueueValidator())
        return try XCTUnwrap(saved)
    }

    private func start(_ f: BackgroundFixture) async throws -> UploadRecord {
        let result = await f.coordinator.start(
            selectedURL: f.source, target: f.target,
            library: f.queue.scope.libraryId, libraryStatus: .active, backgroundEnabled: true,
            progress: { _ in })
        guard case .scheduled(let record) = result else { throw UploadFailure.invalidMetadata }
        return record
    }

    private func createPaused(_ f: BackgroundFixture) async throws -> UploadRecord {
        let record = try await start(f)
        try await f.background.pause(record.id)
        f.transport.tasks = []
        let session = try await f.provider.begin()
        try await f.background.prepareForeground(record, session: session)
        f.transport.submissions = []
        f.transport.files = []
        f.transport.resumed = []
        f.transport.cancelled = []
        return try await read(f, record.id)
    }

    private func fixture(fault: QueueFaultInjector? = nil) async throws -> BackgroundFixture {
        let queue = try await queueFixture(self, fault: fault)
        let parent = try await NodeId.validated(queueUUID(2), using: QueueValidator())
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "BackgroundUpload-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let bytes = Data((0..<4096).map { UInt8(truncatingIfNeeded: $0 &* 31) })
        let source = directory.appendingPathComponent("selected.bin")
        try bytes.write(to: source)
        let provider = UploadCoordinatorProvider(
            base: queue.provider, library: queue.scope.libraryId,
            parent: parent, source: bytes, loseFirstCreate: false,
            partiallyAcceptFirstPatch: bytes.count, loseCompletion: false)
        let repository = AuthenticatedUploadRepository(provider: provider, bridge: QueueValidator())
        let staging = UploadStagingStore(
            shared: try DownloadStagingStore(
                root: directory.appendingPathComponent("staging"), freeBytes: { _ in 2_000_000_000 }
            ))
        let transport = FakeBackgroundTransport()
        let background = BackgroundUploadCoordinator(
            repository: repository, database: queue.database,
            staging: staging, transport: transport)
        let coordinator = FileUploadCoordinator(
            repository: repository, database: queue.database,
            staging: staging, identityGenerator: FixedUploadIDGenerator(value: queueUUID(801)),
            background: background)
        return BackgroundFixture(
            queue: queue, bytes: bytes, source: source,
            target: .createFile(
                library: queue.scope.libraryId, parent: parent, name: "background.bin"),
            provider: provider, repository: repository, staging: staging, transport: transport,
            background: background, coordinator: coordinator)
    }
}

@MainActor
private struct BackgroundFixture {
    let queue: QueueFixture
    let bytes: Data
    let source: URL
    let target: UploadTarget
    let provider: UploadCoordinatorProvider
    let repository: AuthenticatedUploadRepository
    let staging: UploadStagingStore
    let transport: FakeBackgroundTransport
    let background: BackgroundUploadCoordinator
    let coordinator: FileUploadCoordinator
}

@MainActor
private final class FakeBackgroundTransport: BackgroundUploadTransportProtocol {
    var tasks: [BackgroundSystemTask] = []
    var originalTask: BackgroundSystemTask?
    var submissions: [HTTPTransportRequest] = []
    var files: [URL] = []
    var resumed: [Int] = []
    var cancelled: [Int] = []
    var afterSubmit: (() -> Void)?
    var keepsCancelledTask = false
    var beforeResume: (() async -> Void)?
    func inventory() async -> [BackgroundSystemTask] { tasks }
    func submit(_ request: HTTPTransportRequest, file: URL, intent: BackgroundChunkIntent) throws
        -> Int
    {
        submissions.append(request)
        files.append(file)
        let id = submissions.count
        let task = BackgroundSystemTask(
            identifier: id, intentId: intent.id, requestURL: request.url,
            method: "PATCH", offset: request.headers["Upload-Offset"],
            length: request.headers["Content-Length"])
        tasks.append(task)
        originalTask = task
        afterSubmit?()
        return id
    }
    func resume(_ identifier: Int, authorized: @escaping @MainActor () -> Bool) async {
        await beforeResume?()
        if authorized() { resumed.append(identifier) } else { await cancel(identifier) }
    }
    func cancel(_ identifier: Int) async {
        cancelled.append(identifier)
        if !keepsCancelledTask { tasks.removeAll { $0.identifier == identifier } }
    }
}

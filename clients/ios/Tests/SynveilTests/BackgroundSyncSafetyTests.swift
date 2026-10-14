import Foundation
import XCTest

@testable import Synveil

private actor RefreshAuthorizationValidator: AuthenticatedSessionValidationProtocol {
    let result: AuthenticatedSessionValidationResult
    init(_ result: AuthenticatedSessionValidationResult) { self.result = result }
    func validateAuthorization(for session: DeviceCredentialSession) async
        -> AuthenticatedSessionValidationResult
    {
        result
    }
}

@MainActor
final class BackgroundSyncSafetyTests: XCTestCase {
    func testBuiltApplicationHasTypedRefreshIdentifierAndFetchCapability() {
        XCTAssertEqual(Bundle.main.bundleIdentifier, "com.synveil.ios")
        XCTAssertEqual(
            Bundle.main.object(forInfoDictionaryKey: "BGTaskSchedulerPermittedIdentifiers")
                as? [String],
            [BackgroundSyncLimits.identifier])
        XCTAssertEqual(
            Bundle.main.object(forInfoDictionaryKey: "UIBackgroundModes") as? [String], ["fetch"])
    }
    func testBackgroundConfigurationHasConservativeFiniteBounds() {
        XCTAssertTrue(InboundSyncRunConfiguration.background.isValid)
        XCTAssertEqual(InboundSyncRunConfiguration.background.maximumPages, 1)
        XCTAssertEqual(InboundSyncRunConfiguration.background.maximumEvents, 8)
        XCTAssertFalse(InboundSyncRunConfiguration.background.allowsResumingStagedPages)
        XCTAssertEqual(BackgroundSyncLimits.maximumLibraries, 2)
        XCTAssertLessThan(BackgroundSyncLimits.runtime, 30)
    }
    func testOneNewPageIsAppliedAndServerConfirmedWithoutAnotherFeed() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(pages: 2)
        let result = await f.run(.background)
        XCTAssertEqual(result.reason, .moreWork)
        XCTAssertEqual(result.progress.pagesProcessed, 1)
        XCTAssertEqual(result.progress.eventsApplied, 1)
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "1")
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(requests.map(\.method), [.get, .post])
        XCTAssertEqual(
            URLComponents(url: requests[0].url, resolvingAgainstBaseURL: false)?.queryItems?.first?
                .value, "8")
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let confirmed = try await reopened.latestConfirmedInbound(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(confirmed?.state, .ackConfirmed)
    }
    func testResponseOverRequestedEventBudgetCannotStageOrAck() async throws {
        let f = try await inboundFixture(self)
        try await f.configure(count: 9)
        let result = await f.run(.background)
        XCTAssertEqual(result.reason, .protocolFailure)
        let record = try await f.record()
        XCTAssertNil(record)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testPreviouslyReceivedPageRequiresForegroundAction() async throws {
        let f = try await inboundFixture(self)
        _ = try await f.stage()
        let result = await f.run(.background)
        XCTAssertEqual(result.reason, .recoveryConfirmationRequired)
        let record = try await f.record()
        XCTAssertEqual(record?.state, .receivedUnapplied)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        XCTAssertTrue(f.nodes.calls.isEmpty)
    }
    func testPreviouslyAppliedPageDoesNotAutomaticallySubmitInitialAck() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        let result = await f.run(.background)
        XCTAssertEqual(result.reason, .recoveryConfirmationRequired)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        let record = try await f.record()
        XCTAssertEqual(record?.state, .appliedAckPending)
    }
    func testUnknownAckIsNotReplayedAndAttemptsAreUnchanged() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        let receipt = try await f.ack.receipt(scope: f.scope, position: page.start)
        _ = receipt
        let before = try queueRawScalar(f.base.url, "SELECT count(*) FROM sync_ack_attempts")
        let result = await f.run(.background)
        XCTAssertEqual(result.reason, .awaitingAckRecovery)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM sync_ack_attempts"), before)
        let record = try await f.record()
        XCTAssertEqual(record?.state, .ackInFlight)
    }
    func testRebaselineIsDeferredWithoutNewRequests() async throws {
        let f = try await inboundFixture(self)
        _ = try await f.stage()
        try await f.database.blockInbound(scope: f.scope, credentialId: queueUUID(92))
        let result = await f.run(.background)
        XCTAssertEqual(result.reason, .rebaselineRequired)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM rebaseline_sessions"), "0")
    }
    func testForegroundRunOwnsLibraryAcrossDifferentConnections() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let foreground = Task { await f.run() }
        await gate.wait()
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let background = try await inboundFixture(self, base: f.base, database: reopened)
        let result = await background.run(.background)
        XCTAssertEqual(result.reason, .alreadyRunning)
        let requests = await background.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        await gate.release()
        _ = await foreground.value
    }
    func testExpirationDuringFeedLeavesNoUnverifiedStaging() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 0, gate)
        let task = Task { await f.run(.background) }
        await gate.wait()
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .cancelled)
        let record = try await f.record()
        XCTAssertNil(record)
    }
    func testCanceledCredentialCaptureCannotQuarantineOrStealForegroundOwnership() async throws {
        let f = try await inboundFixture(self)
        let gate = QueueGate()
        await f.base.credentials.suspendNextLoad(gate)
        let task = Task { await f.run(.background) }
        await gate.wait()
        let foregroundOwner = UUID()
        try await f.database.claimInboundRun(
            scope: f.scope, credentialId: queueUUID(92), owner: foregroundOwner)
        task.cancel()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .cancelled)
        XCTAssertEqual(f.base.controller.state, .authenticated)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT quarantined FROM scopes"), "0")
        do {
            try await f.database.claimInboundRun(
                scope: f.scope, credentialId: queueUUID(92), owner: UUID())
            XCTFail("Foreground ownership was stolen")
        } catch { XCTAssertEqual(error as? MutationQueueFailure, .concurrentExecution) }
        await f.database.finishInboundRun(owner: foregroundOwner)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testExpirationAfterStagingPreservesOriginalReceivedPage() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        var task: Task<InboundSyncRunResult, Never>?
        task = Task { await f.run(.background) { if $0.phase == .staged { task?.cancel() } } }
        _ = await task!.value
        let record = try await f.record()
        XCTAssertEqual(record?.state, .receivedUnapplied)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 1)
    }
    func testExpirationDuringMaterializationCannotCommitPartialProjection() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        f.nodes.gate = gate
        let task = Task { await f.run(.background) }
        await gate.wait()
        task.cancel()
        await gate.release()
        _ = await task.value
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM cached_nodes"), "0")
        let record = try await f.record()
        XCTAssertEqual(record?.state, .receivedUnapplied)
    }
    func testExpirationAfterApplicationPreservesPendingAckAndProjection() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        var task: Task<InboundSyncRunResult, Never>?
        task = Task { await f.run(.background) { if $0.phase == .applied { task?.cancel() } } }
        _ = await task!.value
        let record = try await f.record()
        XCTAssertEqual(record?.state, .appliedAckPending)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT applied_sequence FROM node_projection_state"),
            "1")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT confirmed_sequence FROM node_projection_state"),
            "0")
    }
    func testExpirationAfterAckDispatchPreservesUnknownEvidenceAcrossReopen() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 1, gate)
        let task = Task { await f.run(.background) }
        await gate.wait()
        task.cancel()
        await gate.release()
        _ = await task.value
        let reopened = try MutationQueueSQLiteStore(url: f.base.url)
        let record = try await reopened.inboundPage(
            scope: f.scope, position: feedPosition(), credentialId: queueUUID(92),
            bridge: QueueValidator())
        XCTAssertEqual(record?.state, .ackInFlight)
        XCTAssertEqual(record?.page.evidence?.token, "v1.original-page-0")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT confirmed_sequence FROM node_projection_state"),
            "0")
    }
    func testAckCommitReturnLossUsesDurableReadbackWithoutReplay() async throws {
        let fault = QueueFaultInjector()
        let f = try await inboundFixture(self, fault: fault)
        try await f.configure()
        fault.arm(.afterAckConfirmationCommit)
        let result = await f.run(.background)
        XCTAssertEqual(result.progress.serverConfirmed?.sequence.rawValue, "1")
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM sync_ack_attempts"), "1")
    }
    func testOfflineAnd503CannotAdvanceCheckpoint() async throws {
        for server in [true, false] {
            let f = try await inboundFixture(self)
            if server {
                await f.wire.configure([try inboundError("dependency_unavailable", status: 503)])
            }
            let result = await f.run(.background)
            XCTAssertTrue([.offline, .serverUnavailable].contains(result.reason))
            XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT sequence FROM sync_bases"), "0")
        }
    }
    func testMissingCredentialAndRevokedDeviceStopBeforeFeed() async throws {
        let f = try await inboundFixture(self)
        try await authorize(f)
        let work = makeWork(f, result: .deviceRevoked)
        let session = try await work.capture()
        do {
            try await work.verify(session: session, library: f.scope.libraryId)
            XCTFail("Revoked")
        } catch {}
        XCTAssertEqual(f.base.controller.state, .recoveryRequired(.deviceRevoked))
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        let missing = try await inboundFixture(self)
        try await missing.base.credentials.delete()
        do {
            _ = try await makeWork(missing).capture()
            XCTFail("Missing Keychain")
        } catch {}
    }
    func testProductionOrchestrationPublishesOnlyActualSQLiteConfirmation() async throws {
        let f = try await inboundFixture(self)
        try await authorize(f)
        try await f.projection.rememberLibrary(try await inboundLibrary(f), scope: f.scope)
        try await f.configure()
        let policy = try await refreshPolicy()
        let outcome = await BackgroundSyncCoordinator(
            policy: policy, work: makeWork(f),
            gate: BackgroundSyncRequestGate(conditionsAllow: { true })
        )
        .run(delivery: RefreshCompletions().delivery())
        XCTAssertEqual(outcome.state, .partial)
        XCTAssertEqual(outcome.pages, 1)
        XCTAssertEqual(outcome.events, 1)
        let saved = try await f.database.latestConfirmedInbound(
            scope: f.scope, credentialId: queueUUID(92), bridge: QueueValidator())
        XCTAssertEqual(outcome.confirmation?.recordedAt, saved?.updatedAt)
        XCTAssertEqual(outcome.confirmation?.position.sequence.rawValue, "1")
        XCTAssertTrue(outcome.taskSucceeded)
    }
    func testDurableAuthorizationUncertaintyAndQuarantineRemainClosed() async throws {
        let f = try await inboundFixture(self)
        let session = try await f.provider.begin()
        let lease = try await f.database.beginSessionAuthorizationCheck(
            SessionAuthorizationIdentity(session.restorationSession))
        do {
            _ = try await f.database.backgroundLibraryIdentifiers(session: session, after: nil)
            XCTFail("Uncertain")
        } catch {}
        try await f.database.finishSessionAuthorizationCheck(lease, outcome: .authorized)
        try await f.database.quarantineMetadataScope(scope: f.scope, credentialId: queueUUID(92))
        let ids = try await f.database.backgroundLibraryIdentifiers(session: session, after: nil)
        XCTAssertTrue(ids.isEmpty)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT quarantined FROM scopes"), "1")
    }
    func testSelectionUsesScopeCredentialAndDeterministicWraparound() async throws {
        let f = try await inboundFixture(self)
        try await authorize(f)
        try await f.projection.rememberLibrary(try await inboundLibrary(f), scope: f.scope)
        let session = try await f.provider.begin()
        let selected = try await f.database.backgroundLibraryIdentifiers(
            session: session, after: nil)
        let wrapped = try await f.database.backgroundLibraryIdentifiers(
            session: session, after: f.scope.libraryId.rawValue)
        XCTAssertEqual(selected, [f.scope.libraryId.rawValue])
        XCTAssertEqual(wrapped, selected)
        let other = try queueSession(f.scope, credential: 93)
        let mismatch = LibraryRequestScope(session: other, revision: session.lifecycleRevision)
        do {
            _ = try await f.database.backgroundLibraryIdentifiers(session: mismatch, after: nil)
            XCTFail("Another credential")
        } catch {}
        XCTAssertEqual(try queueRawScalar(f.base.url, "PRAGMA user_version"), "9")
    }
    func testAuthorizationVerificationDefersWhenForegroundOwnsRun() async throws {
        let f = try await inboundFixture(self)
        try await authorize(f)
        let session = try await f.provider.begin()
        let owner = UUID()
        try await f.database.claimInboundRun(
            scope: f.scope, credentialId: session.credentialIdentifier, owner: owner)
        let work = makeWork(f)
        do {
            try await work.verify(session: session, library: f.scope.libraryId)
            XCTFail("Owned")
        } catch {
            XCTAssertEqual(error as? MutationQueueFailure, .concurrentExecution)
        }
        await f.database.finishInboundRun(owner: owner)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT state FROM session_authorization_checks"),
            "AUTHORIZED")
    }
    func testTransientEvidencePermitsOnlyVerificationAndUncertainEvidenceDoesNot() async throws {
        let f = try await inboundFixture(self)
        try await authorize(f)
        let session = try await f.provider.begin()
        let identity = SessionAuthorizationIdentity(session.restorationSession)
        let lease = try await f.database.beginSessionAuthorizationCheck(identity)
        try await f.database.finishSessionAuthorizationCheck(lease, outcome: .transientFailure)
        try await f.database.requireBackgroundVerificationEligibility(session)
        do {
            try await f.database.requireBackgroundAuthorization(session)
            XCTFail("No current grant")
        } catch {}
        _ = try await f.database.beginSessionAuthorizationCheck(identity)
        do {
            try await f.database.requireBackgroundVerificationEligibility(session)
            XCTFail("Uncertain")
        } catch {}
    }
    func testThreeLibrariesUseBoundedFairSeekSelectionAcrossOpportunities() async throws {
        let f = try await inboundFixture(self)
        try await authorize(f)
        let original = try await inboundLibrary(f)
        for number in 80...82 {
            let scope = try await queueScope(library: number)
            let session = try await f.queue.capture(scope: scope)
            _ = try await f.database.persistCheckpoint(queueCheckpoint(scope: scope))
            let library = Library(
                id: scope.libraryId, revision: original.revision, name: original.name,
                rootNodeId: original.rootNodeId, status: .active,
                createdAt: original.createdAt, updatedAt: original.updatedAt)
            try await f.database.rememberLibrary(
                library, scope: scope, credentialId: session.credentialIdentifier,
                bridge: QueueValidator())
        }
        let session = try await f.provider.begin()
        let first = try await f.database.backgroundLibraryIdentifiers(session: session, after: nil)
        let second = try await f.database.backgroundLibraryIdentifiers(
            session: session, after: first.last)
        XCTAssertEqual(first, [queueUUID(80), queueUUID(81)])
        XCTAssertEqual(second, [queueUUID(82), queueUUID(80)])
    }
    func testLogoutDuringBackgroundAckCannotPublishUnderReplacementScope() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 1, gate)
        let task = Task { await f.run(.background) }
        await gate.wait()
        await f.base.controller.requestLogout()
        await gate.release()
        let result = await task.value
        XCTAssertEqual(result.reason, .committedButSessionChanged)
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT state FROM inbound_pages"), "ACK_IN_FLIGHT")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT confirmed_sequence FROM node_projection_state"),
            "0")
    }
    func testP046IdentifierAndUploadOwnershipRemainIndependent() async throws {
        let f = try await inboundFixture(self)
        try await f.configure()
        _ = await f.run(.background)
        XCTAssertNotEqual(
            BackgroundSyncLimits.identifier, URLSessionBackgroundUploadTransport.identifier)
        let requests = await f.wire.requests()
        XCTAssertTrue(
            requests.allSatisfy {
                $0.url.path.hasSuffix("/changes") || $0.url.path.hasSuffix("/changes/ack")
            })
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM file_uploads"), "0")
        XCTAssertEqual(
            try queueRawScalar(f.base.url, "SELECT count(*) FROM background_upload_jobs"), "0")
    }
    private func authorize(_ f: InboundFixture) async throws {
        let session = try await f.provider.begin()
        let lease = try await f.database.beginSessionAuthorizationCheck(
            SessionAuthorizationIdentity(session.restorationSession))
        try await f.database.finishSessionAuthorizationCheck(lease, outcome: .authorized)
    }
    private func makeWork(
        _ f: InboundFixture, result: AuthenticatedSessionValidationResult = .authorized
    ) -> BackgroundSyncWork {
        BackgroundSyncWork(
            provider: f.provider, database: f.database, bridge: QueueValidator(),
            restoration: SessionRestorationService(
                credentialStore: f.base.credentials,
                authorizationValidator: RefreshAuthorizationValidator(result),
                authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: f.database)),
            inbound: f.coordinator)
    }
}

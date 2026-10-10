import Foundation
import XCTest

@testable import Synveil

#if canImport(SwiftUI)
    import SwiftUI
    import UIKit
#endif

@MainActor
private final class StatusCoordinatorFixture: InboundSyncCoordinatorProtocol {
    let identity: ClientMutationScope
    var runs = 0
    var recoveries = 0
    var reads = 0
    var result = InboundSyncRunResult(reason: .upToDate, progress: .initial, recovery: nil)
    var local = InboundSyncStatus(
        progress: .initial, completeness: .partial, pendingState: nil, recovery: nil,
        stopReason: nil)
    var gate: QueueGate?
    var published = InboundSyncProgress.initial
    init(_ identity: ClientMutationScope) { self.identity = identity }
    func scope(libraryId: LibraryId) async throws -> ClientMutationScope {
        guard libraryId == identity.libraryId else { throw MutationQueueFailure.scopeMismatch }
        return identity
    }
    func status(scope: ClientMutationScope) async -> InboundSyncStatus {
        reads += 1
        return local
    }
    func synchronize(
        scope: ClientMutationScope, configuration: InboundSyncRunConfiguration,
        progress: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult {
        runs += 1
        XCTAssertEqual(scope, identity)
        XCTAssertEqual(configuration, .foreground)
        progress(published)
        if let gate { await gate.arrive() }
        return result
    }
    func recoverUnknownAcknowledgement(
        scope: ClientMutationScope, position: SyncJournalPosition,
        confirmedByUser: Bool, progress: @MainActor (InboundSyncProgress) -> Void
    ) async -> InboundSyncRunResult {
        XCTAssertTrue(confirmedByUser)
        recoveries += 1
        return result
    }
}

@MainActor
final class SyncStatusViewModelTests: XCTestCase {
    private func model(
        _ f: InboundFixture, coordinator: (any InboundSyncCoordinatorProtocol)? = nil,
        status: LibraryStatus = .active
    ) async throws -> SyncStatusViewModel {
        SyncStatusViewModel(
            library: try await inboundLibrary(f, status: status),
            coordinator: coordinator ?? f.coordinator, projection: f.projection,
            checkpoint: f.base.service, sessionController: f.base.controller)
    }
    func testOpeningStatusReadsLocallyWithoutNetwork() async throws {
        let f = try await inboundFixture(self)
        let model = try await model(f)
        await model.loadStatus()
        XCTAssertEqual(model.state, .ready)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testSyncNowInvokesInjectedCoordinator() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        await model.syncNow()
        XCTAssertEqual(injected.runs, 1)
        XCTAssertEqual(model.state, .upToDate)
    }
    func testDoubleTapDoesNotCreateDuplicateRun() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        let gate = QueueGate()
        injected.gate = gate
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        let first = Task { await model.syncNow() }
        await gate.wait()
        XCTAssertFalse(model.canSync)
        await model.syncNow()
        XCTAssertEqual(injected.runs, 1)
        await gate.release()
        await first.value
    }
    func testLoadingPublishesActualPhaseAndVerifiedCounters() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        let gate = QueueGate()
        injected.gate = gate
        injected.published = InboundSyncProgress(
            phase: .applied, pagesProcessed: 0, eventsApplied: 7,
            locallyApplied: try feedPosition(from: "7"),
            serverConfirmed: try feedPosition(), observedHighWatermark: try feedPosition(from: "20")
        )
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        let first = Task { await model.syncNow() }
        await gate.wait()
        XCTAssertEqual(model.state, .syncing)
        XCTAssertEqual(model.message, "Metadata committed locally")
        XCTAssertEqual(model.progress.eventsApplied, 7)
        XCTAssertEqual(model.progress.pagesProcessed, 0)
        XCTAssertEqual(model.progress.serverConfirmed?.sequence.rawValue, "0")
        await gate.release()
        await first.value
    }
    func testRealCoordinatorDrivesUpToDateUIAfterEmptyFeed() async throws {
        let f = try await inboundFixture(self)
        let model = try await model(f)
        await model.loadStatus()
        XCTAssertNotEqual(model.state, .upToDate)
        try await f.configure(pages: 0)
        await model.syncNow()
        XCTAssertEqual(model.state, .upToDate)
        XCTAssertEqual(model.message, "No new changes.")
    }
    func testBudgetExhaustionShowsMoreWork() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        injected.result = .init(reason: .moreWork, progress: .initial, recovery: nil)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        await model.syncNow()
        XCTAssertEqual(model.state, .moreWork)
        XCTAssertTrue(model.message.contains("Sync again to continue"))
    }
    func testMissingCheckpointShowsExplicitSetupGuidance() async throws {
        let f = try await inboundFixture(self, initializeBase: false)
        let model = try await model(f)
        await model.loadStatus()
        XCTAssertEqual(model.state, .checkpointRequired)
        XCTAssertFalse(model.canSync)
        XCTAssertTrue(model.canSetup)
        XCTAssertTrue(model.message.contains("may initialize"))
    }
    func testCheckpointSetupRequiresConfirmation() async throws {
        let f = try await inboundFixture(self, initializeBase: false)
        let model = try await model(f)
        await model.loadStatus()
        await model.prepareCheckpoint(confirmedByUser: false)
        XCTAssertEqual(model.state, .checkpointRequired)
        XCTAssertEqual(try queueRawScalar(f.base.url, "SELECT count(*) FROM sync_bases"), "0")
    }
    func testConfirmedCheckpointSetupDoesNotStartFeed() async throws {
        let f = try await inboundFixture(self, initializeBase: false)
        let model = try await model(f)
        await model.loadStatus()
        await model.prepareCheckpoint(confirmedByUser: true)
        XCTAssertEqual(model.state, .ready)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
        let setup = await f.base.transport.requests()
        XCTAssertEqual(setup.count, 1)
        XCTAssertTrue(setup[0].url.path.hasSuffix("/checkpoint"))
    }
    func testOfflineRetainsPartialCacheAndDisablesSyncNow() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        let injected = StatusCoordinatorFixture(f.scope)
        injected.result = .init(reason: .offline, progress: .initial, recovery: nil)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        await model.syncNow()
        XCTAssertEqual(model.state, .offline)
        XCTAssertFalse(model.canSync)
        XCTAssertTrue(model.message.contains("retained"))
        XCTAssertTrue(model.cacheMessage.contains("Partial"))
    }
    func testOfflineRetryIsExplicitAndBounded() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        injected.result = .init(reason: .offline, progress: .initial, recovery: nil)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        await model.syncNow()
        XCTAssertEqual(injected.runs, 1)
        injected.result = .init(reason: .upToDate, progress: .initial, recovery: nil)
        await model.retryWhenConnectionAvailable()
        XCTAssertEqual(injected.runs, 2)
        XCTAssertEqual(model.state, .upToDate)
    }
    private func shows(_ reason: InboundSyncStopReason, state: SyncStatusUIState, text: String)
        async throws
    {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        injected.result = .init(reason: reason, progress: .initial, recovery: nil)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        await model.syncNow()
        XCTAssertEqual(model.state, state)
        XCTAssertTrue(model.message.contains(text))
    }
    func testAuthenticationGuidance() async throws {
        try await shows(
            .authenticationRequired, state: .failed, text: "Authentication needs recovery")
    }
    func testRevocationGuidanceDistinct() async throws {
        try await shows(.deviceRevoked, state: .failed, text: "revoked")
    }
    func testRebaselineHasNoSafeRetry() async throws {
        try await shows(
            .rebaselineRequired, state: .reconciliationRequired, text: "not available yet")
    }
    func testMetadataRevisionChangeGuidance() async throws {
        try await shows(.metadataChanged, state: .reconciliationRequired, text: "Server changed")
    }
    func testCheckpointConflictGuidance() async throws {
        try await shows(
            .reconciliationRequired, state: .reconciliationRequired, text: "does not match")
    }
    func testStorageFailureGuidance() async throws {
        try await shows(.storageFailure, state: .failed, text: "storage is unavailable")
    }
    func testCancellationNeverClaimsRollback() async throws {
        try await shows(.cancelled, state: .cancelled, text: "committed work may remain")
    }
    func testUnknownAckRequiresRecoveryConfirmation() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        injected.local = .init(
            progress: .initial, completeness: .partial, pendingState: .ackInFlight,
            recovery: .init(position: try feedPosition(), attempts: 1),
            stopReason: .awaitingAckRecovery)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        XCTAssertEqual(model.state, .unknownAck)
        XCTAssertFalse(model.canSync)
        XCTAssertTrue(model.canRecover)
        await model.recoverAcknowledgement(confirmedByUser: false)
        XCTAssertEqual(injected.recoveries, 0)
        injected.result = .init(reason: .progressed, progress: .initial, recovery: nil)
        await model.recoverAcknowledgement(confirmedByUser: true)
        XCTAssertEqual(injected.recoveries, 1)
    }
    func testRetryLimitDisablesRecovery() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        injected.local = .init(
            progress: .initial, completeness: .partial, pendingState: .ackInFlight,
            recovery: .init(position: try feedPosition(), attempts: 8),
            stopReason: .recoveryLimitReached)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        XCTAssertFalse(model.canRecover)
        await model.recoverAcknowledgement(confirmedByUser: true)
        XCTAssertEqual(injected.recoveries, 0)
    }
    func testLogoutInvalidatesLateUIResult() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        let gate = QueueGate()
        injected.gate = gate
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        let task = Task { await model.syncNow() }
        await gate.wait()
        await f.base.controller.requestLogout()
        model.sessionDidChange()
        await gate.release()
        await task.value
        XCTAssertEqual(model.state, .invalidated)
        XCTAssertNil(model.progress.locallyApplied)
        XCTAssertFalse(model.canSync)
    }
    func testLibraryModelsKeepIndependentProgress() async throws {
        let f = try await inboundFixture(self)
        let first = try await model(f)
        await first.loadStatus()
        let otherScope = try await queueScope(library: 81)
        let otherLibrary = Library(
            id: otherScope.libraryId, revision: try LibraryRevision(validating: "1"),
            name: "Other Library",
            rootNodeId: try await NodeId.validated(queueUUID(2), using: QueueValidator()),
            status: .active, createdAt: Date(), updatedAt: Date())
        let other = SyncStatusViewModel(
            library: otherLibrary, coordinator: f.coordinator,
            projection: f.projection, checkpoint: f.base.service,
            sessionController: f.base.controller)
        await other.loadStatus()
        try await f.configure()
        await first.syncNow()
        XCTAssertEqual(first.progress.serverConfirmed?.sequence.rawValue, "1")
        XCTAssertNil(other.progress.serverConfirmed)
        XCTAssertEqual(other.state, .checkpointRequired)
    }
    func testReadOnlyLibraryCanReadSync() async throws {
        let f = try await inboundFixture(self)
        let model = try await model(f, status: .readOnly)
        await model.loadStatus()
        XCTAssertTrue(model.canSync)
        try await f.configure(pages: 0)
        await model.syncNow()
        XCTAssertEqual(model.state, .upToDate)
    }
    func testQuarantinedLibraryCannotSynchronize() async throws {
        let f = try await inboundFixture(self)
        let model = try await model(f, status: .quarantined)
        await model.loadStatus()
        XCTAssertFalse(model.canSync)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testPartialCacheNeverClaimsFullOfflineCopy() async throws {
        let f = try await inboundFixture(self)
        let model = try await model(f)
        await model.loadStatus()
        try await f.configure()
        await model.syncNow()
        XCTAssertEqual(model.completeness, .partial)
        XCTAssertTrue(model.cacheMessage.contains("may be missing"))
        XCTAssertFalse(model.cacheMessage.contains("Full offline copy"))
    }
    func testPendingAckIsExposedWithoutNetwork() async throws {
        let f = try await inboundFixture(self)
        let page = try await f.stage()
        _ = await f.application.apply(scope: f.scope, position: page.start)
        let model = try await model(f)
        await model.loadStatus()
        XCTAssertEqual(model.state, .pendingAck)
        XCTAssertTrue(model.canSync)
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testMissingCoordinatorShowsHonestUnavailableState() async throws {
        let f = try await inboundFixture(self)
        let model = SyncStatusViewModel(
            library: try await inboundLibrary(f), coordinator: nil,
            projection: nil, checkpoint: nil, sessionController: f.base.controller)
        await model.loadStatus()
        XCTAssertEqual(model.stopReason, .storageFailure)
        XCTAssertFalse(model.canSync)
    }
    func testLongLibraryNameIsPreserved() async throws {
        let f = try await inboundFixture(self)
        let name = String(repeating: "Long Library 📚 ", count: 40)
        let model = SyncStatusViewModel(
            library: try await inboundLibrary(f, name: name), coordinator: f.coordinator,
            projection: f.projection, checkpoint: nil, sessionController: f.base.controller)
        XCTAssertEqual(model.library.name, name)
    }

    func testSavedUnappliedPageHasExplicitResumeGuidance() async throws {
        let f = try await inboundFixture(self)
        _ = try await f.stage()
        let model = try await model(f)
        await model.loadStatus()
        XCTAssertTrue(model.message.contains("waiting to be applied"))
        let requests = await f.wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testCancellationAfterAckLoadsRecoveryLocally() async throws {
        let f = try await inboundFixture(self)
        let model = try await model(f)
        await model.loadStatus()
        try await f.configure()
        let gate = QueueGate()
        await f.wire.gate(at: 1, gate)
        let task = Task { await model.syncNow() }
        await gate.wait()
        model.cancel()
        await gate.release()
        await task.value
        XCTAssertEqual(model.state, .unknownAck)
        XCTAssertNotNil(model.recovery)
        XCTAssertTrue(model.canRecover)
        let requests = await f.wire.requests()
        XCTAssertEqual(requests.count, 2)
    }

    func testSessionChangedResultClearsProgressEvenBeforeLifecycleNotification() async throws {
        let f = try await inboundFixture(self)
        let injected = StatusCoordinatorFixture(f.scope)
        let stale = InboundSyncProgress(
            phase: .applied, pagesProcessed: 0, eventsApplied: 1,
            locallyApplied: try feedPosition(from: "1"), serverConfirmed: try feedPosition(),
            observedHighWatermark: nil)
        injected.result = .init(reason: .committedButSessionChanged, progress: stale, recovery: nil)
        let model = try await model(f, coordinator: injected)
        await model.loadStatus()
        await model.syncNow()
        XCTAssertEqual(f.base.controller.state, .authenticated)
        XCTAssertEqual(model.state, .invalidated)
        XCTAssertNil(model.progress.locallyApplied)
        XCTAssertNil(model.progress.serverConfirmed)
        XCTAssertFalse(model.canSync)
    }
    #if canImport(SwiftUI)
        func testNativeStatusViewRendersWithInjectedCoordinator() async throws {
            let f = try await inboundFixture(self)
            let view = SyncStatusView(
                library: try await inboundLibrary(f), coordinator: f.coordinator,
                projection: f.projection, checkpoint: f.base.service,
                sessionController: f.base.controller)
            XCTAssertNotNil(view.body)
            let requests = await f.wire.requests()
            XCTAssertTrue(requests.isEmpty)
        }
        func testNativeLongNameSupportsAccessibilityDynamicType() async throws {
            let f = try await inboundFixture(self)
            let view = SyncStatusView(
                library: try await inboundLibrary(
                    f, name: String(repeating: "Library ", count: 100)),
                coordinator: f.coordinator, projection: f.projection,
                checkpoint: f.base.service, sessionController: f.base.controller)
            let host = UIHostingController(
                rootView: view.environment(\.dynamicTypeSize, .accessibility5))
            host.loadViewIfNeeded()
            XCTAssertNotNil(host.view)
        }
    #endif
}

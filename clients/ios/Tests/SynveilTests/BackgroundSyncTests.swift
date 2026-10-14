import Foundation
import SwiftUI
import XCTest

@testable import Synveil

@MainActor
final class RefreshMemoryPolicy: BackgroundSyncPolicyStorage {
    var value = BackgroundSyncPolicy.disabled
    var writable = true
    func read() -> BackgroundSyncPolicy { value }
    func write(_ policy: BackgroundSyncPolicy) -> Bool {
        guard writable else { return false }
        value = policy
        return true
    }
}

@MainActor
final class RefreshNative: BackgroundRefreshScheduling {
    var registrations = 0
    var registrationAccepted = true
    var pending = false
    var failSubmission = false
    var submissions: [Date] = []
    var cancellations = 0
    var pendingGate: QueueGate?
    var handler: (@MainActor (BackgroundRefreshDelivery) -> Void)?
    func register(handler: @escaping @MainActor (BackgroundRefreshDelivery) -> Void) -> Bool {
        registrations += 1
        self.handler = handler
        return registrationAccepted
    }
    func hasPendingRequest() async -> Bool {
        if let pendingGate { await pendingGate.arrive() }
        return pending
    }
    func submit(earliest: Date) throws {
        if failSubmission { throw LibraryFailure.serverUnavailable }
        submissions.append(earliest)
        pending = true
    }
    func cancel() {
        cancellations += 1
        pending = false
    }
}

/// Completion can originate from a native expiration callback on any queue.
final class RefreshCompletions: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [Bool] = []
    private var waiters: [CheckedContinuation<Void, Never>] = []
    func append(_ value: Bool) {
        let pending = lock.withLock {
            values.append(value)
            let pending = waiters
            waiters.removeAll()
            return pending
        }
        for waiter in pending { waiter.resume() }
    }
    func wait() async {
        await withCheckedContinuation { continuation in
            let ready = lock.withLock {
                if values.isEmpty {
                    waiters.append(continuation)
                    return false
                }
                return true
            }
            if ready { continuation.resume() }
        }
    }
    func snapshot() -> [Bool] { lock.withLock { values } }
    func delivery() -> BackgroundRefreshDelivery {
        BackgroundRefreshDelivery { self.append($0) }
    }
}

final class RefreshMonotonicClock: @unchecked Sendable {
    private let lock = NSLock()
    private var value: TimeInterval = 0
    func read() -> TimeInterval { lock.withLock { value } }
    func advance(_ seconds: TimeInterval) { lock.withLock { value += seconds } }
}

@MainActor
func refreshIdentity() async throws -> BackgroundSyncIdentity {
    let scope = try await queueScope()
    return BackgroundSyncIdentity(
        LibraryRequestScope(session: try queueSession(scope), revision: 0))
}

@MainActor
func refreshPolicy() async throws -> RefreshMemoryPolicy {
    let policy = RefreshMemoryPolicy()
    policy.value.enabled = true
    policy.value.identity = try await refreshIdentity()
    policy.value.grant = UUID()
    return policy
}

@MainActor
final class RefreshWork: BackgroundSyncWorkProtocol {
    let session: LibraryRequestScope
    var selected: [LibraryId]
    var calls: [LibraryId] = []
    var verifications = 0
    var captureFailure: LibraryFailure?
    var results: [InboundSyncStopReason] = [.moreWork]
    var pages = 1
    var events = 1
    var confirmationAvailable = true
    var timeAdvance: (@MainActor () -> Void)?
    var verificationGate: QueueGate?
    init() async throws {
        let scope = try await queueScope()
        session = LibraryRequestScope(session: try queueSession(scope), revision: 0)
        selected = [scope.libraryId]
    }
    func capture() async throws -> LibraryRequestScope {
        if let captureFailure { throw captureFailure }
        return session
    }
    func validate(_ session: LibraryRequestScope) async throws {
        try Task.checkCancellation()
        guard session.sameSession(as: self.session) else { throw LibraryFailure.staleSession }
    }
    func libraries(session: LibraryRequestScope, after: String?) async throws -> [LibraryId] {
        selected
    }
    func verify(session: LibraryRequestScope, library: LibraryId) async throws {
        verifications += 1
        if let verificationGate { await verificationGate.arrive() }
    }
    func synchronize(session: LibraryRequestScope, library: LibraryId) async -> InboundSyncRunResult
    {
        calls.append(library)
        timeAdvance?()
        let reason = results.isEmpty ? .upToDate : results.removeFirst()
        let position = try! feedPosition(from: "1")
        return InboundSyncRunResult(
            reason: reason,
            progress: InboundSyncProgress(
                phase: .stopped, pagesProcessed: pages, eventsApplied: events,
                locallyApplied: position, serverConfirmed: position, observedHighWatermark: position
            ),
            recovery: nil)
    }
    func confirmation(session: LibraryRequestScope, library: LibraryId) async throws
        -> BackgroundSyncConfirmation?
    {
        confirmationAvailable
            ? BackgroundSyncConfirmation(
                library: library, recordedAt: Date(timeIntervalSince1970: 100),
                position: try feedPosition(from: "1")) : nil
    }
}

@MainActor
final class BackgroundSyncTests: XCTestCase {
    func testMissingCorruptUnsupportedAndOversizedPreferencesAreDisabled() throws {
        let name = "p048-tests-\(UUID())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let store = BackgroundSyncPolicyStore(defaults: defaults)
        XCTAssertEqual(store.read(), .disabled)
        for bytes in [
            Data("invalid".utf8), Data("{\"version\":99}".utf8), Data(repeating: 1, count: 4097),
        ] {
            defaults.set(bytes, forKey: BackgroundSyncPolicyStore.key)
            XCTAssertEqual(store.read(), .disabled)
        }
    }
    func testPolicyRoundTripAndInvalidIdentityCannotEnable() async throws {
        let name = "p048-tests-\(UUID())"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let store = BackgroundSyncPolicyStore(defaults: defaults)
        let policy = try await refreshPolicy().value
        XCTAssertTrue(store.write(policy))
        XCTAssertEqual(store.read(), policy)
        var invalid = policy
        invalid.identity = nil
        XCTAssertFalse(store.write(invalid))
        invalid = policy
        invalid.afterLibrary = "../../private"
        XCTAssertFalse(store.write(invalid))
    }
    func testRegistrationOccursOnlyOnceIncludingDeniedRegistration() {
        let native = RefreshNative()
        native.registrationAccepted = false
        let scheduler = makeScheduler(native: native)
        scheduler.register()
        scheduler.register()
        XCTAssertEqual(native.registrations, 1)
        XCTAssertEqual(scheduler.state, .deferred)
    }
    func testDefaultOffNeverSchedulesOrExecutes() async {
        let native = RefreshNative()
        let scheduler = makeScheduler(native: native)
        var executions = 0
        scheduler.install(
            operation: { _ in
                executions += 1
                return BackgroundSyncOutcome(state: .completed)
            },
            identity: { try await refreshIdentity() }, savedConfirmation: { nil })
        scheduler.register()
        await scheduler.schedule()
        let completion = RefreshCompletions()
        scheduler.deliver(completion.delivery())
        XCTAssertEqual(executions, 0)
        XCTAssertTrue(native.submissions.isEmpty)
        XCTAssertEqual(completion.snapshot(), [false])
    }
    func testExplicitEnableSchedulesLowerBoundaryAndDisableCancels() async {
        let policy = RefreshMemoryPolicy()
        let native = RefreshNative()
        let scheduler = makeScheduler(policy: policy, native: native)
        install(scheduler)
        scheduler.register()
        await scheduler.setEnabled(true)
        XCTAssertTrue(policy.value.enabled)
        XCTAssertEqual(native.submissions, [Date(timeIntervalSince1970: 100 + 21600)])
        await scheduler.setEnabled(false)
        XCTAssertFalse(policy.value.enabled)
        XCTAssertFalse(native.pending)
        XCTAssertEqual(scheduler.state, .disabled)
    }
    func testPendingRequestAndRepeatedLaunchDoNotDuplicateSubmission() async throws {
        let policy = try await refreshPolicy()
        let native = RefreshNative()
        let scheduler = makeScheduler(policy: policy, native: native)
        install(scheduler)
        scheduler.register()
        await scheduler.schedule()
        await scheduler.schedule()
        let relaunched = makeScheduler(policy: policy, native: native)
        install(relaunched)
        relaunched.register()
        await relaunched.schedule()
        XCTAssertEqual(native.submissions.count, 1)
    }
    func testDisableRemainsEffectiveWhenPreferencePersistenceFails() async throws {
        let policy = try await refreshPolicy()
        let scheduler = makeScheduler(policy: policy)
        var executions = 0
        scheduler.install(
            operation: { _ in
                executions += 1
                return BackgroundSyncOutcome(state: .completed)
            },
            identity: { try await refreshIdentity() }, savedConfirmation: { nil })
        scheduler.register()
        policy.writable = false
        await scheduler.setEnabled(false)
        let completion = RefreshCompletions()
        scheduler.deliver(completion.delivery())
        XCTAssertEqual(completion.snapshot(), [false])
        XCTAssertEqual(executions, 0)
        XCTAssertFalse(scheduler.settings.enabled)
        XCTAssertEqual(scheduler.state, .failed)
    }
    func testSubmissionFailureHasPersistentCooldownAcrossLaunches() async throws {
        let policy = try await refreshPolicy()
        let native = RefreshNative()
        native.failSubmission = true
        let scheduler = makeScheduler(policy: policy, native: native)
        install(scheduler)
        scheduler.register()
        await scheduler.schedule()
        XCTAssertEqual(scheduler.state, .failed)
        native.failSubmission = false
        let relaunched = makeScheduler(policy: policy, native: native)
        install(relaunched)
        relaunched.register()
        await relaunched.schedule()
        XCTAssertTrue(native.submissions.isEmpty)
    }
    func testDisabledSystemRefreshAndLowPowerDefer() async throws {
        for power in [true, false] {
            let policy = try await refreshPolicy()
            let native = RefreshNative()
            let scheduler = makeScheduler(
                policy: policy, native: native, available: power, lowPower: power)
            install(scheduler)
            scheduler.register()
            await scheduler.schedule()
            XCTAssertEqual(scheduler.state, .deferred)
            XCTAssertTrue(native.submissions.isEmpty)
        }
    }
    func testAccountChangeCannotBorrowOptInOrPublishPriorConfirmation() async throws {
        let policy = try await refreshPolicy()
        let native = RefreshNative()
        let scheduler = makeScheduler(policy: policy, native: native)
        scheduler.install(
            operation: { _ in BackgroundSyncOutcome(state: .completed) },
            identity: { throw LibraryFailure.staleSession }, savedConfirmation: { nil })
        scheduler.register()
        await scheduler.schedule()
        await scheduler.refreshStatus()
        XCTAssertEqual(scheduler.state, .authenticationRequired)
        XCTAssertTrue(native.submissions.isEmpty)
        XCTAssertNil(scheduler.lastConfirmation)
    }
    func testDisableDuringPendingRequestInspectionCannotSubmitLate() async throws {
        let policy = try await refreshPolicy()
        let native = RefreshNative()
        let gate = QueueGate()
        native.pendingGate = gate
        let scheduler = makeScheduler(policy: policy, native: native)
        install(scheduler)
        scheduler.register()
        let attempt = Task { await scheduler.schedule() }
        await gate.wait()
        await scheduler.setEnabled(false)
        await gate.release()
        await attempt.value
        XCTAssertTrue(native.submissions.isEmpty)
        XCTAssertEqual(scheduler.state, .disabled)
    }
    func testExpirationBeforeWorkerAndRepeatedCompletionAreExactlyOnce() {
        let completion = RefreshCompletions()
        let delivery = completion.delivery()
        delivery.expire()
        delivery.expire()
        delivery.finish(success: true)
        XCTAssertEqual(completion.snapshot(), [false])
    }
    func testExpirationDuringDeliveryFinishesImmediatelyAndRetainsDuplicateFence() async throws {
        let policy = try await refreshPolicy()
        let scheduler = makeScheduler(policy: policy)
        let gate = QueueGate()
        scheduler.install(
            operation: { _ in
                await gate.arrive()
                return BackgroundSyncOutcome(state: .completed)
            },
            identity: { try await refreshIdentity() }, savedConfirmation: { nil })
        scheduler.register()
        let first = RefreshCompletions()
        let delivery = first.delivery()
        scheduler.deliver(delivery)
        await gate.wait()
        scheduler.deliver(delivery)
        delivery.expire()
        XCTAssertEqual(first.snapshot(), [false])
        let duplicate = RefreshCompletions()
        scheduler.deliver(duplicate.delivery())
        XCTAssertEqual(duplicate.snapshot(), [false])
        await gate.release()
        await scheduler.waitForCurrentDelivery()
        XCTAssertEqual(first.snapshot(), [false])
        XCTAssertEqual(scheduler.state, .expired)
    }
    func testSessionInvalidationFencesLateSuccess() async throws {
        let policy = try await refreshPolicy()
        let scheduler = makeScheduler(policy: policy)
        let gate = QueueGate()
        scheduler.install(
            operation: { _ in
                await gate.arrive()
                return BackgroundSyncOutcome(state: .completed)
            },
            identity: { try await refreshIdentity() }, savedConfirmation: { nil })
        scheduler.register()
        let completion = RefreshCompletions()
        scheduler.deliver(completion.delivery())
        await gate.wait()
        scheduler.invalidate(disable: true)
        await gate.release()
        await scheduler.waitForCurrentDelivery()
        XCTAssertEqual(completion.snapshot(), [false])
        XCTAssertEqual(scheduler.state, .disabled)
        XCTAssertNil(scheduler.lastConfirmation)
    }
    func testDeadlineWatchdogUsesInjectedExpiration() async throws {
        let policy = try await refreshPolicy()
        let trigger = QueueGate()
        let gate = QueueGate()
        let scheduler = BGAppRefreshScheduler(
            policy: policy, native: RefreshNative(), available: { true }, lowPower: { false },
            deadlineWait: { _ in await trigger.arrive() })
        scheduler.install(
            operation: { _ in
                await gate.arrive()
                return BackgroundSyncOutcome(state: .completed)
            },
            identity: { try await refreshIdentity() }, savedConfirmation: { nil })
        scheduler.register()
        let completion = RefreshCompletions()
        scheduler.deliver(completion.delivery())
        await gate.wait()
        await trigger.wait()
        await trigger.release()
        await completion.wait()
        XCTAssertEqual(completion.snapshot(), [false])
        await gate.release()
        await scheduler.waitForCurrentDelivery()
    }
    func testCredentialReplacementWithoutLifecycleChangeCannotPublishSuccess() async throws {
        let policy = try await refreshPolicy()
        let scheduler = makeScheduler(policy: policy)
        let gate = QueueGate()
        var identity = try await refreshIdentity()
        let other = try await queueScope(owner: 93)
        let replacement = BackgroundSyncIdentity(
            LibraryRequestScope(session: try queueSession(other), revision: 0))
        scheduler.install(
            operation: { _ in
                await gate.arrive()
                return BackgroundSyncOutcome(state: .completed)
            },
            identity: { identity }, savedConfirmation: { nil })
        scheduler.register()
        let completion = RefreshCompletions()
        scheduler.deliver(completion.delivery())
        await gate.wait()
        identity = replacement
        await gate.release()
        await scheduler.waitForCurrentDelivery()
        XCTAssertEqual(completion.snapshot(), [false])
        XCTAssertNil(scheduler.lastConfirmation)
        XCTAssertFalse(policy.value.enabled)
    }
    func testZeroProgressDeferralDoesNotClaimSuccessfulTask() {
        XCTAssertFalse(BackgroundSyncOutcome(state: .partial).taskSucceeded)
        XCTAssertFalse(BackgroundSyncOutcome(state: .offline, pages: 1).taskSucceeded)
        XCTAssertTrue(BackgroundSyncOutcome(state: .partial, pages: 1).taskSucceeded)
    }
    func testColdStartupConsumesTheDeliveredBudgetBeforeDispatch() async throws {
        let policy = try await refreshPolicy()
        let work = try await RefreshWork()
        let clock = RefreshMonotonicClock()
        let completion = RefreshCompletions()
        let delivery = BackgroundRefreshDelivery(uptime: { clock.read() }) { completion.append($0) }
        clock.advance(20)
        let result = await BackgroundSyncCoordinator(
            policy: policy, work: work,
            gate: BackgroundSyncRequestGate(conditionsAllow: { true })
        )
        .run(delivery: delivery)
        XCTAssertEqual(result.state, .deferred)
        XCTAssertEqual(work.verifications, 0)
        XCTAssertTrue(work.calls.isEmpty)
    }
    func testUncertainAndProtocolFailuresAreClassifiedWithoutRecoveryCalls() async throws {
        for (reason, expected) in [
            (InboundSyncStopReason.awaitingAckRecovery, BackgroundSyncState.recoveryRequired),
            (.rebaselineRequired, .recoveryRequired), (.protocolFailure, .failed),
            (.serverUnavailable, .offline), (.authenticationRequired, .authenticationRequired),
        ] {
            let policy = try await refreshPolicy()
            let work = try await RefreshWork()
            work.results = [reason]
            work.pages = 0
            let gate = BackgroundSyncRequestGate(conditionsAllow: { true })
            let result = await BackgroundSyncCoordinator(policy: policy, work: work, gate: gate)
                .run(delivery: RefreshCompletions().delivery())
            XCTAssertEqual(result.state, expected)
            XCTAssertFalse(result.taskSucceeded)
        }
    }
    func testLibraryAndRuntimeBudgetsStopAdditionalWork() async throws {
        let policy = try await refreshPolicy()
        let work = try await RefreshWork()
        let second = try await LibraryId.validated(queueUUID(71), using: QueueValidator())
        work.selected.append(second)
        var uptime: TimeInterval = 0
        work.timeAdvance = { uptime = BackgroundSyncLimits.runtime }
        let gate = BackgroundSyncRequestGate(uptime: { uptime }, conditionsAllow: { true })
        let result = await BackgroundSyncCoordinator(policy: policy, work: work, gate: gate)
            .run(delivery: RefreshCompletions().delivery())
        XCTAssertEqual(work.calls.count, 1)
        XCTAssertEqual(result.state, .partial)
        XCTAssertEqual(policy.value.afterLibrary, work.selected.first?.rawValue)
    }
    func testPersistentLibraryRecoveryDoesNotStarveNextSelectedLibrary() async throws {
        let policy = try await refreshPolicy()
        let work = try await RefreshWork()
        work.selected.append(try await LibraryId.validated(queueUUID(71), using: QueueValidator()))
        work.results = [.awaitingAckRecovery, .upToDate]
        work.pages = 0
        let result = await BackgroundSyncCoordinator(
            policy: policy, work: work, gate: BackgroundSyncRequestGate(conditionsAllow: { true })
        )
        .run(delivery: RefreshCompletions().delivery())
        XCTAssertEqual(work.calls, work.selected)
        XCTAssertEqual(result.state, .recoveryRequired)
        XCTAssertEqual(policy.value.afterLibrary, work.selected.last?.rawValue)
    }
    func testMissingDurableConfirmationCannotBecomeSuccess() async throws {
        let policy = try await refreshPolicy()
        let work = try await RefreshWork()
        work.confirmationAvailable = false
        let result = await BackgroundSyncCoordinator(
            policy: policy, work: work, gate: BackgroundSyncRequestGate(conditionsAllow: { true })
        )
        .run(delivery: RefreshCompletions().delivery())
        XCTAssertEqual(result.state, .failed)
        XCTAssertFalse(result.taskSucceeded)
    }
    func testExcessiveOrDuplicateLibrarySelectionMakesNoVerificationOrFeed() async throws {
        for duplicate in [false, true] {
            let policy = try await refreshPolicy()
            let work = try await RefreshWork()
            if duplicate {
                work.selected.append(work.selected[0])
            } else {
                work.selected += [
                    try await LibraryId.validated(queueUUID(71), using: QueueValidator()),
                    try await LibraryId.validated(queueUUID(72), using: QueueValidator()),
                ]
            }
            let result = await BackgroundSyncCoordinator(
                policy: policy, work: work,
                gate: BackgroundSyncRequestGate(conditionsAllow: { true })
            )
            .run(delivery: RefreshCompletions().delivery())
            XCTAssertEqual(result.state, .failed)
            XCTAssertEqual(work.verifications, 0)
            XCTAssertTrue(work.calls.isEmpty)
        }
    }
    func testInaccessiblePreferenceWriteCannotDispatchVerification() async throws {
        let policy = try await refreshPolicy()
        policy.writable = false
        let work = try await RefreshWork()
        let result = await BackgroundSyncCoordinator(
            policy: policy, work: work, gate: BackgroundSyncRequestGate(conditionsAllow: { true })
        )
        .run(delivery: RefreshCompletions().delivery())
        XCTAssertEqual(result.state, .failed)
        XCTAssertEqual(work.verifications, 0)
        XCTAssertTrue(work.calls.isEmpty)
    }
    func testNetworkPolicyChangeIsCheckedBeforeEveryRequest() async throws {
        let policy = try await refreshPolicy()
        let wifi = InboundWire()
        let cellular = InboundWire()
        let response = try queueHTTP(["data": []])
        await wifi.configure([response])
        await cellular.configure([response])
        var constrained = false
        let gate = BackgroundSyncRequestGate(conditionsAllow: { !constrained })
        gate.open { true }
        let transport = BackgroundSyncHTTPTransport(
            policy: policy, gate: gate, unmetered: wifi, cellular: cellular)
        let request = HTTPTransportRequest(
            url: URL(string: "https://example.com/api/v1/libraries")!)
        _ = try await transport.send(request)
        policy.value.network = .cellular
        _ = try await transport.send(request)
        constrained = true
        do {
            _ = try await transport.send(request)
            XCTFail("Constrained path must defer")
        } catch {}
        let wifiRequests = await wifi.requests()
        let cellularRequests = await cellular.requests()
        XCTAssertEqual(wifiRequests.count, 1)
        XCTAssertEqual(cellularRequests.count, 1)
    }
    func testTransportRejectsFileTransfersMutationAndRebaseline() async throws {
        let policy = try await refreshPolicy()
        let wire = InboundWire()
        let gate = BackgroundSyncRequestGate(conditionsAllow: { true })
        gate.open { true }
        let transport = BackgroundSyncHTTPTransport(
            policy: policy, gate: gate, unmetered: wire, cellular: wire)
        for suffix in ["/upload-sessions", "/versions/1/content", "/mutations", "/rebaseline"] {
            do {
                _ = try await transport.send(
                    HTTPTransportRequest(url: URL(string: "https://example.com/api/v1" + suffix)!))
                XCTFail("Forbidden operation")
            } catch {}
        }
        let requests = await wire.requests()
        XCTAssertTrue(requests.isEmpty)
    }
    func testSettingsRenderDynamicTypeAndTruthfulEmptyHistory() {
        let scheduler = makeScheduler()
        let view = BackgroundSyncSettingsView(scheduler: scheduler)
        let host = UIHostingController(
            rootView: view.environment(\.dynamicTypeSize, .accessibility5))
        host.loadViewIfNeeded()
        XCTAssertNotNil(host.view)
        XCTAssertEqual(view.message, "Background Sync is off.")
        XCTAssertNil(scheduler.lastConfirmation)
    }

    private func makeScheduler(
        policy: RefreshMemoryPolicy = RefreshMemoryPolicy(),
        native: RefreshNative = RefreshNative(),
        available: Bool = true, lowPower: Bool = false
    ) -> BGAppRefreshScheduler {
        BGAppRefreshScheduler(
            policy: policy, native: native, clock: { Date(timeIntervalSince1970: 100) },
            available: { available }, lowPower: { lowPower })
    }
    private func install(_ scheduler: BGAppRefreshScheduler) {
        scheduler.install(
            operation: { _ in BackgroundSyncOutcome(state: .completed) },
            identity: { try await refreshIdentity() }, savedConfirmation: { nil })
    }
}

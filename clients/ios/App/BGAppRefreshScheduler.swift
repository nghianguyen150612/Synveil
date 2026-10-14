import BackgroundTasks
import Foundation
import OSLog
import Observation
import UIKit

/// Completion and expiration are fenced synchronously, even before the MainActor worker starts.
/// The lock protects only this delivery's lifecycle; SQLite owns synchronization exclusion.
final class BackgroundRefreshDelivery: @unchecked Sendable {
    private let lock = NSLock()
    private var expired = false
    private var completed = false
    private var worker: Task<Void, Never>?
    private let completion: (Bool) -> Void
    init(completion: @escaping (Bool) -> Void) { self.completion = completion }
    var isExpired: Bool { lock.withLock { expired } }
    func install(_ task: Task<Void, Never>) {
        lock.withLock {
            worker = task
            if expired { task.cancel() }
        }
    }
    func expire() {
        lock.withLock {
            expired = true
            worker?.cancel()
        }
        finish(success: false)
    }
    func finish(success: Bool) {
        let result: Bool? = lock.withLock {
            guard !completed else { return nil }
            completed = true
            worker = nil
            return success && !expired
        }
        if let result { completion(result) }
    }
}

@MainActor
protocol BackgroundRefreshScheduling {
    func register(handler: @escaping @MainActor (BackgroundRefreshDelivery) -> Void) -> Bool
    func hasPendingRequest() async -> Bool
    func submit(earliest: Date) throws
    func cancel()
}

@MainActor
final class NativeBackgroundRefreshScheduler: BackgroundRefreshScheduling {
    func register(handler: @escaping @MainActor (BackgroundRefreshDelivery) -> Void) -> Bool {
        BGTaskScheduler.shared.register(
            forTaskWithIdentifier: BackgroundSyncLimits.identifier, using: .main
        ) { task in
            MainActor.assumeIsolated {
                guard let refresh = task as? BGAppRefreshTask else {
                    task.setTaskCompleted(success: false)
                    return
                }
                let delivery = BackgroundRefreshDelivery { refresh.setTaskCompleted(success: $0) }
                refresh.expirationHandler = { delivery.expire() }
                handler(delivery)
            }
        }
    }
    func hasPendingRequest() async -> Bool {
        await withCheckedContinuation { continuation in
            BGTaskScheduler.shared.getPendingTaskRequests { requests in
                continuation.resume(
                    returning: requests.contains {
                        $0.identifier == BackgroundSyncLimits.identifier
                    })
            }
        }
    }
    func submit(earliest: Date) throws {
        let request = BGAppRefreshTaskRequest(identifier: BackgroundSyncLimits.identifier)
        request.earliestBeginDate = earliest
        try BGTaskScheduler.shared.submit(request)
    }
    func cancel() {
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: BackgroundSyncLimits.identifier)
    }
}

@Observable
@MainActor
final class BGAppRefreshScheduler {
    static let shared = BGAppRefreshScheduler()
    private static let logger = Logger(subsystem: "com.synveil.ios", category: "metadata-refresh")
    let policy: any BackgroundSyncPolicyStorage
    private let native: any BackgroundRefreshScheduling
    private let clock: @MainActor () -> Date
    private let available: @MainActor () -> Bool
    private let lowPower: @MainActor () -> Bool
    private let deadlineWait: @Sendable () async throws -> Void
    private var registrationAttempted = false
    private var registered = false
    private var scheduling = false
    private var schedulingRevision: UInt64 = 0
    private var active: BackgroundRefreshDelivery?
    private var activeWorker: Task<Void, Never>?
    private var operation: (@MainActor (BackgroundRefreshDelivery) async -> BackgroundSyncOutcome)?
    private var identity: (@MainActor () async throws -> BackgroundSyncIdentity)?
    private var optInIdentity: (@MainActor () async throws -> BackgroundSyncIdentity)?
    private var savedConfirmation: (@MainActor () async throws -> BackgroundSyncConfirmation?)?
    private var recoveryInspection: (@MainActor () async throws -> Bool)?
    private(set) var state: BackgroundSyncState = .disabled
    private(set) var lastConfirmation: BackgroundSyncConfirmation?
    private(set) var settings = BackgroundSyncPolicy.disabled
    private(set) var manualRecoveryRequired = false
    var systemAllowsExecution: Bool { available() && !lowPower() }

    init(
        policy: any BackgroundSyncPolicyStorage = BackgroundSyncPolicyStore(),
        native: any BackgroundRefreshScheduling = NativeBackgroundRefreshScheduler(),
        clock: @escaping @MainActor () -> Date = { Date() },
        available: @escaping @MainActor () -> Bool = {
            UIApplication.shared.backgroundRefreshStatus == .available
        },
        lowPower: @escaping @MainActor () -> Bool = {
            ProcessInfo.processInfo.isLowPowerModeEnabled
        },
        deadlineWait: @escaping @Sendable () async throws -> Void = {
            try await Task.sleep(for: .seconds(BackgroundSyncLimits.runtime))
        }
    ) {
        self.policy = policy
        self.native = native
        self.clock = clock
        self.available = available
        self.lowPower = lowPower
        self.deadlineWait = deadlineWait
        settings = policy.read()
    }
    func install(
        operation: @escaping @MainActor (BackgroundRefreshDelivery) async -> BackgroundSyncOutcome,
        identity: @escaping @MainActor () async throws -> BackgroundSyncIdentity,
        savedConfirmation: @escaping @MainActor () async throws -> BackgroundSyncConfirmation?,
        optInIdentity: (@MainActor () async throws -> BackgroundSyncIdentity)? = nil,
        recoveryInspection: (@MainActor () async throws -> Bool)? = nil
    ) {
        guard self.operation == nil else { return }
        self.operation = operation
        self.identity = identity
        self.optInIdentity = optInIdentity ?? identity
        self.savedConfirmation = savedConfirmation
        self.recoveryInspection = recoveryInspection
    }
    func register() {
        guard !registrationAttempted else { return }
        registrationAttempted = true
        registered = native.register { [weak self] in self?.deliver($0) }
        Self.logger.info("registration accepted=\(self.registered)")
        if !registered { state = .deferred }
    }
    func schedule() async {
        guard !scheduling else { return }
        scheduling = true
        defer { scheduling = false }
        let revision = schedulingRevision
        let saved = policy.read()
        settings = saved
        guard saved.enabled else {
            native.cancel()
            state = .disabled
            return
        }
        guard registered, available(), !lowPower() else {
            native.cancel()
            if active == nil { state = .deferred }
            Self.logger.info("scheduling deferred by system policy")
            return
        }
        guard let identity, let current = try? await identity(), current == saved.identity else {
            native.cancel()
            if active == nil { state = .authenticationRequired }
            return
        }
        let pending = await native.hasPendingRequest()
        guard revision == schedulingRevision, policy.read().enabled,
            policy.read().grant == saved.grant, available(), !lowPower()
        else { return }
        if pending {
            if active == nil { state = .scheduled }
            return
        }
        guard saved.nextSubmissionAllowedAt.map({ $0 <= clock() }) ?? true else {
            if active == nil { state = .deferred }
            return
        }
        var next = policy.read()
        next.nextSubmissionAllowedAt = clock().addingTimeInterval(
            BackgroundSyncLimits.schedulingDelay)
        guard policy.write(next) else {
            state = .failed
            return
        }
        do {
            try native.submit(earliest: next.nextSubmissionAllowedAt!)
            if active == nil { state = .scheduled }
            Self.logger.info("scheduling submitted; delivery is system controlled")
        } catch {
            if active == nil { state = .failed }
            Self.logger.error("scheduling submission rejected")
        }
        settings = policy.read()
    }
    func setEnabled(_ enabled: Bool) async {
        if !enabled {
            invalidate(disable: true)
            return
        }
        let revision = schedulingRevision
        guard let optInIdentity, let current = try? await optInIdentity(), current.isValid,
            revision == schedulingRevision
        else {
            state = .authenticationRequired
            return
        }
        var next = policy.read()
        next.enabled = true
        next.identity = current
        next.grant = UUID()
        next.afterLibrary = nil
        next.nextSubmissionAllowedAt = nil
        guard policy.write(next) else {
            state = .failed
            return
        }
        settings = next
        state = .eligible
        await schedule()
    }
    func setNetwork(_ network: BackgroundSyncNetworkPolicy) async {
        active?.expire()
        schedulingRevision &+= 1
        native.cancel()
        var next = policy.read()
        next.network = network
        if next.enabled { next.grant = UUID() }
        // Keep the cooldown; changing a preference is not permission for aggressive submission.
        guard policy.write(next) else {
            state = .failed
            return
        }
        settings = next
        await schedule()
    }
    func invalidate(disable: Bool = false) {
        schedulingRevision &+= 1
        active?.expire()
        native.cancel()
        lastConfirmation = nil
        manualRecoveryRequired = false
        if disable { _ = policy.write(.disabled) }
        settings = policy.read()
        state = disable ? .disabled : .invalidated
    }
    func refreshStatus() async {
        settings = policy.read()
        guard let identity, let current = try? await identity(),
            !settings.enabled || current == settings.identity
        else {
            lastConfirmation = nil
            state = .authenticationRequired
            return
        }
        let revision = schedulingRevision
        let confirmation = try? await savedConfirmation?()
        let recovery = try? await recoveryInspection?()
        guard revision == schedulingRevision else { return }
        lastConfirmation = confirmation ?? nil
        manualRecoveryRequired = recovery ?? false
        if !settings.enabled {
            state = .disabled
        } else if !available() || lowPower() || !registered {
            state = .deferred
        }
    }
    func deliver(_ delivery: BackgroundRefreshDelivery) {
        if active === delivery { return }
        guard active == nil, !delivery.isExpired, policy.read().enabled,
            available(), !lowPower(), let operation
        else {
            delivery.finish(success: false)
            if active == nil { state = policy.read().enabled ? .deferred : .disabled }
            return
        }
        active = delivery
        state = .running
        let revision = schedulingRevision
        Self.logger.info("execution started")
        let worker = Task { @MainActor [weak self] in
            guard let self else {
                delivery.finish(success: false)
                return
            }
            let watchdog = Task { [deadlineWait] in
                do {
                    try await deadlineWait()
                    delivery.expire()
                } catch {}
            }
            let result = await operation(delivery)
            watchdog.cancel()
            self.active = nil
            guard revision == self.schedulingRevision else {
                delivery.finish(success: false)
                return
            }
            let finalState: BackgroundSyncState = delivery.isExpired ? .expired : result.state
            self.state = finalState
            if !delivery.isExpired {
                self.lastConfirmation = result.confirmation ?? self.lastConfirmation
            }
            Self.logger.info(
                "execution outcome=\(finalState.rawValue, privacy: .public) pages=\(result.pages) events=\(result.events)"
            )
            delivery.finish(success: result.taskSucceeded && !delivery.isExpired)
            // Submission is independent of the reported execution outcome and has the same cooldown.
            let executionState = self.state
            await self.schedule()
            if self.state == .scheduled || self.state == .deferred { self.state = executionState }
        }
        activeWorker = worker
        delivery.install(worker)
    }
    /// Injectable task delivery tests await the real worker's cleanup rather than polling yields.
    func waitForCurrentDelivery() async { await activeWorker?.value }
}

import Foundation
import Observation

/// Noncritical presentation owner. All scheduling failures are local, typed and redacted.
@Observable
@MainActor
final class NotificationCoordinator {
    private let scheduler: any LocalNotificationScheduling
    private let storage: any NotificationPolicyStorage
    private let ledger: any NotificationLedgerStorage
    let authority: any NotificationAuthorityProtocol
    let navigation: NotificationNavigationCoordinator
    private let clock: @MainActor () -> Date
    private let revision: @MainActor () -> UInt64
    private(set) var policy: NotificationPolicy
    private(set) var authorization: NotificationAuthorization = .unavailable
    private(set) var failure: NotificationFailure?
    private(set) var foregroundObservations = 0
    private var scheduling = false

    init(
        scheduler: any LocalNotificationScheduling, policy: any NotificationPolicyStorage,
        ledger: any NotificationLedgerStorage, authority: any NotificationAuthorityProtocol,
        navigation: NotificationNavigationCoordinator,
        revision: @escaping @MainActor () -> UInt64,
        clock: @escaping @MainActor () -> Date = Date.init
    ) {
        self.scheduler = scheduler
        storage = policy
        self.ledger = ledger
        self.authority = authority
        self.navigation = navigation
        self.revision = revision
        self.clock = clock
        self.policy = policy.read()
    }
    func refreshStatus() async { authorization = await scheduler.authorization() }

    /// Called only by the explicit Settings toggle. Categories retain their independent choices.
    func setEnabled(_ enabled: Bool) async {
        if !enabled {
            invalidateSession()
            return
        }
        let captured = revision()
        do {
            let scope = try await authority.currentScope()
            await refreshStatus()
            if authorization == .notDetermined {
                try await scheduler.requestAuthorization()
                await refreshStatus()
            }
            guard revision() == captured else { throw NotificationFailure.sessionInvalidated }
            var next = storage.read()
            next.enabled = true
            next.scopeFingerprint = scope
            guard storage.write(next) else { throw NotificationFailure.storageFailure }
            policy = next
            failure = authorization == .authorized ? nil : .authorizationDenied
        } catch { failure = safeFailure(error) }
    }
    func setCategory(_ category: LocalNotificationCategory, enabled: Bool) {
        var next = storage.read()
        if enabled { next.categories.insert(category) } else { next.categories.remove(category) }
        guard storage.write(next) else {
            failure = .storageFailure
            return
        }
        policy = next
        if !enabled {
            Task {
                do {
                    let events = try await ledger.events().filter {
                        $0.candidate.category == category
                    }
                    for event in events {
                        _ = try await ledger.transition(
                            event.id, from: event.state, to: .invalidated)
                    }
                    scheduler.remove(events.map(\.requestIdentifier))
                } catch { failure = .storageFailure }
            }
        }
    }
    /// The caller supplies a synchronous post-commit observation; system work is never awaited by
    /// a transfer commit, background URLSession completion or BGAppRefresh handler.
    func enqueue(_ observation: @escaping @MainActor () async throws -> NotificationCandidate?) {
        let saved = storage.read()
        let captured = revision()
        guard saved.enabled else { return }
        Task {
            do {
                guard let candidate = try await observation(), current(saved, captured) else {
                    return
                }
                await observe(
                    candidate, expectedGeneration: saved.generation, expectedRevision: captured)
            } catch { failure = safeFailure(error) }
        }
    }
    func observe(
        _ candidate: NotificationCandidate, expectedGeneration: UUID? = nil,
        expectedRevision: UInt64? = nil
    ) async {
        let saved = storage.read()
        let captured = expectedRevision ?? revision()
        guard saved.enabled else {
            failure = .userDisabled
            return
        }
        guard saved.categories.contains(candidate.category) else {
            failure = .categoryDisabled
            return
        }
        guard expectedGeneration == nil || expectedGeneration == saved.generation,
            current(saved, captured), candidate.scopeFingerprint == saved.scopeFingerprint
        else {
            failure = .sessionInvalidated
            return
        }
        do {
            guard candidate.isValid, try await authority.verify(candidate), current(saved, captured)
            else {
                throw NotificationFailure.durableEventUnavailable
            }
            let now = clock()
            _ = try await ledger.insert(
                NotificationEvent(
                    id: UUID(), candidate: candidate, generation: saved.generation,
                    lifecycleRevision: captured, recordedAt: now,
                    expiresAt: now.addingTimeInterval(NotificationLimits.routingLifetime),
                    state: .eligible))
            await reconcile()
        } catch { failure = safeFailure(error) }
    }
    /// Relaunch retries only unclaimed eligibility. A claimed request is never retried, even when
    /// iOS removed it or the process died before the scheduling receipt was stored.
    func reconcile() async {
        guard !scheduling else { return }
        scheduling = true
        defer { scheduling = false }
        let saved = storage.read()
        let captured = revision()
        guard saved.enabled else { return }
        do {
            let events = try await ledger.events()
            for event in events where event.state == .eligible {
                guard current(saved, captured), event.generation == saved.generation,
                    event.candidate.scopeFingerprint == saved.scopeFingerprint,
                    saved.categories.contains(event.candidate.category), clock() < event.expiresAt
                else { continue }
                await refreshStatus()
                guard authorization == .authorized else {
                    failure = .authorizationDenied
                    return
                }
                guard try await authority.verify(event.candidate), current(saved, captured) else {
                    continue
                }
                let pending = await scheduler.pendingIdentifiers().filter {
                    $0.hasPrefix(NotificationLimits.prefix)
                }
                guard current(saved, captured) else { return }
                guard pending.count < NotificationLimits.maximumPending else {
                    failure = .capacity
                    return
                }
                // Durable claim BEFORE the nontransactional system call. A lost write receipt
                // fails closed. No operation evidence changes if any step below fails.
                guard try await ledger.claim(event.id, now: clock()) else {
                    continue
                }
                guard current(saved, captured) else { return }
                do {
                    try await scheduler.schedule(
                        LocalNotificationPayload(
                            handle: event.id, category: event.candidate.category))
                    guard current(saved, captured) else {
                        scheduler.remove([event.requestIdentifier])
                        return
                    }
                    _ = try await ledger.transition(event.id, from: .claimed, to: .scheduled)
                    failure = nil
                } catch { failure = .schedulingError }
            }
        } catch { failure = safeFailure(error) }
    }
    func invalidateSession() {
        navigation.invalidate()
        var next = storage.read()
        let retiredGeneration = next.generation
        next.enabled = false
        next.scopeFingerprint = nil
        next.generation = UUID()
        // Memory fences immediately even if preference storage fails.
        policy = next
        _ = storage.write(next)
        failure = .sessionInvalidated
        Task {
            let pending = await scheduler.pendingIdentifiers()
            if policy.generation == next.generation { scheduler.remove(pending) }
            do {
                scheduler.remove(try await ledger.invalidate(generation: retiredGeneration))
            } catch {
                failure = .storageFailure
            }
        }
    }
    func foregroundObserved() {
        foregroundObservations = min(foregroundObservations + 1, NotificationLimits.maximumRecords)
    }
    private func current(_ saved: NotificationPolicy, _ captured: UInt64) -> Bool {
        revision() == captured && policy.generation == saved.generation
            && storage.read() == saved && saved.enabled
    }
    private func safeFailure(_ error: Error) -> NotificationFailure {
        NotificationFailure.classify(error)
    }
}

extension NotificationFailure {
    static func classify(_ error: Error) -> NotificationFailure {
        if let failure = error as? NotificationFailure { return failure }
        if let failure = error as? LibraryFailure {
            switch failure {
            case .unauthenticated, .authenticationRejected, .deviceRevoked, .staleSession,
                .invalidCredential, .credentialUnavailable:
                return .sessionInvalidated
            case .originMismatch: return .accountMismatch
            default: return .durableEventUnavailable
            }
        }
        if let failure = error as? DownloadFailure {
            switch failure {
            case .unauthenticated, .deviceRevoked, .staleSession: return .sessionInvalidated
            case .scopeMismatch: return .accountMismatch
            default: return .durableEventUnavailable
            }
        }
        return .storageFailure
    }
}

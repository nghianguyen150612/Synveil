import Foundation
import Observation

@Observable
@MainActor
final class NotificationNavigationCoordinator {
    private struct Intent {
        let id: UUID
        let receivedAt: Date
        let fence: UUID
    }
    private let clock: @MainActor () -> Date
    private var pending: Intent?
    private var fence = UUID()
    private var resolving = false
    private(set) var destination: NotificationDestination?
    private(set) var selection: UUID?
    private(set) var failure: NotificationFailure?
    private(set) var intentRevision: UInt64 = 0
    init(clock: @escaping @MainActor () -> Date = Date.init) { self.clock = clock }
    func receive(_ id: UUID) {
        if pending?.id == id || selection == id { return }
        intentRevision &+= 1
        pending = Intent(id: id, receivedAt: clock(), fence: fence)
    }
    func clearDestination() {
        destination = nil
        selection = nil
    }
    func invalidate() {
        intentRevision &+= 1
        fence = UUID()
        pending = nil
        clearDestination()
        failure = .sessionInvalidated
    }
    func consume(
        ready: Bool, authenticated: Bool, mayAwaitAuthentication: Bool,
        policy: NotificationPolicy, ledger: any NotificationLedgerStorage,
        authority: any NotificationAuthorityProtocol
    ) async {
        guard !resolving, let intent = pending else { return }
        guard ready else {
            failure = .startupPending
            return
        }
        guard clock().timeIntervalSince(intent.receivedAt) < NotificationLimits.intentLifetime
        else {
            pending = nil
            failure = .invalidIntent
            return
        }
        guard authenticated else {
            if !mayAwaitAuthentication { pending = nil }
            failure = .startupPending
            return
        }
        resolving = true
        defer { resolving = false }
        do {
            guard let event = try await ledger.events().first(where: { $0.id == intent.id }) else {
                throw NotificationFailure.invalidIntent
            }
            if event.state == .handled {
                pending = nil
                failure = .alreadyHandled
                return
            }
            guard
                [.claimed, .scheduled].contains(event.state), clock() < event.expiresAt,
                event.generation == policy.generation, policy.enabled,
                event.candidate.scopeFingerprint == policy.scopeFingerprint
            else { throw NotificationFailure.invalidIntent }
            guard try await authority.currentScope() == event.candidate.scopeFingerprint else {
                throw NotificationFailure.accountMismatch
            }
            let resolved = try await authority.destination(event.candidate)
            guard fence == intent.fence, pending?.id == intent.id else { return }
            guard try await ledger.transition(event.id, from: event.state, to: .handled),
                fence == intent.fence, pending?.id == intent.id
            else { throw NotificationFailure.alreadyHandled }
            destination = resolved
            selection = intent.id
            pending = nil
            failure = nil
        } catch {
            guard fence == intent.fence, pending?.id == intent.id else { return }
            pending = nil
            failure = NotificationFailure.classify(error)
            destination = .unavailable
            selection = intent.id
        }
    }
}

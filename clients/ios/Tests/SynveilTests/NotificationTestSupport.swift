import Foundation
import XCTest

@testable import Synveil

@MainActor
final class FakeNotificationCenter: LocalNotificationScheduling {
    var status: NotificationAuthorization = .authorized
    var promptStatus: NotificationAuthorization = .authorized
    var prompts = 0
    var pending: [String] = []
    var payloads: [LocalNotificationPayload] = []
    var removed: [String] = []
    var scheduleFailure = false
    var authorizationFailure = false
    var onSchedule: (@MainActor () async -> Void)?
    func authorization() async -> NotificationAuthorization { status }
    func requestAuthorization() async throws {
        prompts += 1
        if authorizationFailure { throw NotificationFailure.authorizationDenied }
        status = promptStatus
    }
    func pendingIdentifiers() async -> [String] { pending }
    func schedule(_ payload: LocalNotificationPayload) async throws {
        payloads.append(payload)
        await onSchedule?()
        if scheduleFailure { throw NotificationFailure.schedulingError }
        pending.append(payload.identifier)
    }
    func remove(_ identifiers: [String]) {
        let owned = identifiers.filter { $0.hasPrefix(NotificationLimits.prefix) }
        removed += owned
        pending.removeAll { owned.contains($0) }
    }
}

@MainActor
final class FakeNotificationPolicy: NotificationPolicyStorage {
    var value = NotificationPolicy()
    var fails = false
    func read() -> NotificationPolicy { value }
    func write(_ policy: NotificationPolicy) -> Bool {
        if fails { return false }
        value = policy
        return true
    }
}

@MainActor
final class FakeNotificationAuthority: NotificationAuthorityProtocol {
    var scope = String(repeating: "a", count: 64)
    var valid = true
    var revoked = false
    var resolveCount = 0
    var verified = 0
    var resolved: NotificationDestination = .unavailable
    var onVerify: (@MainActor () async -> Void)?
    var onResolve: (@MainActor () async -> Void)?
    func currentScope() async throws -> String {
        if revoked { throw NotificationFailure.sessionInvalidated }
        return scope
    }
    func verify(_ candidate: NotificationCandidate) async throws -> Bool {
        verified += 1
        await onVerify?()
        if revoked { throw NotificationFailure.sessionInvalidated }
        return valid && candidate.scopeFingerprint == scope
    }
    func destination(_ candidate: NotificationCandidate) async throws -> NotificationDestination {
        resolveCount += 1
        await onResolve?()
        guard try await verify(candidate) else { throw NotificationFailure.durableEventUnavailable }
        return resolved
    }
}

@MainActor
final class NotificationTestClock {
    var now = Date(timeIntervalSince1970: 1_800_000_000)
    var revision: UInt64 = 1
}

@MainActor
struct NotificationFixture {
    let center: FakeNotificationCenter
    let policy: FakeNotificationPolicy
    let authority: FakeNotificationAuthority
    let ledger: NotificationDeliveryLedger
    let navigation: NotificationNavigationCoordinator
    let coordinator: NotificationCoordinator
    let clock: NotificationTestClock
    let url: URL
    func candidate(
        _ evidence: NotificationEvidence = .verifiedDownload, operation: UUID = UUID(),
        proof: String? = nil
    ) -> NotificationCandidate {
        NotificationCandidate(
            category: evidence.category, evidence: evidence,
            operation: operation.uuidString.lowercased(), library: queueUUID(90),
            scopeFingerprint: authority.scope,
            proofFingerprint: proof ?? NotificationFingerprint.hash([operation.uuidString]))
    }
    func event(_ candidate: NotificationCandidate? = nil) -> NotificationEvent {
        NotificationEvent(
            id: UUID(), candidate: candidate ?? self.candidate(),
            generation: policy.value.generation,
            lifecycleRevision: clock.revision, recordedAt: clock.now,
            expiresAt: clock.now.addingTimeInterval(NotificationLimits.routingLifetime),
            state: .eligible)
    }
    func consume(ready: Bool = true, authenticated: Bool = true, awaiting: Bool = false) async {
        await navigation.consume(
            ready: ready, authenticated: authenticated, mayAwaitAuthentication: awaiting,
            policy: coordinator.policy, ledger: ledger, authority: authority)
    }
}

@MainActor
func notificationFixture(_ test: XCTestCase, enabled: Bool = true) throws -> NotificationFixture {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(
        "SynveilNotifications-" + UUID().uuidString, isDirectory: true)
    test.addTeardownBlock { try? FileManager.default.removeItem(at: root) }
    let url = root.appendingPathComponent("delivery.json")
    let ledger = try NotificationDeliveryLedger(url: url)
    let center = FakeNotificationCenter()
    let policy = FakeNotificationPolicy()
    let authority = FakeNotificationAuthority()
    let clock = NotificationTestClock()
    policy.value.enabled = enabled
    policy.value.scopeFingerprint = enabled ? authority.scope : nil
    policy.value.categories = enabled ? Set(LocalNotificationCategory.allCases) : []
    let navigation = NotificationNavigationCoordinator(clock: { clock.now })
    let coordinator = NotificationCoordinator(
        scheduler: center, policy: policy, ledger: ledger, authority: authority,
        navigation: navigation, revision: { clock.revision }, clock: { clock.now })
    return NotificationFixture(
        center: center, policy: policy, authority: authority, ledger: ledger,
        navigation: navigation, coordinator: coordinator, clock: clock, url: url)
}

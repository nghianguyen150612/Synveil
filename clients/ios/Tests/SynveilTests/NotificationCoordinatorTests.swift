import Foundation
import XCTest

@testable import Synveil

@MainActor
final class NotificationCoordinatorTests: XCTestCase {
    func testFreshPolicyAndAllCategoriesDefaultOff() {
        let defaults = UserDefaults(suiteName: "SynveilNotifications-" + UUID().uuidString)!
        let store = NotificationPolicyStore(defaults: defaults)
        XCTAssertFalse(store.read().enabled)
        XCTAssertTrue(store.read().categories.isEmpty)
    }
    func testStartupNeverPrompts() async throws {
        let f = try notificationFixture(self, enabled: false)
        f.center.status = .notDetermined
        await f.coordinator.refreshStatus()
        await f.coordinator.reconcile()
        XCTAssertEqual(f.center.prompts, 0)
    }
    func testExplicitOptInRequestsPermissionWithoutEnablingCategories() async throws {
        let f = try notificationFixture(self, enabled: false)
        f.center.status = .notDetermined
        await f.coordinator.setEnabled(true)
        XCTAssertEqual(f.center.prompts, 1)
        XCTAssertTrue(f.coordinator.policy.enabled)
        XCTAssertTrue(f.coordinator.policy.categories.isEmpty)
    }
    func testDeniedOptInNeverRepeatedlyPrompts() async throws {
        let f = try notificationFixture(self, enabled: false)
        f.center.status = .notDetermined
        f.center.promptStatus = .denied
        await f.coordinator.setEnabled(true)
        await f.coordinator.setEnabled(true)
        XCTAssertEqual(f.center.prompts, 1)
        XCTAssertEqual(f.coordinator.failure, .authorizationDenied)
    }
    func testUserDisabledDoesNotVerifyOrSchedule() async throws {
        let f = try notificationFixture(self, enabled: false)
        await f.coordinator.observe(f.candidate())
        XCTAssertEqual(f.authority.verified, 0)
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testCategoryDisabledDoesNotSchedule() async throws {
        let f = try notificationFixture(self)
        f.coordinator.setCategory(.verifiedDownload, enabled: false)
        await f.coordinator.observe(f.candidate())
        XCTAssertEqual(f.coordinator.failure, .categoryDisabled)
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testCategoryPreferencesAreIndependent() throws {
        let f = try notificationFixture(self, enabled: false)
        f.coordinator.setCategory(.verifiedDownload, enabled: true)
        XCTAssertEqual(f.coordinator.policy.categories, [.verifiedDownload])
        XCTAssertFalse(f.coordinator.policy.enabled)
    }
    func testGrantedSchedulesGenericPayload() async throws {
        let f = try notificationFixture(self)
        await f.coordinator.observe(f.candidate())
        XCTAssertEqual(f.center.payloads.count, 1)
        XCTAssertEqual(f.center.payloads.first?.title, "Synveil download ready")
        let rows = try await f.ledger.events()
        XCTAssertEqual(rows.first?.state, .scheduled)
    }
    func testDeniedCannotSchedule() async throws { try await unavailable(.denied) }
    func testProvisionalCannotSchedule() async throws { try await unavailable(.provisional) }
    func testEphemeralCannotSchedule() async throws { try await unavailable(.ephemeral) }
    func testUnavailableCannotSchedule() async throws { try await unavailable(.unavailable) }
    func testUndeterminedCompletionNeverPrompts() async throws {
        try await unavailable(.notDetermined)
    }
    private func unavailable(_ status: NotificationAuthorization) async throws {
        let f = try notificationFixture(self)
        f.center.status = status
        await f.coordinator.observe(f.candidate())
        XCTAssertTrue(f.center.payloads.isEmpty)
        XCTAssertEqual(f.center.prompts, 0)
    }
    func testSystemPermissionChangedExternallyIsRechecked() async throws {
        let f = try notificationFixture(self)
        await f.coordinator.observe(f.candidate())
        f.center.status = .denied
        await f.coordinator.observe(f.candidate())
        XCTAssertEqual(f.center.payloads.count, 1)
        XCTAssertEqual(f.coordinator.authorization, .denied)
    }
    func testAuthorizationFailureDoesNotEnableDelivery() async throws {
        let f = try notificationFixture(self, enabled: false)
        f.center.status = .notDetermined
        f.center.authorizationFailure = true
        await f.coordinator.setEnabled(true)
        XCTAssertFalse(f.coordinator.policy.enabled)
    }
    func testDuplicateObservationsScheduleOnce() async throws {
        let f = try notificationFixture(self)
        let candidate = f.candidate()
        await f.coordinator.observe(candidate)
        await f.coordinator.observe(candidate)
        XCTAssertEqual(f.center.payloads.count, 1)
        let rows = try await f.ledger.events()
        XCTAssertEqual(rows.count, 1)
    }
    func testConcurrentObservationsScheduleOnce() async throws {
        let f = try notificationFixture(self)
        let candidate = f.candidate()
        await withTaskGroup(of: Void.self) { group in
            for _ in 0..<20 { group.addTask { await f.coordinator.observe(candidate) } }
        }
        await f.coordinator.reconcile()
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testRelaunchUnclaimedEligibilitySchedulesOnce() async throws {
        let f = try notificationFixture(self)
        _ = try await f.ledger.insert(f.event())
        let reopened = try NotificationDeliveryLedger(url: f.url)
        let coordinator = NotificationCoordinator(
            scheduler: f.center, policy: f.policy, ledger: reopened, authority: f.authority,
            navigation: f.navigation, revision: { f.clock.revision }, clock: { f.clock.now })
        await coordinator.reconcile()
        await coordinator.reconcile()
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testCrashAfterClaimBeforeSystemCallNeverRetries() async throws {
        let f = try notificationFixture(self)
        let event = try await f.ledger.insert(f.event())
        _ = try await f.ledger.claim(event.id, now: f.clock.now)
        await f.coordinator.reconcile()
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testCrashAfterSystemCallBeforeReceiptNeverRetries() async throws {
        let f = try notificationFixture(self)
        let event = try await f.ledger.insert(f.event())
        _ = try await f.ledger.claim(event.id, now: f.clock.now)
        try await f.center.schedule(
            LocalNotificationPayload(handle: event.id, category: .verifiedDownload))
        f.center.pending = []
        await f.coordinator.reconcile()
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testIOSRemovalNeverCreatesReplacement() async throws {
        let f = try notificationFixture(self)
        let candidate = f.candidate()
        await f.coordinator.observe(candidate)
        f.center.pending = []
        await f.coordinator.observe(candidate)
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testSchedulingErrorNeverRetriesOrChangesAuthority() async throws {
        let f = try notificationFixture(self)
        f.center.scheduleFailure = true
        await f.coordinator.observe(f.candidate())
        await f.coordinator.reconcile()
        XCTAssertEqual(f.center.payloads.count, 1)
        XCTAssertTrue(f.authority.valid)
        XCTAssertEqual(f.coordinator.failure, .schedulingError)
    }
    func testPendingSystemCapacityFailsClosed() async throws {
        let f = try notificationFixture(self)
        f.center.pending = (0..<16).map { NotificationLimits.prefix + String($0) }
        await f.coordinator.observe(f.candidate())
        XCTAssertEqual(f.coordinator.failure, .capacity)
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testOtherApplicationsRequestsDoNotCountAgainstCapacity() async throws {
        let f = try notificationFixture(self)
        f.center.pending = (0..<100).map { "other." + String($0) }
        await f.coordinator.observe(f.candidate())
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testBackgroundSyncIsAggregatedWithinQuietPeriod() async throws {
        let f = try notificationFixture(self)
        await f.coordinator.observe(f.candidate(.confirmedInbound))
        await f.coordinator.observe(f.candidate(.confirmedInbound))
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testNewBackgroundProgressAfterQuietPeriodMayNotify() async throws {
        let f = try notificationFixture(self)
        await f.coordinator.observe(f.candidate(.confirmedInbound))
        f.clock.now = f.clock.now.addingTimeInterval(NotificationLimits.syncQuietPeriod)
        await f.coordinator.observe(f.candidate(.confirmedInbound))
        XCTAssertEqual(f.center.payloads.count, 2)
    }
    func testSameUnresolvedRecoveryIsSuppressed() async throws {
        let f = try notificationFixture(self)
        let candidate = f.candidate(.uncertainUpload)
        await f.coordinator.observe(candidate)
        await f.coordinator.observe(candidate)
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testRecurringConditionAtSameDurablePositionRemainsSuppressed() async throws {
        let f = try notificationFixture(self)
        let candidate = f.candidate(.uncertainUpload)
        await f.coordinator.observe(candidate)
        f.authority.valid = false
        await f.coordinator.observe(candidate)
        f.authority.valid = true
        await f.coordinator.observe(candidate)
        XCTAssertEqual(f.center.payloads.count, 1)
    }
    func testNewRecoveryWithDurablyAdvancedPositionMayNotify() async throws {
        let f = try notificationFixture(self)
        let id = UUID()
        await f.coordinator.observe(
            f.candidate(.uncertainUpload, operation: id, proof: String(repeating: "b", count: 64)))
        await f.coordinator.observe(
            f.candidate(.uncertainUpload, operation: id, proof: String(repeating: "c", count: 64)))
        XCTAssertEqual(f.center.payloads.count, 2)
    }
    func testInvalidMetadataFailsClosed() async throws {
        let f = try notificationFixture(self)
        let candidate = NotificationCandidate(
            category: .verifiedDownload, evidence: .committedUpload,
            operation: "secret", library: "private", scopeFingerprint: f.authority.scope,
            proofFingerprint: "bad")
        await f.coordinator.observe(candidate)
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testMissingDurableEvidenceCannotSchedule() async throws {
        let f = try notificationFixture(self)
        f.authority.valid = false
        await f.coordinator.observe(f.candidate())
        XCTAssertEqual(f.coordinator.failure, .durableEventUnavailable)
    }
    func testLifecycleChangeDuringVerificationFencesScheduling() async throws {
        let f = try notificationFixture(self)
        f.authority.onVerify = { f.clock.revision += 1 }
        await f.coordinator.observe(f.candidate())
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testLogoutDuringSystemSchedulingRemovesLateRequest() async throws {
        let f = try notificationFixture(self)
        f.center.onSchedule = {
            f.coordinator.invalidateSession()
            f.clock.revision += 1
        }
        await f.coordinator.observe(f.candidate())
        XCTAssertTrue(f.center.pending.isEmpty)
        XCTAssertFalse(f.coordinator.policy.enabled)
    }
    func testLogoutWithPreferenceWriteFailureStillFencesDelivery() async throws {
        let f = try notificationFixture(self)
        f.policy.fails = true
        f.coordinator.invalidateSession()
        await f.coordinator.observe(f.candidate())
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testCrossAccountScopeCannotSchedule() async throws {
        let f = try notificationFixture(self)
        let candidate = f.candidate()
        f.authority.scope = String(repeating: "b", count: 64)
        await f.coordinator.observe(candidate)
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testForegroundPolicyDoesNotClaimUserSawOutcome() throws {
        let f = try notificationFixture(self)
        for _ in 0..<1000 { f.coordinator.foregroundObserved() }
        XCTAssertEqual(f.coordinator.foregroundObservations, NotificationLimits.maximumRecords)
        XCTAssertTrue(f.center.payloads.isEmpty)
    }
    func testPayloadPrivacyForEveryCategory() throws {
        for category in LocalNotificationCategory.allCases {
            let payload = LocalNotificationPayload(handle: UUID(), category: category)
            XCTAssertEqual(payload.userInfo.count, 1)
            XCTAssertEqual(
                NotificationLimits.handle(
                    identifier: payload.identifier, userInfo: payload.userInfo), payload.handle)
            for secret in [
                "private.txt", "https://", "svd1_", "sha256:", "/Caches/", "v1.signed", "Account",
                "Library",
            ] {
                XCTAssertFalse(
                    (payload.title + payload.body + payload.identifier
                        + payload.userInfo.values.joined()).contains(secret))
            }
        }
    }
    func testAuthenticationAndIntegrityFailuresRemainDistinctFromLedgerFailure() {
        XCTAssertEqual(
            NotificationFailure.classify(LibraryFailure.deviceRevoked), .sessionInvalidated)
        XCTAssertEqual(
            NotificationFailure.classify(LibraryFailure.originMismatch), .accountMismatch)
        XCTAssertEqual(
            NotificationFailure.classify(DownloadFailure.sha256Mismatch), .durableEventUnavailable)
        XCTAssertEqual(NotificationFailure.classify(MutationQueueFailure.diskFull), .storageFailure)
    }
}

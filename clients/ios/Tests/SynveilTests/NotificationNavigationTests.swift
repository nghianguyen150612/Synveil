import Foundation
import XCTest

@testable import Synveil

@MainActor
final class NotificationNavigationTests: XCTestCase {
    private func scheduled(_ f: NotificationFixture) async throws -> NotificationEvent {
        await f.coordinator.observe(f.candidate())
        let events = try await f.ledger.events()
        return try XCTUnwrap(events.first)
    }
    func testCurrentAccountResolvesAndConsumesIntent() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 1)
        XCTAssertEqual(f.navigation.selection, event.id)
        let events = try await f.ledger.events()
        XCTAssertEqual(events.first?.state, .handled)
    }
    func testTapBeforeStartupWaits() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        await f.consume(ready: false)
        XCTAssertNil(f.navigation.destination)
        await f.consume()
        XCTAssertEqual(f.navigation.selection, event.id)
    }
    func testPendingSessionVerificationWaitsWithoutReadingResource() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        await f.consume(authenticated: false, awaiting: true)
        XCTAssertEqual(f.authority.resolveCount, 0)
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 1)
    }
    func testLoggedOutTapCannotNavigate() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        await f.consume(authenticated: false)
        await f.consume()
        XCTAssertNil(f.navigation.destination)
        XCTAssertEqual(f.authority.resolveCount, 0)
    }
    func testDifferentAccountAfterRelaunchCannotResolve() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.authority.scope = String(repeating: "b", count: 64)
        f.navigation.receive(event.id)
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 0)
        XCTAssertEqual(f.navigation.failure, .accountMismatch)
    }
    func testRevokedCredentialCannotResolve() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.authority.revoked = true
        f.navigation.receive(event.id)
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 0)
        XCTAssertEqual(f.navigation.failure, .sessionInvalidated)
    }
    func testRemovedResourceDisplaysSafeUnavailableState() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.authority.valid = false
        f.navigation.receive(event.id)
        await f.consume()
        guard case .unavailable = f.navigation.destination else {
            return XCTFail("Expected safe unavailable destination")
        }
    }
    func testExpiredRoutingRecordIsRejected() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.clock.now = event.expiresAt
        f.navigation.receive(event.id)
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 0)
    }
    func testExpiredPendingIntentIsDiscarded() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        f.clock.now = f.clock.now.addingTimeInterval(NotificationLimits.intentLifetime)
        await f.consume()
        XCTAssertNil(f.navigation.destination)
        XCTAssertEqual(f.navigation.failure, .invalidIntent)
    }
    func testTamperedUserInfoCannotProduceHandle() {
        let id = UUID().uuidString.lowercased()
        XCTAssertNil(
            NotificationLimits.handle(
                identifier: NotificationLimits.prefix + id,
                userInfo: ["event": id, "node": "private"]))
        XCTAssertNil(NotificationLimits.handle(identifier: "other." + id, userInfo: ["event": id]))
        XCTAssertNil(
            NotificationLimits.handle(
                identifier: NotificationLimits.prefix + id, userInfo: ["event": "../../private"]))
        XCTAssertNil(
            NotificationLimits.handle(
                identifier: NotificationLimits.prefix + id,
                userInfo: ["event": UUID().uuidString.lowercased()]))
    }
    func testUnknownHandleCannotResolve() async throws {
        let f = try notificationFixture(self)
        f.navigation.receive(UUID())
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 0)
    }
    func testRepeatedTapConsumesOnce() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        f.navigation.receive(event.id)
        await f.consume()
        f.navigation.clearDestination()
        f.navigation.receive(event.id)
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 1)
        XCTAssertNil(f.navigation.destination)
        XCTAssertEqual(f.navigation.failure, .alreadyHandled)
    }
    func testRepeatedTapAfterRelaunchDoesNotNavigateAgain() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        await f.consume()
        let navigation = NotificationNavigationCoordinator(clock: { f.clock.now })
        navigation.receive(event.id)
        await navigation.consume(
            ready: true, authenticated: true, mayAwaitAuthentication: false,
            policy: f.coordinator.policy, ledger: f.ledger, authority: f.authority)
        XCTAssertNil(navigation.destination)
        XCTAssertEqual(f.authority.resolveCount, 1)
    }
    func testLogoutInvalidatesPendingIntentImmediately() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.navigation.receive(event.id)
        f.coordinator.invalidateSession()
        await f.consume()
        XCTAssertNil(f.navigation.destination)
        XCTAssertEqual(f.authority.resolveCount, 0)
    }
    func testLogoutDuringResourceResolutionCannotPublishNavigation() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.authority.onResolve = { f.navigation.invalidate() }
        f.navigation.receive(event.id)
        await f.consume()
        XCTAssertNil(f.navigation.destination)
    }
    func testNewSessionGenerationRejectsOldRoute() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        f.policy.value.generation = UUID()
        f.navigation.receive(event.id)
        await f.navigation.consume(
            ready: true, authenticated: true, mayAwaitAuthentication: false,
            policy: f.policy.value, ledger: f.ledger, authority: f.authority)
        XCTAssertEqual(f.authority.resolveCount, 0)
    }
    func testUnclaimedEligibilityIsNotATappableCapability() async throws {
        let f = try notificationFixture(self)
        let event = try await f.ledger.insert(f.event())
        f.navigation.receive(event.id)
        await f.consume()
        XCTAssertEqual(f.authority.resolveCount, 0)
    }
    func testDisabledPolicyRejectsRouting() async throws {
        let f = try notificationFixture(self)
        let event = try await scheduled(f)
        var policy = f.coordinator.policy
        policy.enabled = false
        f.navigation.receive(event.id)
        await f.navigation.consume(
            ready: true, authenticated: true, mayAwaitAuthentication: false,
            policy: policy, ledger: f.ledger, authority: f.authority)
        XCTAssertEqual(f.authority.resolveCount, 0)
    }
    func testOnlyLatestPendingIntentIsRetained() async throws {
        let f = try notificationFixture(self)
        await f.coordinator.observe(f.candidate())
        await f.coordinator.observe(f.candidate())
        let events = try await f.ledger.events()
        f.navigation.receive(events[0].id)
        f.navigation.receive(events[1].id)
        await f.consume()
        XCTAssertEqual(f.navigation.selection, events[1].id)
        XCTAssertEqual(f.authority.resolveCount, 1)
    }
    func testResponseDuringActiveNavigationReplacesDestinationSafely() async throws {
        let f = try notificationFixture(self)
        await f.coordinator.observe(f.candidate())
        await f.coordinator.observe(f.candidate())
        let events = try await f.ledger.events()
        f.navigation.receive(events[0].id)
        await f.consume()
        f.navigation.receive(events[1].id)
        await f.consume()
        XCTAssertEqual(f.navigation.selection, events[1].id)
        XCTAssertEqual(f.authority.resolveCount, 2)
    }
    func testNewResponseDuringResolutionDoesNotPublishSupersededRoute() async throws {
        let f = try notificationFixture(self)
        await f.coordinator.observe(f.candidate())
        await f.coordinator.observe(f.candidate())
        let events = try await f.ledger.events()
        f.authority.onResolve = { f.navigation.receive(events[1].id) }
        f.navigation.receive(events[0].id)
        await f.consume()
        XCTAssertNil(f.navigation.destination)
        f.authority.onResolve = nil
        await f.consume()
        XCTAssertEqual(f.navigation.selection, events[1].id)
    }
}

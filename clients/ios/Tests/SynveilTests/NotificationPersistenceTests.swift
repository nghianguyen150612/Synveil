import Foundation
import XCTest

@testable import Synveil

final class NotificationLedgerFault: @unchecked Sendable {
    private let lock = NSLock()
    private var point: NotificationDeliveryLedger.Fault?
    func arm(_ point: NotificationDeliveryLedger.Fault) {
        lock.lock()
        self.point = point
        lock.unlock()
    }
    func hit(_ point: NotificationDeliveryLedger.Fault) throws {
        lock.lock()
        defer { lock.unlock() }
        if self.point == point {
            self.point = nil
            throw NotificationFailure.storageFailure
        }
    }
}

@MainActor
final class NotificationPersistenceTests: XCTestCase {
    func testReopenPreservesDeduplicationAndClaims() async throws {
        let f = try notificationFixture(self)
        let event = try await f.ledger.insert(f.event())
        _ = try await f.ledger.claim(event.id, now: f.clock.now)
        let reopened = try NotificationDeliveryLedger(url: f.url)
        let events = try await reopened.events()
        XCTAssertEqual(events.first?.state, .claimed)
        let duplicate = try await reopened.insert(f.event(event.candidate))
        XCTAssertEqual(duplicate.id, event.id)
    }
    func testInterruptedWriteBeforeWritePreservesSnapshot() async throws {
        try await fault(.beforeWrite, committed: false)
    }
    func testInterruptedWriteBeforeRenamePreservesSnapshot() async throws {
        try await fault(.beforeRename, committed: false)
    }
    func testLostReceiptAfterRenameRetainsClaim() async throws {
        try await fault(.afterRename, committed: true)
    }
    private func fault(_ point: NotificationDeliveryLedger.Fault, committed: Bool) async throws {
        let f = try notificationFixture(self)
        let faults = NotificationLedgerFault()
        let ledger = try NotificationDeliveryLedger(url: f.url, fault: { try faults.hit($0) })
        let event = try await ledger.insert(f.event())
        faults.arm(point)
        do {
            _ = try await ledger.claim(event.id, now: f.clock.now)
            XCTFail("Expected injected write failure")
        } catch {}
        let reopened = try NotificationDeliveryLedger(url: f.url)
        let events = try await reopened.events()
        XCTAssertEqual(events.first?.state, committed ? .claimed : .eligible)
    }
    func testStorageFullEquivalentFailsClosedBeforePublication() async throws {
        let f = try notificationFixture(self)
        let ledger = try NotificationDeliveryLedger(
            url: f.url, fault: { _ in throw MutationQueueFailure.diskFull })
        let coordinator = NotificationCoordinator(
            scheduler: f.center, policy: f.policy, ledger: ledger,
            authority: f.authority, navigation: f.navigation, revision: { f.clock.revision },
            clock: { f.clock.now })
        await coordinator.observe(f.candidate())
        XCTAssertTrue(f.center.payloads.isEmpty)
        XCTAssertEqual(coordinator.failure, .storageFailure)
    }
    func testCorruptMetadataFailsClosed() async throws { try await corrupt(Data("not-json".utf8)) }
    func testFutureFormatFailsClosed() async throws {
        try await corrupt(Data("{\"version\":2,\"events\":[]}".utf8))
    }
    func testTruncatedMetadataCannotResetDeduplication() async throws { try await corrupt(Data()) }
    func testOversizedMetadataFailsClosed() async throws {
        try await corrupt(Data(repeating: 0, count: NotificationLimits.maximumBytes + 1))
    }
    private func corrupt(_ bytes: Data) async throws {
        let f = try notificationFixture(self)
        try bytes.write(to: f.url)
        await f.coordinator.observe(f.candidate())
        XCTAssertTrue(f.center.payloads.isEmpty)
        XCTAssertEqual(f.coordinator.failure, .storageFailure)
    }
    func testSymbolicLinkIsRejected() async throws {
        let f = try notificationFixture(self)
        let target = f.url.appendingPathExtension("target")
        try Data("private".utf8).write(to: target)
        try FileManager.default.removeItem(at: f.url)
        try FileManager.default.createSymbolicLink(at: f.url, withDestinationURL: target)
        do {
            _ = try await f.ledger.events()
            XCTFail("Expected symbolic link rejection")
        } catch {}
    }
    func testRecordAndDedupKeyCapacityIsBounded() async throws {
        let f = try notificationFixture(self)
        for _ in 0..<NotificationLimits.maximumRecords { _ = try await f.ledger.insert(f.event()) }
        do {
            _ = try await f.ledger.insert(f.event())
            XCTFail("Expected bounded ledger")
        } catch {
            XCTAssertEqual(error as? NotificationFailure, .capacity)
        }
        let events = try await f.ledger.events()
        XCTAssertEqual(events.count, NotificationLimits.maximumRecords)
    }
    func testClaimCapacityIncludesUncertainSystemReceipts() async throws {
        let f = try notificationFixture(self)
        for _ in 0..<NotificationLimits.maximumPending {
            let event = try await f.ledger.insert(f.event())
            _ = try await f.ledger.claim(event.id, now: f.clock.now)
        }
        let event = try await f.ledger.insert(f.event())
        do {
            _ = try await f.ledger.claim(event.id, now: f.clock.now)
            XCTFail("Expected uncertain claims to count")
        } catch {
            XCTAssertEqual(error as? NotificationFailure, .capacity)
        }
    }
    func testExpiredClaimReleasesConservativeCapacity() async throws {
        let f = try notificationFixture(self)
        for _ in 0..<NotificationLimits.maximumPending {
            let event = try await f.ledger.insert(f.event())
            _ = try await f.ledger.claim(event.id, now: f.clock.now)
        }
        f.clock.now = f.clock.now.addingTimeInterval(NotificationLimits.routingLifetime)
        let event = try await f.ledger.insert(f.event())
        let claimed = try await f.ledger.claim(event.id, now: f.clock.now)
        XCTAssertTrue(claimed)
    }
    func testInvalidationOnlyRetiresSpecifiedGeneration() async throws {
        let f = try notificationFixture(self)
        let original = try await f.ledger.insert(f.event())
        f.policy.value.generation = UUID()
        let later = try await f.ledger.insert(f.event())
        let removed = try await f.ledger.invalidate(generation: original.generation)
        XCTAssertEqual(removed, [original.requestIdentifier])
        let events = try await f.ledger.events()
        XCTAssertEqual(events.first(where: { $0.id == later.id })?.state, .eligible)
    }
    func testCleanupCannotDeleteMutationOrTransferEvidence() async throws {
        let queue = try await queueFixture(self)
        let mutation = try await queueEnqueued(queue)
        let f = try notificationFixture(self)
        let original = try await f.ledger.insert(f.event())
        _ = try await f.ledger.invalidate(generation: original.generation)
        let stored = try await queue.database.record(
            scope: queue.scope, id: mutation.mutation.id.rawValue)
        XCTAssertEqual(stored?.request, mutation.mutation.requestBody)
        XCTAssertEqual(try queueRawScalar(queue.url, "PRAGMA user_version"), "9")
        XCTAssertEqual(
            try queueRawScalar(
                queue.url,
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
            ), "23")
    }
    func testInvalidEventEncodingRejected() async throws {
        let f = try notificationFixture(self)
        var event = f.event()
        event.state = .scheduled
        do {
            _ = try await f.ledger.insert(event)
            XCTFail("Expected invalid eligibility")
        } catch {
            XCTAssertEqual(error as? NotificationFailure, .payloadRejected)
        }
    }
    func testConcurrentIndependentLedgerClaimsCannotDuplicate() async throws {
        let f = try notificationFixture(self)
        let event = try await f.ledger.insert(f.event())
        let second = try NotificationDeliveryLedger(url: f.url)
        async let first = try? f.ledger.claim(event.id, now: f.clock.now)
        async let other = try? second.claim(event.id, now: f.clock.now)
        let results = await [first, other]
        XCTAssertEqual(results.filter { $0 == true }.count, 1)
    }
}

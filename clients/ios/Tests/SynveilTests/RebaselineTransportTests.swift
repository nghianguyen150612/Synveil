import Foundation
import XCTest

@testable import Synveil

@MainActor
final class RebaselineTransportTests: XCTestCase {
    func testExactStartRouteBodyAuthenticationAndPrivateHeaders() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        try await snapshotStart(f)
        let observed24 = await f.transport.requests().last
        let request = try XCTUnwrap(observed24)
        XCTAssertEqual(
            request.url.path,
            "/synveil/api/v1/devices/\(f.scope.deviceId.rawValue)/libraries/\(f.scope.libraryId.rawValue)/rebaseline"
        )
        XCTAssertEqual(request.method, .post)
        XCTAssertEqual(request.body, Data("{}".utf8))
        XCTAssertEqual(request.headers["Authorization"], "Bearer \(queueBearer)")
        XCTAssertEqual(request.headers["Cache-Control"], "no-store")
        XCTAssertNil(request.headers["Cookie"])
        XCTAssertNil(request.headers["X-CSRF-Token"])
        let observed25 = try await f.database.syncBase(scope: f.scope)
        XCTAssertNil(observed25)
    }
    func testStartRequiresExplicitConfirmation() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        await snapshotAssertFailure(.confirmationRequired) {
            try await snapshotCoordinator(f).start(
                scope: f.scope, library: snapshotLibrary(f.scope), confirmedByUser: false)
        }
        let observed26 = await f.transport.requests().isEmpty
        XCTAssertTrue(observed26)
    }
    func testStrictBootstrapFieldMutations() async throws {
        let scope = try await queueScope()
        let decoder = RebaselineResponseDecoder(bridge: QueueValidator())
        let good = snapshotBootstrapObject(scope)
        for key in good.keys {
            var missing = good
            missing.removeValue(forKey: key)
            await snapshotAssertFailure {
                try await decoder.start(queueHTTP(snapshotEnvelope(missing)), scope: scope)
            }
            var null = good
            null[key] = NSNull()
            await snapshotAssertFailure {
                try await decoder.start(queueHTTP(snapshotEnvelope(null)), scope: scope)
            }
        }
        for (key, value) in [
            "bootstrap_id": "bad", "device_id": queueUUID(12), "library_id": queueUUID(13),
            "state": "PURGING", "generation": "0", "snapshot_epoch": "0",
            "snapshot_resume_sequence": "-1", "manifest_item_count": "18446744073709551615",
            "created_at": "2026-02-30T00:00:00Z", "expires_at": "2026-10-01T00:00:00Z",
        ] {
            var bad = good
            bad[key] = value
            await snapshotAssertFailure {
                try await decoder.start(queueHTTP(snapshotEnvelope(bad)), scope: scope)
            }
        }
    }
    func testBootstrapRejectsUnknownKeysNullOptionalsAndWrongTypes() async throws {
        let scope = try await queueScope()
        let decoder = RebaselineResponseDecoder(bridge: QueueValidator())
        for (key, value) in [
            ("extra", "x" as Any), ("completed_at", NSNull()), ("generation", 1),
            ("snapshot_resume_sequence", true),
        ] {
            var bad = snapshotBootstrapObject(scope)
            bad[key] = value
            await snapshotAssertFailure {
                try await decoder.start(queueHTTP(snapshotEnvelope(bad)), scope: scope)
            }
        }
    }
    func testRequestIdHeaderAndEnvelopeValidation() async throws {
        let scope = try await queueScope()
        let decoder = RebaselineResponseDecoder(bridge: QueueValidator())
        for meta: [String: Any] in [
            ["request_id": "x"], ["request_id": NSNull()],
            ["request_id": "snapshot-request-01", "extra": "x"],
        ] {
            var object = snapshotEnvelope(snapshotBootstrapObject(scope))
            object["meta"] = meta
            await snapshotAssertFailure { try await decoder.start(queueHTTP(object), scope: scope) }
        }
        await snapshotAssertFailure {
            try await decoder.start(
                queueHTTP(
                    snapshotEnvelope(snapshotBootstrapObject(scope)),
                    headers: [
                        "Content-Type": "application/json", "X-Request-Id": "different-request",
                    ]), scope: scope)
        }
    }
    func testSupportedServerStatesAndCompletedTimestamp() async throws {
        let scope = try await queueScope()
        for state in ["OPEN", "ABORTED", "EXPIRED", "COMPLETED"] {
            var object = snapshotBootstrapObject(scope)
            object["state"] = state
            if state == "COMPLETED" { object["completed_at"] = "2026-10-11T12:30:00Z" }
            let value = try await RebaselineResponseDecoder(bridge: QueueValidator()).start(
                queueHTTP(snapshotEnvelope(object)), scope: scope)
            XCTAssertEqual(value.state.rawValue, state)
        }
    }
    func testStartResponseLossIsDurableAndExplicitStartReusesScope() async throws {
        let f = try await queueFixture(self, initializeBase: false)
        let coordinator = snapshotCoordinator(f)
        await f.transport.fail(.timeout)
        await snapshotAssertFailure {
            try await coordinator.start(
                scope: f.scope, library: snapshotLibrary(f.scope), confirmedByUser: true)
        }
        let observed27 = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(observed27, .startUnknown)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let observed28 = try await snapshotCoordinator(f, database: reopened).status(
            scope: f.scope)?.state
        XCTAssertEqual(observed28, .startUnknown)
        try await snapshotStart(f, coordinator: coordinator)
        let observed29 = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(observed29, .bootstrapOpen)
        let requests = await f.transport.requests()
        XCTAssertEqual(requests.count, 2)
        XCTAssertEqual(requests[0].url, requests[1].url)
        XCTAssertEqual(requests[0].body, requests[1].body)
    }
    func testCompleteUsesExactOriginalTokenAndRoute() async throws {
        let f = try await queueFixture(self)
        let coordinator = snapshotCoordinator(f)
        try await snapshotPrepared(f, coordinator: coordinator)
        await f.transport.set(try queueHTTP(snapshotCompletionObject(f.scope)))
        try await coordinator.complete(scope: f.scope, recovering: false, confirmedByUser: true)
        let observed30 = await f.transport.requests().last
        let request = try XCTUnwrap(observed30)
        XCTAssertTrue(request.url.path.hasSuffix("/\(queueUUID(700))/complete"))
        XCTAssertEqual(
            request.body, try RebaselinePolicy.completionBody("opaque.original.terminal-evidence_"))
    }
    func testCompletionBoundsAndRedaction() throws {
        XCTAssertLessThanOrEqual(
            try RebaselinePolicy.completionBody(String(repeating: "a", count: 336)).count, 2048)
        XCTAssertThrowsError(
            try RebaselinePolicy.completionBody(String(repeating: "a", count: 337)))
        XCTAssertThrowsError(try RebaselinePolicy.completionBody(""))
        XCTAssertThrowsError(try RebaselinePolicy.completionBody("a\nb"))
    }
    func testStrictCompletionRejectsWrongCheckpointAndReplayTypes() async throws {
        let scope = try await queueScope()
        let decoder = RebaselineResponseDecoder(bridge: QueueValidator())
        let bootstrap = try await decoder.start(
            queueHTTP(snapshotEnvelope(snapshotBootstrapObject(scope))), scope: scope)
        for (key, value) in [
            ("journal_epoch", "2"), ("acknowledged_sequence", "26"),
            ("acknowledged_sequence", "24"), ("updated_at", "bad"),
        ] {
            var object = snapshotCompletionObject(scope), data = object["data"] as! [String: Any],
                checkpoint = data["checkpoint"] as! [String: Any]
            checkpoint[key] = value
            data["checkpoint"] = checkpoint
            object["data"] = data
            await snapshotAssertFailure {
                try await decoder.completion(queueHTTP(object), expected: bootstrap)
            }
        }
        for value: Any in ["true", 1, NSNull()] {
            var object = snapshotCompletionObject(scope), data = object["data"] as! [String: Any]
            data["replayed"] = value
            object["data"] = data
            await snapshotAssertFailure {
                try await decoder.completion(queueHTTP(object), expected: bootstrap)
            }
        }
        let replayed = try await decoder.completion(
            queueHTTP(snapshotCompletionObject(scope, replayed: true)), expected: bootstrap)
        XCTAssertTrue(replayed.replayed)
    }
    func testExistingOpenBootstrapCannotRewindConfirmedCheckpoint() async throws {
        let f = try await queueFixture(self), coordinator = snapshotCoordinator(f)
        let checkpoint = try await queueCheckpoint(scope: f.scope, sequence: "30")
        _ = try await f.database.persistCheckpoint(checkpoint)
        await snapshotAssertFailure(.reconciliationRequired) {
            try await snapshotStart(f, coordinator: coordinator)
        }
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.sequence, "30")
        XCTAssertEqual(base?.responseBody, checkpoint.responseBody)
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM rebaseline_sessions"), "0")
        let state = try await coordinator.status(scope: f.scope)?.state
        XCTAssertEqual(state, .startUnknown)
    }

}

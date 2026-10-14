import Foundation
import XCTest

@testable import Synveil

private actor EvidenceAuthorizationValidator: AuthenticatedSessionValidationProtocol {
    let result: AuthenticatedSessionValidationResult
    private(set) var calls = 0
    init(_ result: AuthenticatedSessionValidationResult) { self.result = result }
    func validateAuthorization(for session: DeviceCredentialSession) async
        -> AuthenticatedSessionValidationResult
    {
        calls += 1
        return result
    }
    func count() -> Int { calls }
}

@MainActor
final class SessionAuthorizationEvidenceTests: XCTestCase {
    func testCompleteEmptyAuthorizationCatalogWithdrawsOldLibraryEligibilityAtomically()
        async throws
    {
        let f = try await queueFixture(self)
        let session = try queueSession(f.scope)
        await f.transport.set(
            try queueHTTP([
                "data": [], "page": ["has_more": false],
                "meta": ["request_id": "authorization-check-01"],
            ]))
        let service = SessionRestorationService(
            credentialStore: f.credentials,
            authorizationValidator: AuthenticatedSessionValidationService(transport: f.transport),
            authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: f.database))
        let result = await service.restore(configuredServerEndpoint: session.serverEndpoint)
        XCTAssertEqual(result, .remotelyVerifiedAuthorizedSession(session))
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT quarantined FROM scopes"), "1")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM session_authorization_checks"),
            "AUTHORIZED")
        let base = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(base?.sequence, "0")
    }

    func testRealRejectedVerificationWithLostLocalCommitCannotExposeCacheAfterRelaunch()
        async throws
    {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        await f.transport.set(try inboundError("authentication_failed", status: 401))
        let gate = QueueGate()
        await f.transport.suspendNextRequest(gate)
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: f.scope.serverEndpoint),
            restorationService: SessionRestorationService(
                credentialStore: f.credentials,
                authorizationValidator: AuthenticatedSessionValidationService(
                    transport: f.transport),
                authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: f.database)))
        let starting = Task { await controller.start() }
        await gate.wait()
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM session_authorization_checks"), "CHECKING")
        fault.arm(.beforeCommit, failure: .diskFull)
        await gate.release()
        await starting.value
        XCTAssertEqual(controller.restorationFailure, .localStorageFailure)
        XCTAssertFalse(controller.canInspectSavedMetadata)
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let offline = await restore(f, result: .networkFailure, database: reopened)
        XCTAssertTrue(offline.savedAuthorizationEvidenceUnresolved)
        XCTAssertFalse(offline.canInspectSavedMetadata)
        let absent = try await f.credentials.isActiveCredentialAbsent()
        XCTAssertFalse(absent)
    }

    private func identity(_ f: QueueFixture) throws -> SessionAuthorizationIdentity {
        SessionAuthorizationIdentity(try queueSession(f.scope))
    }
    private func restore(
        _ f: QueueFixture, result: AuthenticatedSessionValidationResult,
        database: MutationQueueSQLiteStore? = nil
    ) async -> SessionController {
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: f.scope.serverEndpoint),
            restorationService: SessionRestorationService(
                credentialStore: f.credentials,
                authorizationValidator: EvidenceAuthorizationValidator(result),
                authorizationEvidence: SQLiteSessionAuthorizationEvidence(
                    database: database ?? f.database)))
        await controller.start()
        return controller
    }

    func testOrdinaryTransientVerificationDurablyPreservesSavedEligibility() async throws {
        let f = try await queueFixture(self)
        let controller = await restore(f, result: .networkFailure)
        XCTAssertTrue(controller.canInspectSavedMetadata)
        let eligible = try await f.database.permitsPendingSavedInspection(identity(f))
        XCTAssertTrue(eligible)
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT state FROM session_authorization_checks"), "TRANSIENT"
        )
        XCTAssertEqual(try queueRawScalar(f.url, "SELECT count(*) FROM scopes"), "1")
    }

    func testInterruptedAuthorizationCheckBlocksLaterOfflineLaunchUntilVerified() async throws {
        let f = try await queueFixture(self)
        _ = try await f.database.beginSessionAuthorizationCheck(identity(f))
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let offline = await restore(f, result: .networkFailure, database: reopened)
        XCTAssertTrue(offline.savedAuthorizationEvidenceUnresolved)
        XCTAssertFalse(offline.canInspectSavedMetadata)
        let allowed = try await reopened.permitsPendingSavedInspection(identity(f))
        XCTAssertFalse(allowed)
        let verified = await restore(f, result: .authorized, database: reopened)
        XCTAssertEqual(verified.state, .authenticated)
        let laterOffline = await restore(f, result: .timeout, database: reopened)
        XCTAssertTrue(laterOffline.canInspectSavedMetadata)
        XCTAssertFalse(laterOffline.savedAuthorizationEvidenceUnresolved)
    }

    func testRejectedResponseWhosePersistenceFailsCannotReenableCacheOnRelaunch() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let lease = try await f.database.beginSessionAuthorizationCheck(identity(f))
        fault.arm(.beforeCommit, failure: .diskFull)
        do {
            try await f.database.finishSessionAuthorizationCheck(lease, outcome: .rejected)
            XCTFail("Expected disk failure")
        } catch { XCTAssertEqual(error as? MutationQueueFailure, .diskFull) }
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let controller = await restore(f, result: .networkFailure, database: reopened)
        XCTAssertTrue(controller.savedAuthorizationEvidenceUnresolved)
        XCTAssertFalse(controller.canInspectSavedMetadata)
        let allowed = try await reopened.permitsPendingSavedInspection(identity(f))
        XCTAssertFalse(allowed)
    }

    func testMarkerPersistenceFailurePreventsAuthorizationRequest() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        fault.arm(.beforeCommit, failure: .diskFull)
        let validator = EvidenceAuthorizationValidator(.authorized)
        let service = SessionRestorationService(
            credentialStore: f.credentials, authorizationValidator: validator,
            authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: f.database))
        let result = await service.restore(configuredServerEndpoint: f.scope.serverEndpoint)
        guard case .locallyValidStoredSession(_, .localStorageFailure) = result else {
            return XCTFail("Storage must stop verification")
        }
        let calls = await validator.count()
        XCTAssertEqual(calls, 0)
        let credential = try await f.credentials.load(
            expectedServerEndpoint: f.scope.serverEndpoint)
        XCTAssertEqual(credential.record.credentialId, queueUUID(92))
    }

    func testUnavailableStoragePreventsVerificationWithoutDeletingCredential() async throws {
        let f = try await queueFixture(self)
        let validator = EvidenceAuthorizationValidator(.authorized)
        let service = SessionRestorationService(
            credentialStore: f.credentials, authorizationValidator: validator,
            authorizationEvidence: SQLiteSessionAuthorizationEvidence(database: nil))
        let result = await service.restore(configuredServerEndpoint: f.scope.serverEndpoint)
        guard case .locallyValidStoredSession(_, .localStorageFailure) = result else {
            return XCTFail("No storage means no durable verification marker")
        }
        let calls = await validator.count()
        XCTAssertEqual(calls, 0)
        let absent = try await f.credentials.isActiveCredentialAbsent()
        XCTAssertFalse(absent)
    }

    func testUnconfirmedTransientResultDoesNotGrantSavedAccess() async throws {
        let fault = QueueFaultInjector()
        let f = try await queueFixture(self, fault: fault)
        let lease = try await f.database.beginSessionAuthorizationCheck(identity(f))
        fault.arm(.beforeCommit)
        do {
            try await f.database.finishSessionAuthorizationCheck(lease, outcome: .transientFailure)
            XCTFail("Expected interrupted terminal evidence")
        } catch {}
        let allowed = try await f.database.permitsPendingSavedInspection(identity(f))
        XCTAssertFalse(allowed)
    }

    func testStaleVerificationLeaseCannotOverwriteNewerCheckAcrossConnections() async throws {
        let f = try await queueFixture(self)
        let first = try await f.database.beginSessionAuthorizationCheck(identity(f))
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let second = try await reopened.beginSessionAuthorizationCheck(identity(f))
        XCTAssertTrue(second.requiresVerification)
        do {
            try await f.database.finishSessionAuthorizationCheck(first, outcome: .authorized)
            XCTFail("Old callback cannot publish a grant")
        } catch { XCTAssertEqual(error as? MutationQueueFailure, .ownershipRequired) }
        try await reopened.finishSessionAuthorizationCheck(second, outcome: .transientFailure)
        let allowed = try await reopened.permitsPendingSavedInspection(identity(f))
        XCTAssertFalse(allowed)
    }

    func testAuthorizationEvidenceIsIsolatedByCredentialAndOwner() async throws {
        let f = try await queueFixture(self)
        let original = try await f.database.beginSessionAuthorizationCheck(identity(f))
        try await f.database.finishSessionAuthorizationCheck(original, outcome: .rejected)
        let scope = try await queueScope(device: 191, owner: 190)
        let other = SessionAuthorizationIdentity(try queueSession(scope, credential: 192))
        let fresh = try await f.database.beginSessionAuthorizationCheck(other)
        XCTAssertFalse(fresh.requiresVerification)
        try await f.database.finishSessionAuthorizationCheck(fresh, outcome: .transientFailure)
        let oldAllowed = try await f.database.permitsPendingSavedInspection(identity(f))
        let newAllowed = try await f.database.permitsPendingSavedInspection(other)
        XCTAssertFalse(oldAllowed)
        XCTAssertTrue(newAllowed)
    }

    func testRawSQLiteCannotFabricateAuthorizationEvidence() async throws {
        let f = try await queueFixture(self)
        let lease = try await f.database.beginSessionAuthorizationCheck(identity(f))
        XCTAssertThrowsError(
            try queueRawSQL(
                f.url,
                "UPDATE session_authorization_checks SET state='TRANSIENT',requires_verification=0")
        )
        XCTAssertThrowsError(try queueRawSQL(f.url, "DELETE FROM session_authorization_checks"))
        try await f.database.finishSessionAuthorizationCheck(lease, outcome: .transientFailure)
        let allowed = try await f.database.permitsPendingSavedInspection(identity(f))
        XCTAssertTrue(allowed)
    }

    func testV8MigrationPreservesMutationAndCheckpointBytes() async throws {
        let f = try await queueFixture(self)
        let original = try await queueEnqueued(f)
        let base = try await f.database.syncBase(scope: f.scope)
        try queueRawSQL(f.url, "DROP TABLE session_authorization_checks")
        try queueRawSQL(f.url, "PRAGMA user_version=8")
        let reopened = try MutationQueueSQLiteStore(url: f.url)
        let stored = try await reopened.records(scope: f.scope, limit: 10)
        XCTAssertEqual(stored.first?.request, original.mutation.requestBody)
        let migrated = try await reopened.syncBase(scope: f.scope)
        XCTAssertEqual(migrated?.responseBody, base?.responseBody)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "9")
        XCTAssertEqual(
            try queueRawScalar(f.url, "SELECT count(*) FROM session_authorization_checks"), "0")
    }

    func testV8MigrationFailureRollsBackDDLAndVersion() async throws {
        let f = try await queueFixture(self)
        try queueRawSQL(f.url, "DROP TABLE session_authorization_checks")
        try queueRawSQL(f.url, "PRAGMA user_version=8")
        let fault = QueueFaultInjector()
        fault.arm(.migration)
        XCTAssertThrowsError(try MutationQueueSQLiteStore(url: f.url, fault: { try fault.hit($0) }))
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "8")
        XCTAssertEqual(
            try queueRawScalar(
                f.url,
                "SELECT count(*) FROM sqlite_master WHERE name='session_authorization_checks'"), "0"
        )
        _ = try MutationQueueSQLiteStore(url: f.url)
        XCTAssertEqual(try queueRawScalar(f.url, "PRAGMA user_version"), "9")
    }

    func testAuthorizationChecksDoNotAdvanceSyncOrRewritePendingMutation() async throws {
        let f = try await queueFixture(self)
        let row = try await queueEnqueued(f)
        let before = try await f.database.syncBase(scope: f.scope)
        let lease = try await f.database.beginSessionAuthorizationCheck(identity(f))
        try await f.database.finishSessionAuthorizationCheck(lease, outcome: .transientFailure)
        let after = try await f.database.syncBase(scope: f.scope)
        XCTAssertEqual(after?.responseBody, before?.responseBody)
        let pending = try await queueRecord(f)
        XCTAssertEqual(pending.mutation, row.mutation)
        XCTAssertEqual(pending.state, .pending)
    }
}

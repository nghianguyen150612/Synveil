import Foundation
import XCTest

@testable import Synveil

@MainActor
final class SessionLogoutTests: XCTestCase {
    private let bearer =
        "svd1_abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"

    func testLocalLogoutDeletesAndVerifiesCredentialWithoutNetworkOrEnrollmentToken() async throws {
        let endpoint = try makeEndpoint()
        let session = try makeSession(endpoint: endpoint)
        let store = MemoryCredentialStore(session: session)
        let service = SessionLogoutService(credentialStore: store)

        let result = await service.logoutLocally()

        XCTAssertEqual(result, .credentialAbsent)
        let deleteCount = await store.deleteCount()
        let absenceCheckCount = await store.absenceCheckCount()
        let isAbsent = try await store.isActiveCredentialAbsent()
        XCTAssertEqual(deleteCount, 1)
        XCTAssertEqual(absenceCheckCount, 2)
        XCTAssertTrue(isAbsent)
        do {
            _ = try await store.load(expectedServerEndpoint: endpoint)
            XCTFail("The deleted session remained accessible through the credential store.")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .itemNotFound)
        }
        XCTAssertFalse(String(describing: result).contains(bearer))
    }

    func testMissingCredentialIsSuccessfulIdempotentCleanup() async throws {
        let store = MemoryCredentialStore()
        let result = await SessionLogoutService(credentialStore: store).logoutLocally()

        XCTAssertEqual(result, .credentialAbsent)
        let deleteCount = await store.deleteCount()
        let isAbsent = try await store.isActiveCredentialAbsent()
        XCTAssertEqual(deleteCount, 1)
        XCTAssertTrue(isAbsent)
    }

    func testDeletionFailureDoesNotReportSuccessOrClaimAbsence() async throws {
        let store = MemoryCredentialStore(
            session: try makeSession(endpoint: makeEndpoint()),
            deleteError: .unavailable
        )

        let result = await SessionLogoutService(credentialStore: store).logoutLocally()

        XCTAssertEqual(result, .failed(.secureStorage(.unavailable)))
        let absenceCheckCount = await store.absenceCheckCount()
        XCTAssertEqual(absenceCheckCount, 0)
        XCTAssertTrue(String(describing: result).contains("unavailable"))
        XCTAssertFalse(String(describing: result).contains(bearer))
    }

    func testSuccessfulDeleteWithRemainingItemFailsVerification() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MemoryCredentialStore(session: session, retainItemAfterDelete: true)

        let result = await SessionLogoutService(credentialStore: store).logoutLocally()

        XCTAssertEqual(result, .failed(.credentialRemains))
        let isAbsent = try await store.isActiveCredentialAbsent()
        let deleteCount = await store.deleteCount()
        XCTAssertFalse(isAbsent)
        XCTAssertEqual(deleteCount, 1)
    }

    func testVerificationReadFailureDoesNotReportSuccess() async throws {
        let store = MemoryCredentialStore(
            session: try makeSession(endpoint: makeEndpoint()),
            absenceError: .unavailable
        )

        let result = await SessionLogoutService(credentialStore: store).logoutLocally()

        XCTAssertEqual(result, .failed(.secureStorage(.unavailable)))
    }

    func testLogoutFromAuthenticatedUIReturnsToServerValidationAndRetainsEndpoint() async throws {
        let endpoint = try makeEndpoint()
        let store = MemoryCredentialStore(session: try makeSession(endpoint: endpoint))
        let controller = makeAuthenticatedController(
            endpoint: endpoint,
            logoutService: SessionLogoutService(credentialStore: store)
        )

        await controller.requestLogout()

        XCTAssertEqual(controller.state, .readyForServerValidation)
        XCTAssertEqual(controller.serverEndpoint, endpoint)
        controller.markAuthenticated(after: makeReceipt(for: endpoint))
        XCTAssertEqual(controller.state, .readyForServerValidation)
    }

    func testServerSetupTransitionCannotMasqueradeAsLogout() async throws {
        let endpoint = try makeEndpoint()
        let store = MemoryCredentialStore(session: try makeSession(endpoint: endpoint))
        let controller = makeAuthenticatedController(
            endpoint: endpoint,
            logoutService: SessionLogoutService(credentialStore: store)
        )

        controller.showServerProfileSetup()

        XCTAssertEqual(controller.state, .authenticated)
        let isAbsent = try await store.isActiveCredentialAbsent()
        XCTAssertFalse(isAbsent)
    }

    func testLogoutWithoutEndpointReturnsToServerSetup() async {
        let store = MemoryCredentialStore()
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: nil),
            restorationService: ImmediateLogoutRestorationService(result: .cancelled),
            logoutService: SessionLogoutService(credentialStore: store)
        )

        await controller.start()
        XCTAssertEqual(controller.state, .restorationVerificationPending)
        await controller.requestLogout()

        XCTAssertEqual(controller.state, .needsServerProfile)
        XCTAssertNil(controller.serverEndpoint)
    }

    func testLogoutFailureRetainsRetryableSecureStorageStateThenSucceedsOnRetry() async throws {
        let endpoint = try makeEndpoint()
        let store = MemoryCredentialStore(
            session: try makeSession(endpoint: endpoint),
            deleteError: .unavailable
        )
        let controller = makeAuthenticatedController(
            endpoint: endpoint,
            logoutService: SessionLogoutService(credentialStore: store)
        )

        await controller.requestLogout()

        XCTAssertEqual(controller.state, .logoutCleanupRequired)
        XCTAssertEqual(controller.logoutFailure, .secureStorage(.unavailable))
        XCTAssertEqual(controller.serverEndpoint, endpoint)
        XCTAssertNotEqual(controller.state, .readyForServerValidation)

        await store.setDeleteError(nil)
        await controller.requestLogout()

        XCTAssertEqual(controller.state, .readyForServerValidation)
        let isAbsent = try await store.isActiveCredentialAbsent()
        XCTAssertTrue(isAbsent)
    }

    func testConcurrentLogoutRequestsShareOneCleanupOperation() async throws {
        let endpoint = try makeEndpoint()
        let service = SuspendedLogoutService()
        let controller = makeAuthenticatedController(endpoint: endpoint, logoutService: service)

        async let first: Void = controller.requestLogout()
        await service.waitForCall()
        async let duplicate: Void = controller.requestLogout()
        await Task.yield()

        XCTAssertEqual(controller.state, .logoutInProgress)
        controller.markAuthenticated(after: makeReceipt(for: endpoint))
        XCTAssertEqual(controller.state, .logoutInProgress)
        let firstCallCount = await service.callCount()
        XCTAssertEqual(firstCallCount, 1)
        await service.complete(with: .credentialAbsent)
        await first
        await duplicate

        XCTAssertEqual(controller.state, .readyForServerValidation)
        XCTAssertFalse(String(describing: controller.state).contains(bearer))
        let finalCallCount = await service.callCount()
        XCTAssertEqual(finalCallCount, 1)
    }

    func testCancellingCallerDoesNotAbandonCleanupOrReportEarlySuccess() async throws {
        let endpoint = try makeEndpoint()
        let service = SuspendedLogoutService()
        let controller = makeAuthenticatedController(endpoint: endpoint, logoutService: service)
        let caller = Task { await controller.requestLogout() }
        await service.waitForCall()

        XCTAssertEqual(controller.state, .logoutInProgress)
        caller.cancel()
        XCTAssertEqual(controller.state, .logoutInProgress)
        await service.complete(with: .credentialAbsent)
        await caller.value

        XCTAssertEqual(controller.state, .readyForServerValidation)
    }

    func testLateAuthorizedRestorationResultCannotUndoLogoutOrRetryAfterward() async throws {
        let endpoint = try makeEndpoint()
        let session = try makeSession(endpoint: endpoint)
        let restoration = ControlledLogoutRestorationService(session: session)
        let store = MemoryCredentialStore(session: session)
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint),
            restorationService: restoration,
            logoutService: SessionLogoutService(credentialStore: store)
        )
        await controller.start()
        XCTAssertEqual(controller.state, .restorationVerificationPending)

        await restoration.suspendNextRetry()
        let retry = Task { await controller.retrySessionRestoration() }
        await restoration.waitForRetry()
        XCTAssertTrue(controller.isRestorationRetryInProgress)

        let logout = Task { await controller.requestLogout() }
        await logout.value
        XCTAssertEqual(controller.state, .readyForServerValidation)
        let isAbsent = try await store.isActiveCredentialAbsent()
        XCTAssertTrue(isAbsent)

        await controller.start()
        await controller.retrySessionRestoration()
        await restoration.completeRetry(
            with: .remotelyVerifiedAuthorizedSession(session)
        )
        await retry.value

        XCTAssertEqual(controller.state, .readyForServerValidation)
        let retryCount = await restoration.retryCount()
        XCTAssertEqual(retryCount, 1)
    }

    func testLateHTTP200AfterLogoutCannotRestoreAuthentication() async throws {
        let endpoint = try makeEndpoint()
        let session = try makeSession(endpoint: endpoint)
        let store = MemoryCredentialStore(session: session)
        let transport = LogoutRaceHTTPTransport()
        let restoration = SessionRestorationService(
            credentialStore: store,
            authorizationValidator: AuthenticatedSessionValidationService(transport: transport)
        )
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint),
            restorationService: restoration,
            logoutService: SessionLogoutService(credentialStore: store)
        )

        await controller.start()
        XCTAssertEqual(controller.state, .restorationVerificationPending)

        let retry = Task { await controller.retrySessionRestoration() }
        await transport.waitForRequests(2)
        await controller.requestLogout()
        XCTAssertEqual(controller.state, .readyForServerValidation)

        await transport.completeSuspendedRequest(with: successfulLibrariesResponse())
        await retry.value

        XCTAssertEqual(controller.state, .readyForServerValidation)
        let isAbsent = try await store.isActiveCredentialAbsent()
        XCTAssertTrue(isAbsent)
        let requestCount = await transport.requestCount()
        XCTAssertEqual(requestCount, 2)
    }

    func testRootRendersLogoutControlProgressAndCleanupRecovery() async throws {
        let endpoint = try makeEndpoint()
        let service = SuspendedLogoutService()
        let controller = makeAuthenticatedController(endpoint: endpoint, logoutService: service)
        let authenticatedShell = AuthenticatedShellPlaceholderView(
            sessionController: controller
        )

        XCTAssertNotNil(authenticatedShell.body)
        XCTAssertEqual(SessionLogoutAccessibility.logoutButton, "synveil.session.logout")
        XCTAssertEqual(
            SessionLogoutAccessibility.logoutConfirmation,
            "synveil.session.logout-confirm"
        )

        let logout = Task { await controller.requestLogout() }
        await service.waitForCall()
        XCTAssertEqual(controller.state, .logoutInProgress)
        XCTAssertNotNil(RootView(sessionController: controller).body)
        XCTAssertEqual(
            SessionLogoutAccessibility.logoutProgress,
            "synveil.logout.progress"
        )

        await service.complete(with: .failed(.serviceUnavailable))
        await logout.value
        XCTAssertEqual(controller.state, .logoutCleanupRequired)
        XCTAssertEqual(controller.logoutFailure, .serviceUnavailable)
        XCTAssertNotNil(RootView(sessionController: controller).body)
        XCTAssertEqual(SessionLogoutAccessibility.logoutRetry, "synveil.logout.retry")
    }

    func testFutureStartupCannotRestoreCredentialDeletedByLogout() async throws {
        let endpoint = try makeEndpoint()
        let session = try makeSession(endpoint: endpoint)
        let store = MemoryCredentialStore(session: session)
        let validator = CountingLogoutAuthorizationValidator()
        let controller = makeAuthenticatedController(
            endpoint: endpoint,
            logoutService: SessionLogoutService(credentialStore: store)
        )

        await controller.requestLogout()
        let nextLaunch = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint),
            restorationService: SessionRestorationService(
                credentialStore: store,
                authorizationValidator: validator
            )
        )
        await nextLaunch.start()

        XCTAssertEqual(nextLaunch.state, .readyForServerValidation)
        XCTAssertNotEqual(nextLaunch.state, .authenticated)
        let validationCount = await validator.callCount()
        XCTAssertEqual(validationCount, 0)
    }

    private func makeEndpoint() throws -> ServerEndpoint {
        try ServerEndpoint(validating: "https://sync.synveil.example:8443")
    }

    private func makeSession(endpoint: ServerEndpoint) throws -> DeviceCredentialSession {
        let record = try DeviceCredentialRecord(
            ownerUserId: "11111111-2222-3333-4444-555555555555",
            deviceId: "66666666-7777-8888-9999-000000000000",
            credentialId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            credential: DeviceCredential(validatedRawValue: bearer),
            createdAt: "2026-10-07T12:00:00Z"
        )
        return DeviceCredentialSession(serverEndpoint: endpoint, record: record)
    }

    private func makeReceipt(for endpoint: ServerEndpoint) -> SecureCredentialPersistenceReceipt {
        SecureCredentialPersistenceReceipt(
            session: try! makeSession(endpoint: endpoint)
        )
    }

    private func makeAuthenticatedController(
        endpoint: ServerEndpoint,
        logoutService: SessionLogoutServiceProtocol
    ) -> SessionController {
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint),
            logoutService: logoutService
        )
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()
        controller.markAuthenticated(after: makeReceipt(for: endpoint))
        return controller
    }

    private func successfulLibrariesResponse() -> HTTPTransportResponse {
        HTTPTransportResponse(
            statusCode: 200,
            headers: ["Content-Type": "application/json; charset=utf-8"],
            body: Data(
                """
                {"data":[],"page":{"has_more":false},"meta":{"request_id":"request-123"}}
                """.utf8
            )
        )
    }
}

private actor MemoryCredentialStore: SecureCredentialSinkProtocol {
    private var session: DeviceCredentialSession?
    private var deleteError: SecureCredentialSinkError?
    private let absenceError: SecureCredentialSinkError?
    private let retainItemAfterDelete: Bool
    private var deletes = 0
    private var absenceChecks = 0

    init(
        session: DeviceCredentialSession? = nil,
        deleteError: SecureCredentialSinkError? = nil,
        absenceError: SecureCredentialSinkError? = nil,
        retainItemAfterDelete: Bool = false
    ) {
        self.session = session
        self.deleteError = deleteError
        self.absenceError = absenceError
        self.retainItemAfterDelete = retainItemAfterDelete
    }

    func preflight() async throws {}

    func store(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt {
        throw SecureCredentialSinkError.writeFailure
    }

    func update(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt {
        throw SecureCredentialSinkError.writeFailure
    }

    func load(expectedServerEndpoint: ServerEndpoint?) async throws -> DeviceCredentialSession {
        guard let session else { throw SecureCredentialSinkError.itemNotFound }
        if let expectedServerEndpoint, expectedServerEndpoint != session.serverEndpoint {
            throw SecureCredentialSinkError.scopeMismatch
        }
        return session
    }

    func delete() async throws {
        deletes += 1
        if let deleteError { throw deleteError }
        if !retainItemAfterDelete { session = nil }
    }

    func isActiveCredentialAbsent() async throws -> Bool {
        absenceChecks += 1
        if let absenceError { throw absenceError }
        return session == nil
    }

    func deleteCount() -> Int { deletes }
    func absenceCheckCount() -> Int { absenceChecks }
    func setDeleteError(_ error: SecureCredentialSinkError?) { deleteError = error }
}

private actor SuspendedLogoutService: SessionLogoutServiceProtocol {
    private var calls = 0
    private var continuation: CheckedContinuation<SessionLogoutResult, Never>?
    private var callWaiters: [CheckedContinuation<Void, Never>] = []

    func logoutLocally() async -> SessionLogoutResult {
        calls += 1
        callWaiters.forEach { $0.resume() }
        callWaiters.removeAll()
        return await withCheckedContinuation { continuation in
            self.continuation = continuation
        }
    }

    func waitForCall() async {
        guard calls == 0 else { return }
        await withCheckedContinuation { callWaiters.append($0) }
    }

    func callCount() -> Int { calls }

    func complete(with result: SessionLogoutResult) {
        continuation?.resume(returning: result)
        continuation = nil
    }
}

private actor ImmediateLogoutRestorationService: SessionRestorationServiceProtocol {
    let result: SessionRestorationResult

    init(result: SessionRestorationResult) {
        self.result = result
    }

    func restore(configuredServerEndpoint: ServerEndpoint?) async -> SessionRestorationResult {
        result
    }

    func retryVerification(
        of session: DeviceCredentialSession,
        expectedServerEndpoint: ServerEndpoint
    ) async -> SessionRestorationResult {
        result
    }
}

private actor ControlledLogoutRestorationService: SessionRestorationServiceProtocol {
    private let session: DeviceCredentialSession
    private var retryCalls = 0
    private var shouldSuspendNextRetry = false
    private var retryContinuation: CheckedContinuation<SessionRestorationResult, Never>?
    private var retryWaiters: [CheckedContinuation<Void, Never>] = []

    init(session: DeviceCredentialSession) {
        self.session = session
    }

    func restore(configuredServerEndpoint: ServerEndpoint?) async -> SessionRestorationResult {
        .locallyValidStoredSession(session, verification: .timeout)
    }

    func retryVerification(
        of session: DeviceCredentialSession,
        expectedServerEndpoint: ServerEndpoint
    ) async -> SessionRestorationResult {
        retryCalls += 1
        retryWaiters.forEach { $0.resume() }
        retryWaiters.removeAll()
        guard shouldSuspendNextRetry else {
            return .remotelyVerifiedAuthorizedSession(session)
        }
        shouldSuspendNextRetry = false
        return await withCheckedContinuation { retryContinuation = $0 }
    }

    func suspendNextRetry() { shouldSuspendNextRetry = true }

    func waitForRetry() async {
        guard retryCalls == 0 else { return }
        await withCheckedContinuation { retryWaiters.append($0) }
    }

    func completeRetry(with result: SessionRestorationResult) {
        retryContinuation?.resume(returning: result)
        retryContinuation = nil
    }

    func retryCount() -> Int { retryCalls }
}

private actor CountingLogoutAuthorizationValidator: AuthenticatedSessionValidationProtocol {
    private var calls = 0

    func validateAuthorization(
        for session: DeviceCredentialSession
    ) async -> AuthenticatedSessionValidationResult {
        calls += 1
        return .authorized
    }

    func callCount() -> Int { calls }
}

private actor LogoutRaceHTTPTransport: HTTPTransportProtocol {
    private var calls = 0
    private var responseContinuation: CheckedContinuation<HTTPTransportResponse, Never>?
    private var requestWaiters: [(Int, CheckedContinuation<Void, Never>)] = []

    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        calls += 1
        let readyWaiters = requestWaiters.filter { $0.0 <= calls }
        requestWaiters.removeAll { $0.0 <= calls }
        readyWaiters.forEach { $0.1.resume() }

        if calls == 1 {
            throw SynveilTransportError.timeout
        }
        return await withCheckedContinuation { responseContinuation = $0 }
    }

    func waitForRequests(_ expectedCount: Int) async {
        guard calls < expectedCount else { return }
        await withCheckedContinuation { requestWaiters.append((expectedCount, $0)) }
    }

    func completeSuspendedRequest(with response: HTTPTransportResponse) {
        responseContinuation?.resume(returning: response)
        responseContinuation = nil
    }

    func requestCount() -> Int { calls }
}

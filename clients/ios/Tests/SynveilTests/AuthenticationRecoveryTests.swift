import SwiftUI
import UIKit
import XCTest
@testable import Synveil

final class AuthenticationRecoveryTests: XCTestCase {
    func testAuthenticationRejectionExplainsBlockedRetainedSessionAndOwnerRecovery() {
        let result = AuthenticationRecoveryPresenter.recovery(.authentication)
        XCTAssertEqual(result.category, .authentication)
        XCTAssertTrue(result.message.contains("saved authorization"))
        XCTAssertTrue(result.sessionProtection.contains("not been deleted"))
        XCTAssertTrue(result.sessionProtection.contains("blocked"))
        XCTAssertTrue(result.nextStep.contains("trusted server owner"))
        XCTAssertNil(result.primaryAction)
    }

    func testRevokedDeviceHasDistinctContentAndNoBearerRetry() {
        let result = AuthenticationRecoveryPresenter.recovery(.deviceRevoked)
        XCTAssertEqual(result.category, .deviceRevoked)
        XCTAssertTrue(result.title.contains("revoked"))
        XCTAssertTrue(result.message.contains("Retrying the same credential will not restore"))
        XCTAssertTrue(result.nextStep.contains("authorized enrollment"))
        XCTAssertNil(result.primaryAction)
        XCTAssertNotEqual(result, AuthenticationRecoveryPresenter.recovery(.authentication))
    }

    func testEmptyAndMalformedTokenHaveDistinctCorrectableLocalFeedback() {
        let empty = AuthenticationRecoveryPresenter.enrollment(.invalidToken("ignored"), inputIsEmpty: true)!
        let malformed = AuthenticationRecoveryPresenter.enrollment(.invalidToken("ignored"), inputIsEmpty: false)!
        XCTAssertEqual(empty.category, .emptyToken)
        XCTAssertEqual(malformed.category, .malformedToken)
        XCTAssertTrue(malformed.nextStep.contains("64 lowercase hexadecimal"))
        XCTAssertTrue(malformed.nextStep.contains("correct the input"))
        XCTAssertTrue(malformed.sessionProtection.contains("No enrollment request"))
    }

    func testServerRejectedGrantUsesGenericOwnerGuidance() {
        let result = AuthenticationRecoveryPresenter.enrollment(.rejected("untrusted typo detail"), inputIsEmpty: false)!
        XCTAssertEqual(result.category, .enrollmentRejected)
        XCTAssertTrue(result.nextStep.contains("new grant"))
        XCTAssertFalse(result.accessibilityDescription.contains("typo"))
        XCTAssertNil(result.primaryAction)
    }

    func testAmbiguousEnrollmentExplicitlyForbidsSameGrantReplay() {
        let result = AuthenticationRecoveryPresenter.recovery(.enrollmentAmbiguous)
        XCTAssertTrue(result.message.contains("may already have been consumed"))
        XCTAssertTrue(result.nextStep.contains("Do not resend"))
        XCTAssertTrue(result.nextStep.contains("revoke affected credentials"))
        XCTAssertNil(result.primaryAction)
    }

    func testOfflineRestorationOffersReadOnlyVerificationAndRetainsCredential() {
        assertTransient(.networkFailure, category: .offline)
    }

    func testTimeoutRestorationOffersReadOnlyVerification() {
        assertTransient(.timeout, category: .timeout)
    }

    func testDNSFailureDoesNotImplyCredentialDeletion() {
        assertTransient(.dnsFailure, category: .dns)
    }

    func testHTTP503PresentationIsTemporaryAndNeverRevoked() {
        assertTransient(.serverUnavailable, category: .serverUnavailable)
        let result = AuthenticationRecoveryPresenter.serverValidation(
            .failed(.httpError(statusCode: 503, code: "device_revoked", requestId: nil)))!
        XCTAssertEqual(result.category, .serverUnavailable)
        XCTAssertFalse(result.message.contains("revoked"))
    }

    func testTLSExplainsTrustedHTTPSAndOffersNoBypass() {
        let result = AuthenticationRecoveryPresenter.recovery(.tls)
        XCTAssertEqual(result.severity, .security)
        XCTAssertTrue(result.nextStep.contains("HTTPS certificate"))
        XCTAssertNil(result.primaryAction)
        for prohibited in ["trust anyway", "disable HTTPS", "insecure HTTP", "ignore certificate"] {
            XCTAssertFalse(result.accessibilityDescription.contains(prohibited))
        }
    }

    func testProtocolFailureCannotPresentAuthorizationSuccess() {
        let result = AuthenticationRecoveryPresenter.restoration(.unexpectedProtocolResponse)
        XCTAssertEqual(result.category, .protocolFailure)
        XCTAssertNil(result.primaryAction)
        XCTAssertTrue(result.sessionProtection.contains("blocked"))
        XCTAssertTrue(result.nextStep.contains("compatibility"))
    }

    func testKeychainUnavailableExplainsDeviceUnlockAndActualRelaunchBoundary() {
        let result = AuthenticationRecoveryPresenter.recovery(.secureStore, storageFailure: .unavailable)
        XCTAssertEqual(result.category, .secureStoreUnavailable)
        XCTAssertTrue(result.nextStep.contains("Unlock the device"))
        XCTAssertTrue(result.nextStep.contains("reopen Synveil"))
        XCTAssertNil(result.primaryAction)
    }

    func testCorruptRecordFailsClosedAndDoesNotSuggestOverwrite() {
        let result = AuthenticationRecoveryPresenter.recovery(.secureStore, storageFailure: .corruptRecord)
        XCTAssertEqual(result.category, .corruptRecord)
        XCTAssertTrue(result.nextStep.contains("will not be silently replaced"))
        XCTAssertTrue(result.sessionProtection.contains("blocked"))
        XCTAssertNil(result.primaryAction)
    }

    func testUnsupportedRecordHasDistinctFailClosedPresentation() {
        let result = AuthenticationRecoveryPresenter.recovery(.secureStore, storageFailure: .unsupportedRecord)
        XCTAssertEqual(result.category, .unsupportedRecord)
        XCTAssertTrue(result.nextStep.contains("up to date"))
        XCTAssertNil(result.primaryAction)
    }

    func testScopeMismatchOffersNoMigrationAndNoDifferentOriginRequest() {
        let result = AuthenticationRecoveryPresenter.recovery(.scopeMismatch)
        XCTAssertTrue(result.message.contains("does not match"))
        XCTAssertTrue(result.sessionProtection.contains("not been sent"))
        XCTAssertTrue(result.nextStep.contains("Automatic migration is unavailable"))
        XCTAssertNil(result.primaryAction)
    }

    func testCleanupFailureNeverClaimsLogoutSuccess() {
        let result = AuthenticationRecoveryPresenter.logoutCleanup
        XCTAssertEqual(result.category, .logoutCleanup)
        XCTAssertTrue(result.message.contains("Logout has not completed"))
        XCTAssertTrue(result.sessionProtection.contains("remains blocked"))
        XCTAssertEqual(result.primaryAction, .retryCleanup)
    }

    func testPresentationIgnoresSecretBearingStateStrings() {
        let secret = "sve1_" + String(repeating: "a", count: 64)
        for state in [
            .invalidToken(secret), .rejected(secret), .recoveryRequired(secret),
            .secureStoreUnavailable(secret), .securityServicesUnavailable(secret),
        ] as [EnrollmentUIState] {
            let result = AuthenticationRecoveryPresenter.enrollment(state, inputIsEmpty: false, requestID: secret)!
            XCTAssertFalse(result.accessibilityDescription.contains(secret))
            XCTAssertNil(result.requestID)
        }
    }

    func testUntrustedRequestIDsAreNotDisplayed() {
        for value in [
            "svd1_" + String(repeating: "f", count: 64),
            "request-sve1_12345678", "Authorization", "Bearer-secret", String(repeating: "a", count: 64), "request\n1234",
            "<script>request</script>", "https://private.example/token", String(repeating: "x", count: 129), "tiny",
        ] {
            XCTAssertNil(AuthenticationRecoveryPresenter.safeRequestID(value))
        }
    }

    func testBoundedAllowlistedRequestIDMayBeDisplayed() {
        let result = AuthenticationRecoveryPresenter.enrollment(
            .rejected("ignored"), inputIsEmpty: false, requestID: "request-123")!
        XCTAssertEqual(result.requestID, "request-123")
    }

    func testPreauthPresentationDoesNotEchoArbitraryServerCodeOrContentType() {
        for state in [
            .aliveButNotReady(requestId: "request-123", code: "svd1_secret"),
            .failed(.httpError(statusCode: 401, code: "device_revoked", requestId: "sve1_secret")),
            .failed(.unexpectedContentType(contentType: "svd1_secret")),
        ] as [ServerValidationUIState] {
            let result = AuthenticationRecoveryPresenter.serverValidation(state)!
            XCTAssertFalse(result.accessibilityDescription.contains("svd1_"))
            XCTAssertFalse(result.accessibilityDescription.contains("sve1_"))
            XCTAssertFalse(result.accessibilityDescription.contains("revoked"))
            XCTAssertTrue(result.sessionProtection.contains("until the server is verified ready"))
        }
    }

    func testCancelledVerificationDoesNotReportPermanentAuthenticationFailure() {
        let result = AuthenticationRecoveryPresenter.restoration(.cancelled)
        XCTAssertEqual(result.category, .verificationPending)
        XCTAssertEqual(result.primaryAction, .retryVerification)
        XCTAssertTrue(result.message.contains("does not mean your authorization was rejected"))
        XCTAssertNil(AuthenticationRecoveryPresenter.serverValidation(.failed(.cancelled)))
    }

    func testStatusAndActionsHaveReadableAccessibilityDescriptionsAndStableIDs() {
        let reasons: [AppRecoveryReason] = [
            .configuration, .authentication, .deviceRevoked, .secureStore, .credential,
            .scopeMismatch, .tls, .protocolFailure, .enrollmentAmbiguous, .transport,
        ]
        var ids = Set<String>()
        for reason in reasons {
            let result = AuthenticationRecoveryPresenter.recovery(reason)
            XCTAssertFalse(result.accessibilityDescription.isEmpty)
            XCTAssertTrue(result.accessibilityDescription.contains(result.title))
            XCTAssertTrue(ids.insert(result.accessibilityIdentifier).inserted)
        }
        for action in [.retryVerification, .retryCleanup, .retryServerValidation] as [AuthenticationRecoveryAction] {
            XCTAssertFalse(action.label.isEmpty)
            XCTAssertFalse(action.hint.isEmpty)
            XCTAssertTrue(action.accessibilityIdentifier.hasPrefix("synveil."))
        }
    }

    @MainActor
    func testRecoveryViewsRenderAtAccessibilityDynamicTypeSizes() {
        let presentation = AuthenticationRecoveryPresenter.recovery(.enrollmentAmbiguous)
        let standard = UIHostingController(
            rootView: AuthenticationRecoveryMessageView(presentation: presentation)
                .environment(\.dynamicTypeSize, .large))
        let large = UIHostingController(
            rootView: AuthenticationRecoveryMessageView(presentation: presentation)
                .environment(\.dynamicTypeSize, .accessibility5))
        let proposal = CGSize(width: 320, height: 10_000)
        let standardSize = standard.sizeThatFits(in: proposal)
        let largeSize = large.sizeThatFits(in: proposal)
        XCTAssertGreaterThan(largeSize.height, standardSize.height)
        XCTAssertLessThanOrEqual(largeSize.width, proposal.width)
        XCTAssertNotNil(AuthenticationRecoveryView(presentation: presentation).body)
    }

    @MainActor
    func testControllerRetainsTypedStorageDistinctionsWithoutNetworkRetry() async {
        let cases: [(SessionRestorationResult, SessionSecureStorageFailure)] = [
            (.keychainUnavailable, .unavailable), (.keychainReadFailure, .readFailure),
            (.keychainFailure, .failure), (.corruptedKeychainSession, .corruptRecord),
            (.unsupportedKeychainFormat(999), .unsupportedRecord),
        ]
        for (result, expected) in cases {
            let service = RecoveryRestorationStub(results: [result])
            let controller = SessionController(configuration: .init(serverEndpoint: nil), restorationService: service)
            await controller.start()
            XCTAssertEqual(controller.state, .recoveryRequired(.secureStore))
            XCTAssertEqual(controller.secureStorageFailure, expected)
            await controller.retrySessionRestoration()
            let calls = await service.callCount()
            XCTAssertEqual(calls, 1)
            controller.requireRecovery(.authentication)
            XCTAssertNil(controller.secureStorageFailure)
        }
    }

    @MainActor
    func testCleanupRetryUsesEstablishedServiceAndKeepsReadinessGate() async {
        let store = RecoveryCleanupStore()
        let endpoint = try! ServerEndpoint(validating: "https://recovery.synveil.example")
        let controller = SessionController(
            configuration: .init(serverEndpoint: endpoint),
            restorationService: RecoveryRestorationStub(results: [.cancelled]),
            logoutService: SessionLogoutService(credentialStore: store))
        await controller.start()
        await controller.requestLogout()
        XCTAssertEqual(controller.state, .logoutCleanupRequired)
        XCTAssertNotNil(controller.logoutFailure)
        XCTAssertNotNil(LogoutCleanupRecoveryView(sessionController: controller).body)
        await controller.requestLogout()
        let deletes = await store.deleteCount()
        XCTAssertEqual(deletes, 2)
        XCTAssertEqual(controller.state, .readyForServerValidation)
        XCTAssertNil(controller.logoutFailure)
    }

    @MainActor
    func testPermanentRecoveryRenderingAndRetryCannotTriggerEnrollmentOrCleanup() async {
        let store = RecoveryCleanupStore()
        let service = RecoveryRestorationStub(results: [.cancelled])
        let controller = SessionController(
            configuration: .init(serverEndpoint: nil), restorationService: service,
            logoutService: SessionLogoutService(credentialStore: store))
        for reason in [.authentication, .deviceRevoked, .enrollmentAmbiguous, .scopeMismatch] as [AppRecoveryReason] {
            controller.requireRecovery(reason)
            XCTAssertNotNil(RootView(sessionController: controller).body)
            await controller.retrySessionRestoration()
            await controller.requestLogout()
            controller.requireEnrollment()
            XCTAssertEqual(controller.state, .recoveryRequired(reason))
        }
        let deletes = await store.deleteCount()
        let restores = await service.callCount()
        XCTAssertEqual(deletes, 0)
        XCTAssertEqual(restores, 0)
    }

    @MainActor
    func testRestorationFailureContextClearsAfterSuccessfulVerification() async throws {
        let session = try makeSession()
        let service = RecoveryRestorationStub(results: [
            .locallyValidStoredSession(session, verification: .timeout),
            .remotelyVerifiedAuthorizedSession(session),
        ])
        let controller = SessionController(configuration: .init(serverEndpoint: nil), restorationService: service)
        await controller.start()
        XCTAssertEqual(controller.restorationFailure, .timeout)
        await controller.retrySessionRestoration()
        XCTAssertEqual(controller.state, .authenticated)
        XCTAssertNil(controller.restorationFailure)
        XCTAssertNil(controller.secureStorageFailure)
    }

    @MainActor
    func testRestorationFailureContextClearsWhenLogoutStarts() async throws {
        let session = try makeSession()
        let controller = SessionController(
            configuration: .init(serverEndpoint: nil),
            restorationService: RecoveryRestorationStub(results: [.locallyValidStoredSession(session, verification: .dnsFailure)]))
        await controller.start()
        XCTAssertEqual(controller.restorationFailure, .dnsFailure)
        await controller.requestLogout()
        XCTAssertEqual(controller.state, .logoutCleanupRequired)
        XCTAssertNil(controller.restorationFailure)
    }

    @MainActor
    func testCancelledOrStaleServerCheckCannotReplaceNewerRetry() async {
        let service = RecoveryServerStub()
        let controller = configuredController()
        let model = ServerValidationViewModel(sessionController: controller, validationService: service)
        model.validateServer()
        await service.waitForCalls(1)
        let firstCompletion = model.currentValidationTask
        model.cancelValidation()
        model.retry()
        await service.waitForCalls(2)
        await service.complete(1, with: .failure(.timeout))
        await firstCompletion?.value
        XCTAssertTrue(model.isChecking)
        XCTAssertEqual(model.state, .checking)
        await service.complete(2, with: .ready(livenessRequestId: nil, readinessRequestId: nil))
        await model.waitForCurrentValidation()
        XCTAssertEqual(model.state, .ready)
        XCTAssertEqual(controller.state, .needsEnrollment)
    }

    @MainActor
    func testStaleFailureCannotOverwriteSuccessfulNewerServerCheck() async {
        let service = RecoveryServerStub()
        let controller = configuredController()
        let model = ServerValidationViewModel(sessionController: controller, validationService: service)
        model.validateServer()
        await service.waitForCalls(1)
        let firstTask = model.currentValidationTask
        model.cancelValidation()
        model.retry()
        await service.waitForCalls(2)
        await service.complete(2, with: .ready(livenessRequestId: nil, readinessRequestId: nil))
        await model.waitForCurrentValidation()
        await service.complete(1, with: .failure(.timeout))
        await firstTask?.value
        XCTAssertEqual(model.state, .ready)
        XCTAssertEqual(controller.state, .needsEnrollment)
        XCTAssertNil(model.recoveryPresentation)
        XCTAssertFalse(model.isChecking)
    }

    @MainActor
    func testDuplicateServerRetryCannotStartAnotherOperation() async {
        let service = RecoveryServerStub()
        let model = ServerValidationViewModel(sessionController: configuredController(), validationService: service)
        model.validateServer()
        await service.waitForCalls(1)
        model.retry()
        let calls = await service.callCount()
        XCTAssertEqual(calls, 1)
        await service.complete(1, with: .failure(.offline))
        await model.waitForCurrentValidation()
        XCTAssertEqual(model.recoveryPresentation?.category, .offline)
    }

    @MainActor
    func testLateServerFailureCannotOverwriteNewerRootRecovery() async {
        let service = RecoveryServerStub()
        let controller = configuredController()
        let model = ServerValidationViewModel(sessionController: controller, validationService: service)
        model.validateServer()
        await service.waitForCalls(1)
        controller.requireRecovery(.tls)
        await service.complete(1, with: .failure(.timeout))
        await model.waitForCurrentValidation()
        XCTAssertEqual(controller.state, .recoveryRequired(.tls))
        XCTAssertNil(model.recoveryPresentation)
    }

    @MainActor
    func testPreauthRetryIsUnavailableOutsideServerReadinessState() async {
        let service = RecoveryServerStub()
        let controller = configuredController()
        controller.requireRecovery(.authentication)
        let model = ServerValidationViewModel(sessionController: controller, validationService: service)
        model.retry()
        let calls = await service.callCount()
        XCTAssertEqual(calls, 0)
        XCTAssertEqual(controller.state, .recoveryRequired(.authentication))
    }

    @MainActor
    func testMalformedEnrollmentAllowsCorrectionWithoutNetworkRequest() async {
        let service = RecoveryExchangeStub(result: .rejected(code: "invalid_enrollment", requestId: nil))
        let controller = configuredController()
        controller.requireEnrollment()
        let model = EnrollmentViewModel(
            sessionController: controller, exchangeService: service,
            credentialSink: StubSecureCredentialSink(isAvailable: true), rustBridge: RecoveryRustStub())
        model.rawTokenInput = "malformed"
        model.submitEnrollment()
        await model.waitForCurrentSubmission()
        let firstCalls = await service.callCount()
        XCTAssertEqual(firstCalls, 0)
        XCTAssertEqual(model.recoveryPresentation?.category, .malformedToken)
        model.rawTokenInput = "sve1_" + String(repeating: "a", count: 64)
        XCTAssertEqual(model.state, .idle)
        model.submitEnrollment()
        await model.waitForCurrentSubmission()
        let correctedCalls = await service.callCount()
        XCTAssertEqual(correctedCalls, 1)
        XCTAssertEqual(model.recoveryPresentation?.category, .enrollmentRejected)
        XCTAssertTrue(model.rawTokenInput.isEmpty)
    }

    @MainActor
    func testAmbiguousExchangeAndReconstructedViewCannotReplayGrant() async {
        let service = RecoveryExchangeStub(result: .recoveryRequired(.timeout))
        let controller = configuredController()
        controller.requireEnrollment()
        let model = EnrollmentViewModel(
            sessionController: controller, exchangeService: service,
            credentialSink: StubSecureCredentialSink(isAvailable: true), rustBridge: RecoveryRustStub())
        model.rawTokenInput = "sve1_" + String(repeating: "a", count: 64)
        model.submitEnrollment()
        await model.waitForCurrentSubmission()
        XCTAssertEqual(controller.state, .recoveryRequired(.enrollmentAmbiguous))
        XCTAssertNil(model.recoveryPresentation?.primaryAction)
        XCTAssertNotNil(RootView(sessionController: controller).body)
        model.submitEnrollment()
        let calls = await service.callCount()
        XCTAssertEqual(calls, 1)
    }

    @MainActor
    func testLateEnrollmentErrorCannotOverwriteNewerRecovery() async {
        let service = RecoveryExchangeStub(result: .recoveryRequired(.timeout), suspended: true)
        let controller = configuredController()
        controller.requireEnrollment()
        let model = EnrollmentViewModel(
            sessionController: controller, exchangeService: service,
            credentialSink: StubSecureCredentialSink(isAvailable: true), rustBridge: RecoveryRustStub())
        model.rawTokenInput = "sve1_" + String(repeating: "a", count: 64)
        model.submitEnrollment()
        await service.waitForCall()
        controller.requireRecovery(.tls)
        await service.complete()
        await model.waitForCurrentSubmission()
        XCTAssertEqual(controller.state, .recoveryRequired(.tls))
        XCTAssertNil(model.recoveryPresentation)
        XCTAssertFalse(model.isSubmitting)
    }

    @MainActor
    func testEnrollmentSubmissionFromRecoveryCannotBypassServerReadiness() async {
        let service = RecoveryExchangeStub(result: .recoveryRequired(.timeout))
        let controller = configuredController()
        let model = EnrollmentViewModel(
            sessionController: controller, exchangeService: service,
            credentialSink: StubSecureCredentialSink(isAvailable: true), rustBridge: RecoveryRustStub())
        model.rawTokenInput = "sve1_" + String(repeating: "a", count: 64)
        model.submitEnrollment()
        let calls = await service.callCount()
        XCTAssertEqual(calls, 0)
        XCTAssertEqual(controller.state, .readyForServerValidation)
    }

    private func assertTransient(_ failure: SessionRestorationVerificationFailure, category: AuthenticationRecoveryCategory) {
        let result = AuthenticationRecoveryPresenter.restoration(failure)
        XCTAssertEqual(result.category, category)
        XCTAssertEqual(result.primaryAction, .retryVerification)
        XCTAssertEqual(result.severity, .temporary)
        XCTAssertTrue(result.sessionProtection.contains("retained"))
        XCTAssertFalse(result.accessibilityDescription.contains("logged out"))
    }

    private func makeSession() throws -> DeviceCredentialSession {
        let record = try DeviceCredentialRecord(
            ownerUserId: "11111111-2222-3333-4444-555555555555",
            deviceId: "66666666-7777-8888-9999-000000000000",
            credentialId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            credential: DeviceCredential(validatedRawValue: "svd1_" + String(repeating: "a", count: 64)),
            createdAt: "2026-10-07T12:00:00Z")
        return DeviceCredentialSession(
            serverEndpoint: try ServerEndpoint(validating: "https://recovery.synveil.example"), record: record)
    }

    @MainActor
    private func configuredController() -> SessionController {
        let controller = SessionController(configuration: .init(serverEndpoint: nil))
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(try! ServerEndpoint(validating: "https://recovery.synveil.example"))
        return controller
    }


}

private actor RecoveryRestorationStub: SessionRestorationServiceProtocol {
    var results: [SessionRestorationResult]
    var calls = 0
    init(results: [SessionRestorationResult]) { self.results = results }
    func restore(configuredServerEndpoint: ServerEndpoint?) async -> SessionRestorationResult {
        calls += 1
        return results.removeFirst()
    }
    func retryVerification(of session: DeviceCredentialSession, expectedServerEndpoint: ServerEndpoint) async -> SessionRestorationResult {
        await restore(configuredServerEndpoint: expectedServerEndpoint)
    }
    func callCount() -> Int { calls }
}

private actor RecoveryCleanupStore: SecureCredentialSinkProtocol {
    var deletes = 0
    func preflight() async throws {}
    func store(_ record: DeviceCredentialRecord, for serverEndpoint: ServerEndpoint) async throws -> SecureCredentialPersistenceReceipt {
        throw SecureCredentialSinkError.writeFailure
    }
    func update(_ record: DeviceCredentialRecord, for serverEndpoint: ServerEndpoint) async throws -> SecureCredentialPersistenceReceipt {
        throw SecureCredentialSinkError.writeFailure
    }
    func load(expectedServerEndpoint: ServerEndpoint?) async throws -> DeviceCredentialSession {
        throw SecureCredentialSinkError.itemNotFound
    }
    func delete() async throws {
        deletes += 1
        if deletes == 1 { throw SecureCredentialSinkError.deletionFailure }
    }
    func isActiveCredentialAbsent() async throws -> Bool { deletes >= 2 }
    func deleteCount() -> Int { deletes }
}

private actor RecoveryServerStub: ServerValidationServiceProtocol {
    var calls = 0
    var continuations: [Int: CheckedContinuation<ConnectionCheckResult, Never>] = [:]
    var waiters: [(Int, CheckedContinuation<Void, Never>)] = []
    func validateServer(endpoint: ServerEndpoint) async -> ConnectionCheckResult {
        calls += 1
        let call = calls
        return await withCheckedContinuation { continuation in
            continuations[call] = continuation
            let ready = waiters.filter { $0.0 <= calls }
            waiters.removeAll { $0.0 <= calls }
            for waiter in ready { waiter.1.resume() }
        }
    }
    func waitForCalls(_ count: Int) async {
        if calls >= count { return }
        await withCheckedContinuation { waiters.append((count, $0)) }
    }
    func complete(_ call: Int, with result: ConnectionCheckResult) {
        continuations.removeValue(forKey: call)?.resume(returning: result)
    }
    func callCount() -> Int { calls }
}

private actor RecoveryExchangeStub: EnrollmentExchangeServiceProtocol {
    let result: EnrollmentExchangeResult
    let suspended: Bool
    var calls = 0
    var continuation: CheckedContinuation<Void, Never>?
    var waiter: CheckedContinuation<Void, Never>?
    init(result: EnrollmentExchangeResult, suspended: Bool = false) {
        self.result = result
        self.suspended = suspended
    }
    func exchange(endpoint: ServerEndpoint, token: EnrollmentToken) async -> EnrollmentExchangeResult {
        calls += 1
        if suspended {
            await withCheckedContinuation { continuation in
                self.continuation = continuation
                waiter?.resume()
                waiter = nil
            }
        }
        return result
    }
    func waitForCall() async {
        if calls > 0 { return }
        await withCheckedContinuation { waiter = $0 }
    }
    func complete() { continuation?.resume(); continuation = nil }
    func callCount() -> Int { calls }
}

private struct RecoveryRustStub: RustBridgeProtocol {
    func parseSHA256(_ canonical: String) async throws -> Data { Data() }
    func formatSHA256(_ digest: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ token: String) async throws -> Bool { EnrollmentToken.isValid(token) }
    func validateDeviceBearerToken(_ token: String) async throws -> Bool { DeviceCredential.isValid(token) }
    func validateLibraryID(_ value: String) async throws -> Bool { false }
    func validateNodeID(_ value: String) async throws -> Bool { false }
    func validateLogicalName(_ value: String) async throws -> Bool { !value.isEmpty }
}

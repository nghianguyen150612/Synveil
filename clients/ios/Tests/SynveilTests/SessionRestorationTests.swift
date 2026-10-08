import XCTest

@testable import Synveil

final class SessionRestorationTests: XCTestCase {
    private let bearer =
        "svd1_abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"

    @MainActor
    func testMissingKeychainItemWithoutEndpointShowsServerSetup() async {
        let store = MockSecureCredentialSink(session: nil)
        let validator = ScriptedAuthorizationValidator()
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: nil),
            restorationService: SessionRestorationService(
                credentialStore: store,
                authorizationValidator: validator
            )
        )

        await controller.start()

        XCTAssertEqual(controller.state, .needsServerProfile)
        let calls = await validator.callCount()
        XCTAssertEqual(calls, 0)
    }

    @MainActor
    func testMissingKeychainItemWithEndpointRequiresServerValidation() async throws {
        let endpoint = try makeEndpoint()
        let store = MockSecureCredentialSink(session: nil)
        let controller = makeController(endpoint: endpoint, store: store)

        await controller.start()

        XCTAssertEqual(controller.state, .readyForServerValidation)
        XCTAssertEqual(controller.serverEndpoint, endpoint)
    }

    @MainActor
    func testStoredCanonicalEndpointIsRecoveredWithoutBootstrapEndpoint() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator(outcomes: [.authorized])
        let controller = makeController(endpoint: nil, store: store, validator: validator)

        await controller.start()

        XCTAssertEqual(controller.serverEndpoint, session.serverEndpoint)
        XCTAssertEqual(controller.state, .authenticated)
    }

    @MainActor
    func testConfiguredMatchingOriginRestoresSession() async throws {
        let endpoint = try makeEndpoint()
        let session = try makeSession(endpoint: endpoint)
        let store = MockSecureCredentialSink(session: session)
        let controller = makeController(
            endpoint: endpoint,
            store: store,
            validator: ScriptedAuthorizationValidator(outcomes: [.authorized])
        )

        await controller.start()

        XCTAssertEqual(controller.state, .authenticated)
        let expectedEndpoints = await store.expectedEndpoints()
        XCTAssertEqual(expectedEndpoints, [endpoint, endpoint])
    }

    @MainActor
    func testConfiguredOriginMismatchFailsClosedBeforeNetworkRequest() async throws {
        let stored = try makeSession(endpoint: makeEndpoint())
        let configured = try ServerEndpoint(validating: "https://other.synveil.example")
        let store = MockSecureCredentialSink(session: stored)
        let validator = ScriptedAuthorizationValidator()
        let controller = makeController(endpoint: configured, store: store, validator: validator)

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.scopeMismatch))
        XCTAssertEqual(controller.serverEndpoint, configured)
        let calls = await validator.callCount()
        XCTAssertEqual(calls, 0)
        let deletes = await store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }

    @MainActor
    func testCorruptKeychainPayloadFailsClosed() async throws {
        let store = MockSecureCredentialSink(error: .corruptPayload)
        let controller = makeController(endpoint: try makeEndpoint(), store: store)

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.secureStore))
    }

    @MainActor
    func testUnsupportedKeychainFormatFailsClosed() async throws {
        let store = MockSecureCredentialSink(error: .unsupportedFormat(2))
        let controller = makeController(endpoint: nil, store: store)

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.secureStore))
    }

    @MainActor
    func testInvalidStoredBearerFailsClosedWithoutNetwork() async throws {
        let store = MockSecureCredentialSink(error: .invalidCredential)
        let validator = ScriptedAuthorizationValidator()
        let controller = makeController(
            endpoint: try makeEndpoint(),
            store: store,
            validator: validator
        )

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.credential))
        let calls = await validator.callCount()
        XCTAssertEqual(calls, 0)
    }

    @MainActor
    func testTemporaryKeychainUnavailabilityRetainsRecoveryState() async throws {
        let store = MockSecureCredentialSink(error: .unavailable)
        let controller = makeController(endpoint: try makeEndpoint(), store: store)

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.secureStore))
        let deletes = await store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }

    @MainActor
    func testReadFailureIsNotTreatedAsMissingSession() async throws {
        let store = MockSecureCredentialSink(error: .readFailure)
        let service = SessionRestorationService(
            credentialStore: store,
            authorizationValidator: ScriptedAuthorizationValidator()
        )
        let result = await service.restore(configuredServerEndpoint: nil)
        XCTAssertEqual(result, .keychainReadFailure)
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: nil),
            restorationService: service
        )

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.secureStore))
    }

    func testRustValidationFailurePreventsAuthorizationRequest() async throws {
        let store = MockSecureCredentialSink(error: .invalidCredential)
        let transport = StubHTTPTransport(response: successfulLibrariesResponse())
        let probe = AuthenticatedSessionValidationService(transport: transport)
        let service = SessionRestorationService(
            credentialStore: store,
            authorizationValidator: probe
        )

        let result = await service.restore(configuredServerEndpoint: try makeEndpoint())

        XCTAssertEqual(result, .invalidCredential)
        let requests = await transport.requests()
        XCTAssertTrue(requests.isEmpty)
    }

    @MainActor
    func testLocallyValidCredentialDoesNotAuthenticateBeforeRemoteResult() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator()
        await validator.suspendNextCall()
        let controller = makeController(endpoint: nil, store: store, validator: validator)

        let startup = Task { await controller.start() }
        await validator.waitForCalls(1)

        XCTAssertEqual(controller.state, .initializing)
        await validator.completeSuspendedCall(with: .authorized)
        await startup.value
        XCTAssertEqual(controller.state, .authenticated)
    }

    func testPublicHealthSuccessDoesNotProveDeviceAuthorization() async throws {
        let transport = StubHTTPTransport(
            response: HTTPTransportResponse(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: Data(#"{"status":"ready"}"#.utf8)
            )
        )
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint())
        )

        XCTAssertEqual(result, .unexpectedProtocolResponse)
    }

    func testProbeConstructsDeviceBearerOnlyInAuthorizationHeader() async throws {
        let transport = StubHTTPTransport(response: successfulLibrariesResponse())
        let probe = AuthenticatedSessionValidationService(transport: transport)
        let session = try makeSession(endpoint: makeEndpoint())

        let result = await probe.validateAuthorization(for: session)

        XCTAssertEqual(result, .authorized)
        let capturedRequest = await transport.lastRequest()
        let request = try XCTUnwrap(capturedRequest)
        XCTAssertEqual(request.headers["Authorization"], "Bearer \(bearer)")
        XCTAssertFalse(request.url.absoluteString.contains(bearer))
        XCTAssertFalse(request.url.query?.contains(bearer) ?? false)
        XCTAssertEqual(request.url.query, "limit=1")
    }

    func testProbeTargetsStoredCanonicalOriginAndIsReadOnly() async throws {
        let endpoint = try ServerEndpoint(validating: "https://sync.synveil.example:8443/vault")
        let transport = StubHTTPTransport(response: successfulLibrariesResponse())
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(for: makeSession(endpoint: endpoint))

        XCTAssertEqual(result, .authorized)
        let capturedRequest = await transport.lastRequest()
        let request = try XCTUnwrap(capturedRequest)
        XCTAssertEqual(request.url.scheme, "https")
        XCTAssertEqual(request.url.host, "sync.synveil.example")
        XCTAssertEqual(request.url.port, 8443)
        XCTAssertEqual(request.url.path, "/vault/api/v1/libraries")
        XCTAssertEqual(request.method, .get)
        XCTAssertNil(request.body)
        let requestCount = await transport.requestCount()
        XCTAssertEqual(requestCount, 1)
    }

    @MainActor
    func testValidAuthenticatedResponseAuthenticatesRestoredSession() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let service = SessionRestorationService(
            credentialStore: MockSecureCredentialSink(session: session),
            authorizationValidator: ScriptedAuthorizationValidator(outcomes: [.authorized])
        )
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: nil),
            restorationService: service
        )

        await controller.start()

        XCTAssertEqual(controller.state, .authenticated)
        XCTAssertEqual(controller.serverEndpoint, session.serverEndpoint)
    }

    @MainActor
    func testAuthenticationFailedDoesNotAuthenticateOrDeleteCredential() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let controller = makeController(
            endpoint: nil,
            store: store,
            validator: ScriptedAuthorizationValidator(outcomes: [.authenticationRejected])
        )

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.authentication))
        XCTAssertFalse(String(describing: controller.state).contains(bearer))
        let deletes = await store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }

    @MainActor
    func testDeviceRevokedEntersRevokedRecoveryWithoutDeletingCredential() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let controller = makeController(
            endpoint: nil,
            store: store,
            validator: ScriptedAuthorizationValidator(outcomes: [.deviceRevoked])
        )

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.deviceRevoked))
        let deletes = await store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }

    func testOldCreationTimestampDoesNotImplyCredentialExpiry() async throws {
        let session = try makeSession(
            endpoint: makeEndpoint(),
            createdAt: "2010-01-01T00:00:00Z"
        )
        let service = SessionRestorationService(
            credentialStore: MockSecureCredentialSink(session: session),
            authorizationValidator: ScriptedAuthorizationValidator(outcomes: [.authorized])
        )

        let result = await service.restore(configuredServerEndpoint: nil)

        guard case .remotelyVerifiedAuthorizedSession = result else {
            return XCTFail("Server authorization, not createdAt, determines validity.")
        }
    }

    @MainActor
    func testOfflineRestorationKeepsCredentialAndOffersRetry() async throws {
        try await assertTransientFailure(
            transportError: .offline,
            expectedVerification: .networkFailure
        )
    }

    @MainActor
    func testDNSFailureKeepsCredentialAndOffersRetry() async throws {
        try await assertTransientFailure(
            transportError: .dnsFailure,
            expectedVerification: .dnsFailure
        )
    }

    @MainActor
    func testTimeoutKeepsCredentialAndOffersRetry() async throws {
        try await assertTransientFailure(
            transportError: .timeout,
            expectedVerification: .timeout
        )
    }

    @MainActor
    func testHTTP503IsServerUnavailableNotRevoked() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let transport = StubHTTPTransport(
            response: errorResponse(status: 503, code: "service_unavailable")
        )
        let service = SessionRestorationService(
            credentialStore: store,
            authorizationValidator: AuthenticatedSessionValidationService(transport: transport)
        )

        let result = await service.restore(configuredServerEndpoint: nil)

        XCTAssertEqual(
            result,
            .locallyValidStoredSession(session, verification: .serverUnavailable)
        )
        let deletes = await store.deleteCount()
        XCTAssertEqual(deletes, 0)
    }

    func testTLSFailureFailsClosed() async throws {
        let probe = AuthenticatedSessionValidationService(
            transport: StubHTTPTransport(error: .tlsError)
        )

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .tlsFailure)
    }

    func testWrongContentTypeIsRejected() async throws {
        let transport = StubHTTPTransport(
            response: HTTPTransportResponse(
                statusCode: 200,
                headers: ["Content-Type": "text/html"],
                body: successfulLibrariesResponse().body
            )
        )
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .unexpectedContentType)
    }

    func testMalformedAuthenticatedResponseIsRejected() async throws {
        let transport = StubHTTPTransport(
            response: HTTPTransportResponse(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: Data(#"{"data":[],"page":{}}"#.utf8)
            )
        )
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .unexpectedProtocolResponse)
    }

    func testAuthenticatedResponseWithUnexpectedLibraryFieldsIsRejected() async throws {
        let body = Data(
            """
            {
              "data": [{
                "id": "11111111-2222-3333-4444-555555555555",
                "type": "library",
                "revision": "1",
                "unexpected": true,
                "attributes": {
                  "name": "Vault",
                  "root_node_id": "66666666-7777-8888-9999-000000000000",
                  "status": "ACTIVE",
                  "created_at": "2026-10-07T12:00:00Z",
                  "updated_at": "2026-10-07T12:00:00Z"
                }
              }],
              "page": {"has_more": false},
              "meta": {"request_id": "request-123"}
            }
            """.utf8
        )
        let transport = StubHTTPTransport(
            response: HTTPTransportResponse(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: body
            )
        )
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .unexpectedProtocolResponse)
    }

    func testAuthenticationFailedEnvelopeIsClassifiedOnlyForHTTP401() async throws {
        let unauthorized = AuthenticatedSessionValidationService(
            transport: StubHTTPTransport(
                response: errorResponse(status: 401, code: "authentication_failed")
            )
        )
        let forbidden = AuthenticatedSessionValidationService(
            transport: StubHTTPTransport(
                response: errorResponse(status: 403, code: "authentication_failed")
            )
        )
        let session = try makeSession(endpoint: makeEndpoint())

        let unauthorizedResult = await unauthorized.validateAuthorization(for: session)
        let forbiddenResult = await forbidden.validateAuthorization(for: session)
        XCTAssertEqual(unauthorizedResult, .authenticationRejected)
        XCTAssertEqual(forbiddenResult, .unexpectedProtocolResponse)
    }

    func testDeviceRevokedEnvelopeIsClassifiedOnlyForHTTP401() async throws {
        let probe = AuthenticatedSessionValidationService(
            transport: StubHTTPTransport(
                response: errorResponse(status: 401, code: "device_revoked")
            )
        )

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .deviceRevoked)
    }

    func testMalformedAuthenticationErrorEnvelopeDoesNotRejectCredential() async throws {
        let transport = StubHTTPTransport(
            response: HTTPTransportResponse(
                statusCode: 401,
                headers: ["Content-Type": "application/json"],
                body: Data(#"{"error":{"code":"authentication_failed"}}"#.utf8)
            )
        )
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .unexpectedProtocolResponse)
    }

    func testRedirectIsRejectedWithoutFollowingIt() async throws {
        let transport = StubHTTPTransport(
            response: HTTPTransportResponse(
                statusCode: 302,
                headers: ["Location": "https://other.synveil.example/api/v1/libraries"],
                body: Data()
            )
        )
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .redirectRejected)
        let requestCount = await transport.requestCount()
        XCTAssertEqual(requestCount, 1)
    }

    func testProbeResponseBodyIsBounded() async throws {
        let transport = StubHTTPTransport(
            response: HTTPTransportResponse(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: Data(repeating: 0x20, count: 16 * 1024 + 1)
            )
        )
        let probe = AuthenticatedSessionValidationService(transport: transport)

        let result = try await probe.validateAuthorization(
            for: makeSession(endpoint: makeEndpoint()))

        XCTAssertEqual(result, .unexpectedProtocolResponse)
    }

    @MainActor
    func testExplicitRetryUsesSameSessionAndPerformsFreshGETWithoutEnrollment() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator(
            outcomes: [.networkFailure, .authorized]
        )
        let controller = makeController(endpoint: nil, store: store, validator: validator)

        await controller.start()
        XCTAssertEqual(controller.state, .restorationVerificationPending)
        await controller.retrySessionRestoration()

        XCTAssertEqual(controller.state, .authenticated)
        let checkedSessions = await validator.checkedSessions()
        XCTAssertEqual(checkedSessions, [session, session])
        let loadCount = await store.loadCount()
        XCTAssertEqual(loadCount, 4)
    }

    @MainActor
    func testRetryDoesNotProbeCredentialThatChangedInKeychain() async throws {
        let endpoint = try makeEndpoint()
        let session = try makeSession(endpoint: endpoint)
        let replacement = try makeSession(
            endpoint: endpoint,
            credentialValue: "svd1_1111111111111111111111111111111111111111111111111111111111111111"
        )
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator(outcomes: [.networkFailure])
        let controller = makeController(endpoint: nil, store: store, validator: validator)
        await controller.start()
        await store.replaceSession(with: replacement)

        await controller.retrySessionRestoration()

        XCTAssertEqual(controller.state, .restorationVerificationPending)
        let checkedSessions = await validator.checkedSessions()
        XCTAssertEqual(checkedSessions, [session])
    }

    @MainActor
    func testRetryActionsDoNotCreateDuplicateConcurrentProbes() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator(outcomes: [.networkFailure])
        let controller = makeController(endpoint: nil, store: store, validator: validator)
        await controller.start()
        await validator.suspendNextCall()

        let firstRetry = Task { await controller.retrySessionRestoration() }
        await validator.waitForCalls(2)
        await controller.retrySessionRestoration()
        XCTAssertTrue(controller.isRestorationRetryInProgress)
        await validator.completeSuspendedCall(with: .authorized)
        await firstRetry.value

        XCTAssertEqual(controller.state, .authenticated)
        let calls = await validator.callCount()
        XCTAssertEqual(calls, 2)
    }

    @MainActor
    func testDuplicateStartupCallsDoNotCreateDuplicateLoadsOrProbes() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator()
        await validator.suspendNextCall()
        let controller = makeController(endpoint: nil, store: store, validator: validator)

        let startup = Task { await controller.start() }
        await validator.waitForCalls(1)
        await controller.start()

        let loads = await store.loadCount()
        let calls = await validator.callCount()
        XCTAssertEqual(loads, 1)
        XCTAssertEqual(calls, 1)
        await validator.completeSuspendedCall(with: .authorized)
        await startup.value
        XCTAssertEqual(controller.state, .authenticated)
    }

    @MainActor
    func testCancellationDuringProbeCannotAuthenticate() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator()
        await validator.suspendNextCall()
        let controller = makeController(endpoint: nil, store: store, validator: validator)

        let startup = Task { await controller.start() }
        await validator.waitForCalls(1)
        startup.cancel()
        await validator.completeSuspendedCall(with: .authorized)
        await startup.value

        XCTAssertEqual(controller.state, .restorationVerificationPending)
    }

    @MainActor
    func testCancellationDuringKeychainLoadCannotAuthenticateOrProbe() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let validator = ScriptedAuthorizationValidator()
        await store.suspendNextLoad()
        let controller = makeController(endpoint: nil, store: store, validator: validator)

        let startup = Task { await controller.start() }
        await store.waitForLoads(1)
        startup.cancel()
        await store.completeSuspendedLoad()
        await startup.value

        XCTAssertEqual(controller.state, .restorationVerificationPending)
        let calls = await validator.callCount()
        XCTAssertEqual(calls, 0)
    }

    @MainActor
    func testStaleSuccessCannotOverwriteNewRecoveryState() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let validator = ScriptedAuthorizationValidator()
        await validator.suspendNextCall()
        let controller = makeController(
            endpoint: nil,
            store: MockSecureCredentialSink(session: session),
            validator: validator
        )

        let startup = Task { await controller.start() }
        await validator.waitForCalls(1)
        controller.requireRecovery(.authentication)
        await validator.completeSuspendedCall(with: .authorized)
        await startup.value

        XCTAssertEqual(controller.state, .recoveryRequired(.authentication))
    }

    @MainActor
    func testSessionIdentityChangeDuringProbeDiscardsStaleSuccess() async throws {
        let endpoint = try makeEndpoint()
        let originalSession = try makeSession(endpoint: endpoint)
        let replacementSession = try makeSession(
            endpoint: endpoint,
            credentialValue: "svd1_1111111111111111111111111111111111111111111111111111111111111111"
        )
        let store = MockSecureCredentialSink(session: originalSession)
        let validator = ScriptedAuthorizationValidator()
        await validator.suspendNextCall()
        let controller = makeController(endpoint: endpoint, store: store, validator: validator)

        let startup = Task { await controller.start() }
        await validator.waitForCalls(1)
        await store.replaceSession(with: replacementSession)
        await validator.completeSuspendedCall(with: .authorized)
        await startup.value

        XCTAssertEqual(controller.state, .restorationVerificationPending)
    }

    @MainActor
    func testRustInitializationFailureFailsClosed() async {
        let controller = SessionController(configuration: AppConfiguration(serverEndpoint: nil))
        controller.markRestorationDependenciesUnavailable()

        await controller.start()

        XCTAssertEqual(controller.state, .recoveryRequired(.secureStore))
    }

    @MainActor
    func testNoEnrollmentTokenIsNeededForRestoration() async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let transport = StubHTTPTransport(response: successfulLibrariesResponse())
        let service = SessionRestorationService(
            credentialStore: MockSecureCredentialSink(session: session),
            authorizationValidator: AuthenticatedSessionValidationService(transport: transport)
        )
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: nil),
            restorationService: service
        )

        await controller.start()

        XCTAssertEqual(controller.state, .authenticated)
        let requests = await transport.requests()
        XCTAssertEqual(requests.count, 1)
        XCTAssertEqual(requests.first?.method, .get)
        XCTAssertEqual(requests.first?.url.path, "/api/v1/libraries")
        XCTAssertNil(requests.first?.body)
        XCTAssertFalse(requests.first?.url.absoluteString.contains("sve1_") ?? true)
    }

    @MainActor
    func testFirstEnrollmentReceiptGateRemainsSeparateFromRestoration() throws {
        let endpoint = try makeEndpoint()
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint)
        )

        controller.markAuthenticated(after: makeReceipt(for: endpoint))
        XCTAssertEqual(controller.state, .initializing)

        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint)
        controller.requireEnrollment()
        controller.markAuthenticated(after: makeReceipt(for: endpoint))
        XCTAssertEqual(controller.state, .authenticated)
    }

    @MainActor
    private func assertTransientFailure(
        transportError: SynveilTransportError,
        expectedVerification: SessionRestorationVerificationFailure
    ) async throws {
        let session = try makeSession(endpoint: makeEndpoint())
        let store = MockSecureCredentialSink(session: session)
        let service = SessionRestorationService(
            credentialStore: store,
            authorizationValidator: AuthenticatedSessionValidationService(
                transport: StubHTTPTransport(error: transportError)
            )
        )
        let controller = SessionController(
            configuration: AppConfiguration(serverEndpoint: nil),
            restorationService: service
        )

        await controller.start()

        XCTAssertEqual(controller.state, .restorationVerificationPending)
        let deletes = await store.deleteCount()
        XCTAssertEqual(deletes, 0)
        let result = await service.restore(configuredServerEndpoint: nil)
        XCTAssertEqual(
            result,
            .locallyValidStoredSession(session, verification: expectedVerification)
        )
    }

    @MainActor
    private func makeController(
        endpoint: ServerEndpoint?,
        store: MockSecureCredentialSink,
        validator: ScriptedAuthorizationValidator = ScriptedAuthorizationValidator()
    ) -> SessionController {
        SessionController(
            configuration: AppConfiguration(serverEndpoint: endpoint),
            restorationService: SessionRestorationService(
                credentialStore: store,
                authorizationValidator: validator
            )
        )
    }

    private func makeEndpoint() throws -> ServerEndpoint {
        try ServerEndpoint(validating: "https://sync.synveil.example:8443")
    }

    private func makeSession(
        endpoint: ServerEndpoint,
        createdAt: String = "2026-10-07T12:00:00Z",
        credentialValue: String? = nil
    ) throws -> DeviceCredentialSession {
        let record = try DeviceCredentialRecord(
            ownerUserId: "11111111-2222-3333-4444-555555555555",
            deviceId: "66666666-7777-8888-9999-000000000000",
            credentialId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            credential: DeviceCredential(validatedRawValue: credentialValue ?? bearer),
            createdAt: createdAt
        )
        return DeviceCredentialSession(serverEndpoint: endpoint, record: record)
    }

    @MainActor
    private func makeReceipt(for endpoint: ServerEndpoint) -> SecureCredentialPersistenceReceipt {
        let record = try! DeviceCredentialRecord(
            ownerUserId: "11111111-2222-3333-4444-555555555555",
            deviceId: "66666666-7777-8888-9999-000000000000",
            credentialId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            credential: DeviceCredential(validatedRawValue: bearer),
            createdAt: "2026-10-07T12:00:00Z"
        )
        return SecureCredentialPersistenceReceipt(
            session: DeviceCredentialSession(serverEndpoint: endpoint, record: record)
        )
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

    private func errorResponse(status: Int, code: String) -> HTTPTransportResponse {
        let body = """
            {
              "error": {
                "code": "\(code)",
                "message": "Authentication state is unavailable.",
                "request_id": "request-123",
                "retryable": false
              }
            }
            """
        return HTTPTransportResponse(
            statusCode: status,
            headers: ["Content-Type": "application/json"],
            body: Data(body.utf8)
        )
    }
}

private actor MockSecureCredentialSink: SecureCredentialSinkProtocol {
    private var session: DeviceCredentialSession?
    private let error: SecureCredentialSinkError?
    private var loadCalls = 0
    private var deleteCalls = 0
    private var requestedEndpoints: [ServerEndpoint?] = []
    private var shouldSuspendNextLoad = false
    private var suspendedLoadContinuation: CheckedContinuation<Void, Never>?
    private var loadWaiters: [(Int, CheckedContinuation<Void, Never>)] = []

    init(
        session: DeviceCredentialSession? = nil,
        error: SecureCredentialSinkError? = nil
    ) {
        self.session = session
        self.error = error
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
        loadCalls += 1
        requestedEndpoints.append(expectedServerEndpoint)
        resumeReadyLoadWaiters()
        if shouldSuspendNextLoad {
            shouldSuspendNextLoad = false
            await withCheckedContinuation { continuation in
                suspendedLoadContinuation = continuation
            }
        }
        if let error {
            throw error
        }
        guard let session else {
            throw SecureCredentialSinkError.itemNotFound
        }
        if let expectedServerEndpoint, expectedServerEndpoint != session.serverEndpoint {
            throw SecureCredentialSinkError.scopeMismatch
        }
        return session
    }

    func delete() async throws {
        deleteCalls += 1
    }

    func loadCount() -> Int { loadCalls }
    func deleteCount() -> Int { deleteCalls }
    func expectedEndpoints() -> [ServerEndpoint?] { requestedEndpoints }
    func replaceSession(with value: DeviceCredentialSession) { session = value }
    func suspendNextLoad() { shouldSuspendNextLoad = true }
    func waitForLoads(_ expectedCount: Int) async {
        guard loadCalls < expectedCount else { return }
        await withCheckedContinuation { continuation in
            loadWaiters.append((expectedCount, continuation))
        }
    }
    func completeSuspendedLoad() {
        suspendedLoadContinuation?.resume()
        suspendedLoadContinuation = nil
    }

    private func resumeReadyLoadWaiters() {
        let ready = loadWaiters.filter { $0.0 <= loadCalls }
        loadWaiters.removeAll { $0.0 <= loadCalls }
        for waiter in ready {
            waiter.1.resume()
        }
    }
}

private actor ScriptedAuthorizationValidator: AuthenticatedSessionValidationProtocol {
    private typealias ValidationContinuation = CheckedContinuation<
        AuthenticatedSessionValidationResult,
        Never
    >

    private var outcomes: [AuthenticatedSessionValidationResult]
    private var calls = 0
    private var sessions: [DeviceCredentialSession] = []
    private var shouldSuspendNextCall = false
    private var suspendedContinuation: ValidationContinuation?
    private var callWaiters: [(Int, CheckedContinuation<Void, Never>)] = []

    init(outcomes: [AuthenticatedSessionValidationResult] = [.authorized]) {
        self.outcomes = outcomes
    }

    func validateAuthorization(
        for session: DeviceCredentialSession
    ) async -> AuthenticatedSessionValidationResult {
        calls += 1
        sessions.append(session)
        resumeReadyWaiters()

        if shouldSuspendNextCall {
            shouldSuspendNextCall = false
            return await withCheckedContinuation { continuation in
                suspendedContinuation = continuation
            }
        }
        guard !outcomes.isEmpty else {
            return .authorized
        }
        return outcomes.removeFirst()
    }

    func suspendNextCall() {
        shouldSuspendNextCall = true
    }

    func waitForCalls(_ expectedCount: Int) async {
        guard calls < expectedCount else { return }
        await withCheckedContinuation { continuation in
            callWaiters.append((expectedCount, continuation))
        }
    }

    func completeSuspendedCall(with result: AuthenticatedSessionValidationResult) {
        suspendedContinuation?.resume(returning: result)
        suspendedContinuation = nil
    }

    func callCount() -> Int { calls }
    func checkedSessions() -> [DeviceCredentialSession] { sessions }

    private func resumeReadyWaiters() {
        let ready = callWaiters.filter { $0.0 <= calls }
        callWaiters.removeAll { $0.0 <= calls }
        for waiter in ready {
            waiter.1.resume()
        }
    }
}

private actor StubHTTPTransport: HTTPTransportProtocol {
    private let response: HTTPTransportResponse?
    private let error: SynveilTransportError?
    private var sentRequests: [HTTPTransportRequest] = []

    init(response: HTTPTransportResponse? = nil, error: SynveilTransportError? = nil) {
        self.response = response
        self.error = error
    }

    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        sentRequests.append(request)
        if let error {
            throw error
        }
        guard let response else {
            throw SynveilTransportError.offline
        }
        return response
    }

    func requests() -> [HTTPTransportRequest] { sentRequests }
    func lastRequest() -> HTTPTransportRequest? { sentRequests.last }
    func requestCount() -> Int { sentRequests.count }
}

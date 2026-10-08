import Foundation
import XCTest

@testable import Synveil

@MainActor
final class EnrollmentTests: XCTestCase {

    private var mockTransport: MockEnrollmentHTTPTransport!
    private var mockEndpoint: ServerEndpoint!

    override func setUp() {
        super.setUp()
        mockTransport = MockEnrollmentHTTPTransport()
        mockEndpoint = try! ServerEndpoint(validating: "https://example.synveil.com")
    }

    override func tearDown() {
        mockTransport = nil
        mockEndpoint = nil
        super.tearDown()
    }

    // MARK: - Test Helpers & Synthetic Values
    private let validSyntheticToken =
        "sve1_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    private let validSyntheticCredential =
        "svd1_abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
    private let validOwnerUserId = "11111111-2222-3333-4444-555555555555"
    private let validDeviceId = "66666666-7777-8888-9999-000000000000"
    private let validCredentialId = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
    private let validCreatedAt = "2026-10-07T12:00:00Z"

    private func makeValidResponseBody(
        ownerUserId: String? = nil,
        deviceId: String? = nil,
        credentialId: String? = nil,
        credential: String? = nil,
        createdAt: String? = nil,
        requestId: String? = "req_12345"
    ) -> Data {
        let json: [String: Any] = [
            "data": [
                "owner_user_id": ownerUserId ?? validOwnerUserId,
                "device_id": deviceId ?? validDeviceId,
                "credential_id": credentialId ?? validCredentialId,
                "device_credential": credential ?? validSyntheticCredential,
                "created_at": createdAt ?? validCreatedAt,
            ],
            "meta": [
                "request_id": requestId as Any
            ],
        ]
        return try! JSONSerialization.data(withJSONObject: json, options: [])
    }

    // 1. Empty token rejected locally
    func test1_emptyTokenRejectedLocally() {
        XCTAssertNil(EnrollmentToken.parse(""))
        XCTAssertFalse(EnrollmentToken.isValid(""))
    }

    // 2. Malformed prefix rejected locally
    func test2_malformedPrefixRejectedLocally() {
        let token = "bad1_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        XCTAssertNil(EnrollmentToken.parse(token))
        XCTAssertFalse(EnrollmentToken.isValid(token))
    }

    // 3. Uppercase/non-lowercase hex rejected
    func test3_uppercaseHexRejected() {
        let token = "sve1_0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF"
        XCTAssertNil(EnrollmentToken.parse(token))
        XCTAssertFalse(EnrollmentToken.isValid(token))
    }

    // 4. Wrong token length rejected
    func test4_wrongTokenLengthRejected() {
        let shortToken = "sve1_0123456789abcdef"
        XCTAssertNil(EnrollmentToken.parse(shortToken))
        XCTAssertFalse(EnrollmentToken.isValid(shortToken))
    }

    // 5. Canonical sve1_ token accepted
    func test5_canonicalTokenAccepted() {
        let token = EnrollmentToken.parse(validSyntheticToken)
        XCTAssertNotNil(token)
        XCTAssertEqual(token?.rawValue, validSyntheticToken)
    }

    // 6. Invalid local token performs zero network requests
    func test6_invalidLocalTokenPerformsZeroNetworkRequests() async {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(mockEndpoint)
        controller.requireEnrollment()

        let viewModel = EnrollmentViewModel(
            sessionController: controller,
            exchangeService: EnrollmentExchangeService(transport: mockTransport)
        )
        viewModel.rawTokenInput = "invalid_token_value"
        viewModel.submitEnrollment()

        // Wait brief tick
        try? await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(mockTransport.sentRequests.count, 0)
        XCTAssertEqual(
            viewModel.state,
            .invalidToken(
                "Enrollment token format is invalid. It must begin with 'sve1_' followed by 64 "
                    + "lowercase hex characters."
            )
        )
    }

    // 7. Correct endpoint path is used
    func test7_correctEndpointPathIsUsed() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        _ = await service.exchange(endpoint: mockEndpoint, token: token)

        XCTAssertEqual(mockTransport.sentRequests.count, 1)
        XCTAssertEqual(
            mockTransport.sentRequests.first?.url.path,
            "/api/v1/device-enrollment/exchange"
        )
    }

    // 8. POST method is used
    func test8_postMethodIsUsed() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        _ = await service.exchange(endpoint: mockEndpoint, token: token)

        XCTAssertEqual(mockTransport.sentRequests.first?.method, .post)
    }

    // 9. JSON request shape contains exactly enrollment_token
    func test9_jsonRequestShapeContainsExactField() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        _ = await service.exchange(endpoint: mockEndpoint, token: token)

        guard let body = mockTransport.sentRequests.first?.body else {
            XCTFail("Missing request body")
            return
        }

        let json = try? JSONSerialization.jsonObject(with: body, options: []) as? [String: Any]
        XCTAssertEqual(json?.count, 1)
        XCTAssertEqual(json?["enrollment_token"] as? String, validSyntheticToken)
    }

    // 10. Token is not placed in URL/query
    func test10_tokenIsNotInURLOrQuery() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        _ = await service.exchange(endpoint: mockEndpoint, token: token)

        guard let requestURL = mockTransport.sentRequests.first?.url else {
            XCTFail("Missing request URL")
            return
        }

        XCTAssertFalse(requestURL.absoluteString.contains(validSyntheticToken))
        XCTAssertNil(requestURL.query)
    }

    // 11. Redirects are rejected
    func test11_redirectsAreRejected() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedError = SynveilTransportError.redirectRejected(statusCode: 302)

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.redirect))
    }

    // 12. No automatic retry after timeout
    func test12_noAutomaticRetryAfterTimeout() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedError = SynveilTransportError.timeout

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.timeout))
        XCTAssertEqual(mockTransport.sentRequests.count, 1)
    }

    // 13. No automatic retry after disconnect
    func test13_noAutomaticRetryAfterDisconnect() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedError = SynveilTransportError.offline

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.disconnect))
        XCTAssertEqual(mockTransport.sentRequests.count, 1)
    }

    // 14. 16 KiB response bound
    func test14_16KiBResponseBound() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        let oversizedBody = Data(repeating: 0x41, count: 16 * 1024 + 1)
        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: oversizedBody
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.oversizedResponse))
    }

    // 15. Wrong Content-Type rejected
    func test15_wrongContentTypeRejected() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "text/html"],
            body: "<html>Error</html>".data(using: .utf8)!
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.wrongContentType))
    }

    // 16. Malformed JSON rejected
    func test16_malformedJSONRejected() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: "{ invalid json".data(using: .utf8)!
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.malformedResponse))
    }

    // 17. HTTP 201 valid credential response succeeds at API-client layer
    func test17_validCredentialResponseSucceeds() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        if case .success(let record) = result {
            XCTAssertEqual(record.ownerUserId, validOwnerUserId)
            XCTAssertEqual(record.deviceId, validDeviceId)
            XCTAssertEqual(record.credentialId, validCredentialId)
            XCTAssertEqual(record.credential.rawValue, validSyntheticCredential)
            XCTAssertEqual(record.createdAt, validCreatedAt)
        } else {
            XCTFail("Expected .success, got \(result)")
        }
    }

    // 18. Malformed svd1_ credential rejected
    func test18_malformedCredentialRejected() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody(credential: "invalid_credential_format")
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.malformedResponse))
    }

    // 19. Malformed owner/device/credential IDs rejected
    func test19_malformedOpaqueIDsRejected() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody(ownerUserId: "not-a-uuid")
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.malformedResponse))
    }

    // 20. Invalid timestamp rejected
    func test20_invalidTimestampRejected() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody(createdAt: "invalid-date")
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.malformedResponse))
    }

    // 21. invalid_enrollment mapped to rejected state
    func test21_invalidEnrollmentMappedToRejected() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        let errorJson = try! JSONSerialization.data(
            withJSONObject: [
                "error": ["code": "invalid_enrollment", "message": "Grant invalid"]
            ]
        )

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 400,
            headers: ["content-type": "application/json"],
            body: errorJson
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        if case .rejected(let code, _) = result {
            XCTAssertEqual(code, "invalid_enrollment")
        } else {
            XCTFail("Expected .rejected, got \(result)")
        }
    }

    // 22. HTTP 503 mapped to recovery-required/ambiguous state
    func test22_http503MappedToRecoveryRequired() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 503,
            headers: ["content-type": "application/json"],
            body: Data()
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.http503))
    }

    // 23. Response loss mapped to recovery-required state
    func test23_responseLossMappedToRecoveryRequired() async {
        let service = EnrollmentExchangeService(transport: mockTransport)
        let token = EnrollmentToken.parse(validSyntheticToken)!

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 500,
            headers: ["content-type": "application/json"],
            body: Data()
        )

        let result = await service.exchange(endpoint: mockEndpoint, token: token)
        XCTAssertEqual(result, .recoveryRequired(.responseLoss))
    }

    // 24. Cancellation cannot authenticate session
    func test24_cancellationCannotAuthenticateSession() async {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(mockEndpoint)
        controller.requireEnrollment()

        let viewModel = EnrollmentViewModel(
            sessionController: controller,
            exchangeService: EnrollmentExchangeService(transport: mockTransport)
        )

        viewModel.rawTokenInput = validSyntheticToken
        viewModel.cancel()

        XCTAssertNotEqual(controller.state, .authenticated)
        XCTAssertEqual(controller.state, .needsEnrollment)
    }

    // 25. Successful exchange alone does not produce .authenticated
    func test25_successfulExchangeAloneDoesNotProduceAuthenticated() async {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(mockEndpoint)
        controller.requireEnrollment()

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        let viewModel = EnrollmentViewModel(
            sessionController: controller,
            exchangeService: EnrollmentExchangeService(transport: mockTransport),
            credentialSink: StubSecureCredentialSink(isAvailable: true)
        )

        viewModel.rawTokenInput = validSyntheticToken
        viewModel.submitEnrollment()

        try? await Task.sleep(nanoseconds: 100_000_000)

        XCTAssertEqual(viewModel.state, .succeeded)
        XCTAssertEqual(controller.state, .needsEnrollment)
        XCTAssertNotEqual(controller.state, .authenticated)
    }

    // 26. Token/credential model descriptions are redacted
    func test26_modelDescriptionsAreRedacted() {
        let token = EnrollmentToken.parse(validSyntheticToken)!
        let credential = DeviceCredential.parse(validSyntheticCredential)!

        XCTAssertEqual(token.description, "[REDACTED_ENROLLMENT_TOKEN]")
        XCTAssertEqual(token.debugDescription, "[REDACTED_ENROLLMENT_TOKEN]")
        XCTAssertEqual("\(token)", "[REDACTED_ENROLLMENT_TOKEN]")

        XCTAssertEqual(credential.description, "[REDACTED_DEVICE_CREDENTIAL]")
        XCTAssertEqual(credential.debugDescription, "[REDACTED_DEVICE_CREDENTIAL]")
        XCTAssertEqual("\(credential)", "[REDACTED_DEVICE_CREDENTIAL]")
    }

    // 27. Duplicate submit is prevented
    func test27_duplicateSubmitIsPrevented() async {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(mockEndpoint)
        controller.requireEnrollment()

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        let viewModel = EnrollmentViewModel(
            sessionController: controller,
            exchangeService: EnrollmentExchangeService(transport: mockTransport),
            credentialSink: StubSecureCredentialSink(isAvailable: true)
        )

        viewModel.rawTokenInput = validSyntheticToken
        viewModel.submitEnrollment()
        viewModel.submitEnrollment()  // Immediate duplicate

        try? await Task.sleep(nanoseconds: 100_000_000)

        XCTAssertEqual(mockTransport.sentRequests.count, 1)
    }

    // 28. Prompt023 state transition remains intact
    func test28_prompt023StateTransitionIntact() {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(mockEndpoint)
        XCTAssertEqual(controller.state, .readyForServerValidation)

        controller.requireEnrollment()
        XCTAssertEqual(controller.state, .needsEnrollment)
    }

    // 29. Production orchestration does not consume real grant without secure-storage readiness
    func test29_preflightPreventsGrantConsumptionWhenSecureStorageFails() async {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(mockEndpoint)
        controller.requireEnrollment()

        mockTransport.stubbedResponse = HTTPTransportResponse(
            statusCode: 201,
            headers: ["content-type": "application/json"],
            body: makeValidResponseBody()
        )

        let viewModel = EnrollmentViewModel(
            sessionController: controller,
            exchangeService: EnrollmentExchangeService(transport: mockTransport)
        )

        viewModel.rawTokenInput = validSyntheticToken
        viewModel.submitEnrollment()

        try? await Task.sleep(nanoseconds: 100_000_000)

        XCTAssertEqual(mockTransport.sentRequests.count, 0)
        XCTAssertEqual(
            viewModel.state,
            .secureStoreUnavailable(
                "Secure credential storage is unavailable. No enrollment request was sent."
            )
        )
    }

    // 30. Retry behavior never silently replays an ambiguous single-shot exchange
    func test30_retryDoesNotReplaySameExchange() async {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(mockEndpoint)
        controller.requireEnrollment()

        mockTransport.stubbedError = SynveilTransportError.timeout

        let viewModel = EnrollmentViewModel(
            sessionController: controller,
            exchangeService: EnrollmentExchangeService(transport: mockTransport),
            credentialSink: StubSecureCredentialSink(isAvailable: true)
        )

        viewModel.rawTokenInput = validSyntheticToken
        viewModel.submitEnrollment()

        try? await Task.sleep(nanoseconds: 100_000_000)

        XCTAssertEqual(
            viewModel.state,
            .recoveryRequired(
                "The enrollment result is unknown. Use the trusted owner recovery workflow before "
                    + "trying again."
            )
        )
        XCTAssertEqual(mockTransport.sentRequests.count, 1)
    }
}

// MARK: - Mock HTTP Transport
private class MockEnrollmentHTTPTransport: HTTPTransportProtocol, @unchecked Sendable {
    var sentRequests: [HTTPTransportRequest] = []
    var stubbedResponse: HTTPTransportResponse?
    var stubbedError: Error?

    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        sentRequests.append(request)

        if let error = stubbedError {
            throw error
        }

        if let response = stubbedResponse {
            return response
        }

        return HTTPTransportResponse(statusCode: 200, headers: [:], body: Data())
    }
}

import Foundation
import XCTest
@testable import Synveil

final class ServerReachabilityTests: XCTestCase {
    // MARK: - Test Mock Transport

    private final class MockHTTPTransport: HTTPTransportProtocol, @unchecked Sendable {
        struct MockResponse {
            let statusCode: Int
            let headers: [String: String]
            let body: Data
        }

        var responses: [String: Result<MockResponse, SynveilTransportError>] = [:]
        var recordedRequests: [HTTPTransportRequest] = []

        func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
            recordedRequests.append(request)
            let path = request.url.path

            guard let handler = responses[path] else {
                throw SynveilTransportError.offline
            }

            switch handler {
            case .success(let resp):
                return HTTPTransportResponse(
                    statusCode: resp.statusCode,
                    headers: resp.headers,
                    body: resp.body
                )
            case .failure(let err):
                throw err
            }
        }
    }

    // MARK: - Helper Constructors

    private func makeEndpoint(
        _ urlString: String = "https://synveil.example.com"
    ) -> ServerEndpoint {
        try! ServerEndpoint(validating: urlString)
    }

    private func makeJsonData(_ string: String) -> Data {
        string.data(using: .utf8)!
    }

    private func makeConfiguredSessionController(
        endpoint: ServerEndpoint? = nil
    ) -> SessionController {
        let controller = SessionController()
        controller.showServerProfileSetup()
        controller.configureServerEndpoint(endpoint ?? makeEndpoint())
        return controller
    }

    // MARK: - 1. /health/live valid 200 {"status":"live"} succeeds

    func test01_livenessSuccess() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: [
                    "Content-Type": "application/json",
                    "X-Request-Id": "req_live_12345678",
                ],
                body: makeJsonData("{\"status\":\"live\"}")
            )
        )
        mock.responses["/health/ready"] = .success(
            .init(
                statusCode: 200,
                headers: [
                    "Content-Type": "application/json",
                    "X-Request-Id": "req_ready_12345678",
                ],
                body: makeJsonData("{\"status\":\"ready\"}")
            )
        )

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        if case .ready(let liveReqId, let readyReqId) = result {
            XCTAssertEqual(liveReqId, "req_live_12345678")
            XCTAssertEqual(readyReqId, "req_ready_12345678")
        } else {
            XCTFail("Expected .ready result but got \(result)")
        }
    }

    // MARK: - 2. Liveness failure prevents readiness request

    func test02_livenessFailureStopsReadiness() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.offline)

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.offline))
        XCTAssertEqual(mock.recordedRequests.count, 1)
        XCTAssertEqual(mock.recordedRequests.first?.url.path, "/health/live")
    }

    // MARK: - 3. Readiness is called only after successful liveness

    func test03_readinessCalledOnlyAfterSuccessfulLiveness() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"live\"}")
            )
        )
        mock.responses["/health/ready"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"ready\"}")
            )
        )

        let service = ServerValidationService(transport: mock)
        _ = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(mock.recordedRequests.count, 2)
        XCTAssertEqual(mock.recordedRequests[0].url.path, "/health/live")
        XCTAssertEqual(mock.recordedRequests[1].url.path, "/health/ready")
    }

    // MARK: - 4. /health/ready valid 200 {"status":"ready"} produces ready success

    func test04_readinessSuccess() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"live\"}")
            )
        )
        mock.responses["/health/ready"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"ready\"}")
            )
        )

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        if case .ready = result {
            // Success
        } else {
            XCTFail("Expected .ready but got \(result)")
        }
    }

    // MARK: - 5. Valid readiness 503 maps to alive-but-not-ready

    func test05_readiness503MapsToAliveButNotReady() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"live\"}")
            )
        )
        let errJson = """
            {
              "error": {
                "code": "service_initializing",
                "message": "Server warming up",
                "request_id": "req_503_12345678",
                "retryable": true
              }
            }
            """
        mock.responses["/health/ready"] = .success(
            .init(
                statusCode: 503,
                headers: [
                    "Content-Type": "application/json",
                    "X-Request-Id": "req_503_12345678",
                ],
                body: makeJsonData(errJson)
            )
        )

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        if case .aliveButNotReady(let reqId, let code) = result {
            XCTAssertEqual(reqId, "req_503_12345678")
            XCTAssertEqual(code, "service_initializing")
        } else {
            XCTFail("Expected .aliveButNotReady but got \(result)")
        }
    }

    // MARK: - 6. DNS failure classification

    func test06_dnsFailureClassification() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.dnsFailure)

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.dnsFailure))
    }

    // MARK: - 7. Timeout classification

    func test07_timeoutClassification() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.timeout)

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.timeout))
    }

    // MARK: - 8. Connection/offline classification

    func test08_offlineClassification() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.offline)

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.offline))
    }

    // MARK: - 9. TLS failure classification

    func test09_tlsErrorClassification() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.tlsError)

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.tlsError))
    }

    // MARK: - 10. Redirects are rejected rather than followed

    func test10_redirectsRejected() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.redirectRejected(statusCode: 302))

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.redirectRejected(statusCode: 302)))
    }

    // MARK: - 11. Unexpected content type is rejected

    func test11_unexpectedContentTypeRejected() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "text/html"],
                body: makeJsonData("<html><body>200 OK</body></html>")
            )
        )

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.unexpectedContentType(contentType: "text/html")))
    }

    // MARK: - 12. Body larger than 64 KiB is rejected

    func test12_bodyLargerThan64KBRejected() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.bodyLimitExceeded)

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.bodyLimitExceeded))
    }

    // MARK: - 13. Malformed JSON is rejected

    func test13_malformedJsonRejected() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{ invalid json }")
            )
        )

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.protocolError(.invalidJSON)))
    }

    // MARK: - 14. Wrong health status string is rejected

    func test14_wrongHealthStatusRejected() async {
        let mock = MockHTTPTransport()
        // /health/live returns "ready" instead of "live"
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"ready\"}")
            )
        )

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        XCTAssertEqual(result, .failure(.protocolError(.invalidHealthStatus)))
    }

    // MARK: - 15. Generic HTTP errors are not interpreted as success

    func test15_genericHttpErrorHandled() async {
        let mock = MockHTTPTransport()
        let errJson = """
            {
              "error": {
                "code": "internal_error",
                "message": "Internal server error",
                "request_id": "req_500_12345678",
                "retryable": true
              }
            }
            """
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 500,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData(errJson)
            )
        )

        let service = ServerValidationService(transport: mock)
        let result = await service.validateServer(endpoint: makeEndpoint())

        if case .failure(.httpError(let statusCode, let code, let reqId)) = result {
            XCTAssertEqual(statusCode, 500)
            XCTAssertEqual(code, "internal_error")
            XCTAssertEqual(reqId, "req_500_12345678")
        } else {
            XCTFail("Expected .failure(.httpError) but got \(result)")
        }
    }

    // MARK: - 16. Cancellation does not advance session state

    @MainActor
    func test16_cancellationDoesNotAdvanceSessionState() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.cancelled)

        let sessionController = makeConfiguredSessionController()
        XCTAssertEqual(sessionController.state, .readyForServerValidation)

        let service = ServerValidationService(transport: mock)
        let viewModel = ServerValidationViewModel(
            sessionController: sessionController,
            validationService: service
        )

        viewModel.validateServer()
        viewModel.cancelValidation()

        // Give async tasks time to settle
        try? await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(sessionController.state, .readyForServerValidation)
    }

    // MARK: - 17. Retry starts a fresh probe sequence

    @MainActor
    func test17_retryStartsFreshProbeSequence() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.offline)

        let sessionController = makeConfiguredSessionController()

        let service = ServerValidationService(transport: mock)
        let viewModel = ServerValidationViewModel(
            sessionController: sessionController,
            validationService: service
        )

        viewModel.validateServer()
        try? await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(mock.recordedRequests.count, 1)

        // Update mock to succeed on second attempt
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"live\"}")
            )
        )
        mock.responses["/health/ready"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"ready\"}")
            )
        )

        viewModel.retry()
        try? await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(mock.recordedRequests.count, 3)  // 1 initial + 2 retry probes
        XCTAssertEqual(viewModel.state, .ready)
        XCTAssertEqual(sessionController.state, .needsEnrollment)
    }

    // MARK: - 18. Valid full check transitions .readyForServerValidation -> .needsEnrollment

    @MainActor
    func test18_validFullCheckTransitionsToNeedsEnrollment() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"live\"}")
            )
        )
        mock.responses["/health/ready"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"ready\"}")
            )
        )

        let sessionController = makeConfiguredSessionController()
        XCTAssertEqual(sessionController.state, .readyForServerValidation)

        let service = ServerValidationService(transport: mock)
        let viewModel = ServerValidationViewModel(
            sessionController: sessionController,
            validationService: service
        )

        viewModel.validateServer()
        try? await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(viewModel.state, .ready)
        XCTAssertEqual(sessionController.state, .needsEnrollment)
    }

    // MARK: - 19. Failed check remains pre-enrollment

    @MainActor
    func test19_failedCheckRemainsPreEnrollment() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .failure(.offline)

        let sessionController = makeConfiguredSessionController()
        XCTAssertEqual(sessionController.state, .readyForServerValidation)

        let service = ServerValidationService(transport: mock)
        let viewModel = ServerValidationViewModel(
            sessionController: sessionController,
            validationService: service
        )

        viewModel.validateServer()
        try? await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertEqual(viewModel.state, .failed(.offline))
        XCTAssertEqual(sessionController.state, .readyForServerValidation)
    }

    // MARK: - 20. Server validation never produces .authenticated

    @MainActor
    func test20_serverValidationNeverProducesAuthenticated() async {
        let mock = MockHTTPTransport()
        mock.responses["/health/live"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"live\"}")
            )
        )
        mock.responses["/health/ready"] = .success(
            .init(
                statusCode: 200,
                headers: ["Content-Type": "application/json"],
                body: makeJsonData("{\"status\":\"ready\"}")
            )
        )

        let sessionController = makeConfiguredSessionController()

        let service = ServerValidationService(transport: mock)
        let viewModel = ServerValidationViewModel(
            sessionController: sessionController,
            validationService: service
        )

        viewModel.validateServer()
        try? await Task.sleep(nanoseconds: 50_000_000)

        XCTAssertNotEqual(sessionController.state, .authenticated)
        XCTAssertEqual(sessionController.state, .needsEnrollment)
    }
}

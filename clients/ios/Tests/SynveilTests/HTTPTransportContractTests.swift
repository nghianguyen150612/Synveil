import XCTest

#if canImport(Foundation)
    import Foundation
#endif

// Access Domain transport contracts
@testable import Synveil

final class HTTPTransportContractTests: XCTestCase {

    private enum TestError: Error, Equatable {
        case networkUnavailable
        case timeout
    }

    private struct StubHTTPTransport: HTTPTransportProtocol {
        var handler: @Sendable (HTTPTransportRequest) async throws -> HTTPTransportResponse

        func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
            try await handler(request)
        }
    }

    func testHTTPMethodRawValuesAndCases() {
        XCTAssertEqual(HTTPMethod.get.rawValue, "GET")
        XCTAssertEqual(HTTPMethod.post.rawValue, "POST")
        XCTAssertEqual(HTTPMethod.put.rawValue, "PUT")
        XCTAssertEqual(HTTPMethod.patch.rawValue, "PATCH")
        XCTAssertEqual(HTTPMethod.delete.rawValue, "DELETE")
        XCTAssertEqual(HTTPMethod.head.rawValue, "HEAD")

        XCTAssertEqual(HTTPMethod.allCases.count, 6)
    }

    func testHTTPTransportRequestDefaultInitialization() throws {
        let url = try XCTUnwrap(URL(string: "https://example.com/api/v1/health"))
        let request = HTTPTransportRequest(url: url)

        XCTAssertEqual(request.url, url)
        XCTAssertEqual(request.method, .get)
        XCTAssertTrue(request.headers.isEmpty)
        XCTAssertNil(request.body)
    }

    func testHTTPTransportRequestFullValuePreservation() throws {
        let url = try XCTUnwrap(URL(string: "https://example.com/api/v1/enroll"))
        let headers = [
            "Content-Type": "application/json",
            "Authorization": "Bearer sve1_testtoken123",
        ]
        let bodyData = Data("{\"code\":\"123456\"}".utf8)

        let request = HTTPTransportRequest(
            url: url,
            method: .post,
            headers: headers,
            body: bodyData
        )

        XCTAssertEqual(request.url, url)
        XCTAssertEqual(request.method, .post)
        XCTAssertEqual(request.headers, headers)
        XCTAssertEqual(request.body, bodyData)
    }

    func testHTTPTransportResponseValuePreservation() {
        let headers = ["Content-Type": "application/json"]
        let bodyData = Data("{\"status\":\"ok\"}".utf8)

        let response = HTTPTransportResponse(
            statusCode: 200,
            headers: headers,
            body: bodyData
        )

        XCTAssertEqual(response.statusCode, 200)
        XCTAssertEqual(response.headers, headers)
        XCTAssertEqual(response.body, bodyData)
    }

    func testStubTransportAsyncRequestResponseFlow() async throws {
        let expectedURL = try XCTUnwrap(URL(string: "https://example.com/api/v1/status"))
        let expectedResponse = HTTPTransportResponse(
            statusCode: 200,
            headers: ["X-Synveil-Server": "v0.1.0"],
            body: Data("{\"healthy\":true}".utf8)
        )

        let transport = StubHTTPTransport { request in
            XCTAssertEqual(request.url, expectedURL)
            XCTAssertEqual(request.method, .get)
            return expectedResponse
        }

        let request = HTTPTransportRequest(url: expectedURL, method: .get)
        let response = try await transport.send(request)

        XCTAssertEqual(response, expectedResponse)
        XCTAssertEqual(response.statusCode, 200)
        XCTAssertEqual(response.body, expectedResponse.body)
    }

    func testStubTransportErrorPropagation() async {
        let transport = StubHTTPTransport { _ in
            throw TestError.networkUnavailable
        }

        let url = URL(string: "https://example.com/api/v1/fail")!
        let request = HTTPTransportRequest(url: url)

        do {
            _ = try await transport.send(request)
            XCTFail("Expected StubHTTPTransport to throw an error")
        } catch let error as TestError {
            XCTAssertEqual(error, TestError.networkUnavailable)
        } catch {
            XCTFail("Unexpected error type thrown: \(error)")
        }
    }
}

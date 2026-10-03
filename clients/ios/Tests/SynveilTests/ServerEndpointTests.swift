import XCTest

@testable import Synveil

final class ServerEndpointTests: XCTestCase {

    // MARK: - Valid Endpoint Tests

    func testValidStandardHTTPSEndpoint() throws {
        let endpoint = try ServerEndpoint(validating: "https://example.com")
        XCTAssertEqual(endpoint.scheme, "https")
        XCTAssertEqual(endpoint.host, "example.com")
        XCTAssertNil(endpoint.port)
        XCTAssertEqual(endpoint.path, "/")
        XCTAssertEqual(endpoint.urlString, "https://example.com/")
        XCTAssertTrue(endpoint.isSecureScheme)
    }

    func testValidHTTPEndpointForLocalDev() throws {
        let endpoint = try ServerEndpoint(validating: "http://192.0.2.1:8080")
        XCTAssertEqual(endpoint.scheme, "http")
        XCTAssertEqual(endpoint.host, "192.0.2.1")
        XCTAssertEqual(endpoint.port, 8080)
        XCTAssertEqual(endpoint.path, "/")
        XCTAssertEqual(endpoint.urlString, "http://192.0.2.1:8080/")
        XCTAssertFalse(endpoint.isSecureScheme)
    }

    func testValidEndpointWithExplicitPort() throws {
        let endpoint = try ServerEndpoint(validating: "https://cloud.example.com:8443")
        XCTAssertEqual(endpoint.scheme, "https")
        XCTAssertEqual(endpoint.host, "cloud.example.com")
        XCTAssertEqual(endpoint.port, 8443)
        XCTAssertEqual(endpoint.urlString, "https://cloud.example.com:8443/")
    }

    func testValidEndpointWithSubpath() throws {
        let endpoint = try ServerEndpoint(validating: "https://example.com/synveil")
        XCTAssertEqual(endpoint.path, "/synveil")
        XCTAssertEqual(endpoint.urlString, "https://example.com/synveil")
    }

    func testWhitespaceTrimmingAndTrailingSlashNormalization() throws {
        let endpoint = try ServerEndpoint(validating: "  https://example.com/  \n")
        XCTAssertEqual(endpoint.urlString, "https://example.com/")

        let endpointWithPath = try ServerEndpoint(validating: "https://example.com/api/v1/")
        XCTAssertEqual(endpointWithPath.path, "/api/v1")
        XCTAssertEqual(endpointWithPath.urlString, "https://example.com/api/v1")
    }

    func testCaseNormalizationForSchemeAndHost() throws {
        let endpoint = try ServerEndpoint(validating: "HTTPS://SERVER.EXAMPLE:8443/Path")
        XCTAssertEqual(endpoint.scheme, "https")
        XCTAssertEqual(endpoint.host, "server.example")
        XCTAssertEqual(endpoint.port, 8443)
        XCTAssertEqual(endpoint.path, "/Path")
        XCTAssertEqual(endpoint.urlString, "https://server.example:8443/Path")
    }

    // MARK: - Invalid Endpoint Tests

    func testRejectEmptyOrWhitespaceInput() {
        XCTAssertThrowsError(try ServerEndpoint(validating: "")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .empty)
        }

        XCTAssertThrowsError(try ServerEndpoint(validating: "   \n\t ")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .empty)
        }
    }

    func testRejectMissingOrUnsupportedScheme() {
        XCTAssertThrowsError(try ServerEndpoint(validating: "example.com")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .unsupportedScheme(""))
        }

        XCTAssertThrowsError(try ServerEndpoint(validating: "ftp://example.com")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .unsupportedScheme("ftp"))
        }

        XCTAssertThrowsError(try ServerEndpoint(validating: "file:///path/to/file")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .unsupportedScheme("file"))
        }
    }

    func testRejectMissingHost() {
        XCTAssertThrowsError(try ServerEndpoint(validating: "https:///path")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .missingHost)
        }
    }

    func testRejectUserinfoCredentials() {
        XCTAssertThrowsError(try ServerEndpoint(validating: "https://user:password@example.com")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .userinfoNotAllowed)
        }

        XCTAssertThrowsError(try ServerEndpoint(validating: "https://user@example.com")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .userinfoNotAllowed)
        }
    }

    func testRejectQueryParameters() {
        XCTAssertThrowsError(try ServerEndpoint(validating: "https://example.com/?token=secret")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .queryNotAllowed)
        }
    }

    func testRejectURLFragments() {
        XCTAssertThrowsError(try ServerEndpoint(validating: "https://example.com/#section")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .fragmentNotAllowed)
        }
    }

    // MARK: - Value Semantics & Conformance Tests

    func testEqualityAndHashing() throws {
        let ep1 = try ServerEndpoint(validating: "https://example.com")
        let ep2 = try ServerEndpoint(validating: "https://example.com/")
        let ep3 = try ServerEndpoint(validating: "https://other.example.com")

        XCTAssertEqual(ep1, ep2)
        XCTAssertNotEqual(ep1, ep3)
        XCTAssertEqual(ep1.hashValue, ep2.hashValue)
    }

    func testCustomStringConvertible() throws {
        let endpoint = try ServerEndpoint(validating: "https://example.com:8443")
        XCTAssertEqual(String(describing: endpoint), "https://example.com:8443/")
    }
}

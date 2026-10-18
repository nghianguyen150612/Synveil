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
        let endpoint = try ServerEndpoint(
            validating: "http://127.0.0.1:8080", allowLoopbackTestHTTP: true)
        XCTAssertEqual(endpoint.scheme, "http")
        XCTAssertEqual(endpoint.host, "127.0.0.1")
        XCTAssertEqual(endpoint.port, 8080)
        XCTAssertEqual(endpoint.path, "/")
        XCTAssertEqual(endpoint.urlString, "http://127.0.0.1:8080/")
        XCTAssertFalse(endpoint.isSecureScheme)

        let ipv6Endpoint = try ServerEndpoint(
            validating: "http://[::1]", allowLoopbackTestHTTP: true)
        XCTAssertEqual(ipv6Endpoint.urlString, "http://[::1]/")
    }

    func testValidEndpointWithExplicitPort() throws {
        let endpoint = try ServerEndpoint(validating: "https://cloud.example.com:8443")
        XCTAssertEqual(endpoint.scheme, "https")
        XCTAssertEqual(endpoint.host, "cloud.example.com")
        XCTAssertEqual(endpoint.port, 8443)
        XCTAssertEqual(endpoint.urlString, "https://cloud.example.com:8443/")
    }

    func testRootSlashNormalizationAndWhitespaceRejection() throws {
        XCTAssertEqual(
            try ServerEndpoint(validating: "https://example.com/").urlString,
            "https://example.com/"
        )
        for value in [" https://example.com", "https://example.com ", "https://example.com\n"] {
            XCTAssertThrowsError(try ServerEndpoint(validating: value))
        }
    }

    func testCaseNormalizationForSchemeAndHost() throws {
        let endpoint = try ServerEndpoint(validating: "HTTPS://SERVER.EXAMPLE:8443/")
        XCTAssertEqual(endpoint.scheme, "https")
        XCTAssertEqual(endpoint.host, "server.example")
        XCTAssertEqual(endpoint.port, 8443)
        XCTAssertEqual(endpoint.path, "/")
        XCTAssertEqual(endpoint.urlString, "https://server.example:8443/")
    }

    // MARK: - Invalid Endpoint Tests

    func testRejectEmptyOrWhitespaceInput() {
        XCTAssertThrowsError(try ServerEndpoint(validating: "")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .empty)
        }

        XCTAssertThrowsError(try ServerEndpoint(validating: "   \n\t ")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .controlOrWhitespace)
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
        let credentialURL = "https://user:password@example.com"
        XCTAssertThrowsError(try ServerEndpoint(validating: credentialURL)) { error in
            XCTAssertEqual(error as? EndpointValidationError, .userinfoNotAllowed)
        }

        let userOnlyURL = "https://user@example.com"
        XCTAssertThrowsError(try ServerEndpoint(validating: userOnlyURL)) { error in
            XCTAssertEqual(error as? EndpointValidationError, .userinfoNotAllowed)
        }
    }

    func testRejectQueryParameters() {
        let queryURL = "https://example.com/?token=secret"
        XCTAssertThrowsError(try ServerEndpoint(validating: queryURL)) { error in
            XCTAssertEqual(error as? EndpointValidationError, .queryNotAllowed)
        }
    }

    func testRejectURLFragments() {
        let fragmentURL = "https://example.com/#section"
        XCTAssertThrowsError(try ServerEndpoint(validating: fragmentURL)) { error in
            XCTAssertEqual(error as? EndpointValidationError, .fragmentNotAllowed)
        }
    }

    func testRejectsNonOriginPathsAndEmptyQueryOrFragment() {
        for value in [
            "https://example.com/api/v1", "https://example.com?", "https://example.com#",
        ] {
            XCTAssertThrowsError(try ServerEndpoint(validating: value))
        }
    }

    func testHTTPRequiresExplicitDebugPolicyAndNumericLoopback() {
        XCTAssertThrowsError(
            try ServerEndpoint(
                validating: "http://127.0.0.1", allowLoopbackTestHTTP: false
            )
        ) { error in
            XCTAssertEqual(error as? EndpointValidationError, .httpsRequired)
        }

        for value in [
            "http://localhost",
            "http://192.168.1.10",
            "http://10.0.0.1",
        ] {
            XCTAssertThrowsError(
                try ServerEndpoint(validating: value, allowLoopbackTestHTTP: true)
            ) { error in
                XCTAssertEqual(error as? EndpointValidationError, .loopbackHTTPNotAllowed)
            }
        }
    }

    func testRejectsMalformedAndOutOfRangePorts() {
        for value in [
            "https://example.com:",
            "https://example.com:0",
            "https://example.com:65536",
            "https://example.com:abc",
        ] {
            XCTAssertThrowsError(try ServerEndpoint(validating: value))
        }
    }

    func testRejectsMalformedNumericIPv4Hosts() {
        for value in [
            "https://999.999.999.999",
            "https://127.1",
            "https://127.0.0.1.",
        ] {
            XCTAssertThrowsError(try ServerEndpoint(validating: value))
        }
    }

    func testRejectsOriginsOverAndroidByteLimit() {
        let longHost = String(repeating: "a", count: 2_049)
        XCTAssertThrowsError(try ServerEndpoint(validating: "https://\(longHost)")) { error in
            XCTAssertEqual(error as? EndpointValidationError, .tooLong)
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

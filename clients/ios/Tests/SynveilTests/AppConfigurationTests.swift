import XCTest

@testable import Synveil

final class AppConfigurationTests: XCTestCase {

    func testSafeProductionDefaultIsUnconfigured() {
        let config = AppConfiguration()
        XCTAssertEqual(config.environment, .production)
        XCTAssertNil(
            config.serverEndpoint,
            "Production configuration must default to nil serverEndpoint"
        )
    }

    func testExplicitConfigurationInjection() throws {
        let endpoint = try ServerEndpoint(validating: "https://server.example")
        let config = AppConfiguration(environment: .testing, serverEndpoint: endpoint)

        XCTAssertEqual(config.environment, .testing)
        XCTAssertEqual(config.serverEndpoint, endpoint)
    }

    func testLoadFromProcessEnvironmentWithDefaults() {
        let emptyEnv: [String: String] = [:]
        let config = AppConfiguration.load(processEnvironment: emptyEnv)

        XCTAssertEqual(config.environment, .production)
        XCTAssertNil(config.serverEndpoint)
    }

    func testLoadFromProcessEnvironmentWithOverrides() throws {
        let customEnv = [
            AppConfiguration.environmentNameKey: "development",
            AppConfiguration.environmentServerURLKey: "https://dev.example:8443",
        ]

        let config = AppConfiguration.load(processEnvironment: customEnv)
        XCTAssertEqual(config.environment, .development)
        XCTAssertNotNil(config.serverEndpoint)
        XCTAssertEqual(config.serverEndpoint?.urlString, "https://dev.example:8443/")
    }

    func testLoadFromProcessEnvironmentWithInvalidURLIgnoresMalformedEndpoint() {
        let invalidEnv = [
            AppConfiguration.environmentServerURLKey: "not-a-valid-url"
        ]

        let config = AppConfiguration.load(processEnvironment: invalidEnv)
        XCTAssertEqual(config.environment, .production)
        XCTAssertNil(
            config.serverEndpoint,
            "Malformed process environment URL must fail safely and yield nil"
        )
    }

    func testValueSemanticsEquality() throws {
        let ep = try ServerEndpoint(validating: "https://example.com")
        let config1 = AppConfiguration(environment: .production, serverEndpoint: ep)
        let config2 = AppConfiguration(environment: .production, serverEndpoint: ep)

        XCTAssertEqual(config1, config2)
        XCTAssertEqual(config1.hashValue, config2.hashValue)
    }
}

import XCTest

@testable import Synveil

final class RustBridgeABITests: XCTestCase {
    func testRustBridgeABIVersionInitialization() throws {
        let adapter = try RustBridgeAdapter()
        XCTAssertEqual(adapter.abiVersion, 1)
    }

    func testRustBridgeExpectedABIVersionConstant() {
        XCTAssertEqual(RustBridgeAdapter.expectedABIVersion, 1)
    }

    func testRustBridgeInvalidArgumentFromRealRust() {
        XCTAssertThrowsError(try RustBridgeAdapter(expectedABIVersion: 0)) { error in
            XCTAssertEqual(error as? RustBridgeError, RustBridgeError.invalidArgument)
        }
    }

    func testRustBridgeUnsupportedABIVersionFromRealRust() {
        XCTAssertThrowsError(try RustBridgeAdapter(expectedABIVersion: 2)) { error in
            guard case .unsupportedABIVersion(let expected, let actual) = error as? RustBridgeCompatibilityError else {
                XCTFail("Expected RustBridgeCompatibilityError.unsupportedABIVersion, got \(error)")
                return
            }
            XCTAssertEqual(expected, 2)
            XCTAssertEqual(actual, 1)
        }
    }
}

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
}

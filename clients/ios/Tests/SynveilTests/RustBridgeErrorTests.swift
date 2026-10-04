import XCTest

@testable import Synveil

final class RustBridgeErrorTests: XCTestCase {
    func testSwiftStatusDecodingAllCodes() {
        XCTAssertEqual(RustBridgeStatus(rawValue: 0), .success)
        XCTAssertEqual(RustBridgeStatus(rawValue: 1), .invalidArgument)
        XCTAssertEqual(RustBridgeStatus(rawValue: 2), .invalidUtf8)
        XCTAssertEqual(RustBridgeStatus(rawValue: 3), .bufferTooSmall)
        XCTAssertEqual(RustBridgeStatus(rawValue: 4), .domainError)
        XCTAssertEqual(RustBridgeStatus(rawValue: 5), .internalError)
        XCTAssertEqual(RustBridgeStatus(rawValue: 6), .panicEncountered)
        XCTAssertEqual(RustBridgeStatus(rawValue: 7), .unsupportedABIVersion)
        XCTAssertEqual(RustBridgeStatus(rawValue: 0xFFFF_FFFE), .unknown(0xFFFF_FFFE))
    }

    func testCheckStatusMappingToSwiftErrors() throws {
        // Success path
        XCTAssertNoThrow(try RustBridgeError.checkStatus(0))

        // Mapped error cases
        XCTAssertThrowsError(try RustBridgeError.checkStatus(1)) { err in
            XCTAssertEqual(err as? RustBridgeError, .invalidArgument)
        }
        XCTAssertThrowsError(try RustBridgeError.checkStatus(2)) { err in
            XCTAssertEqual(err as? RustBridgeError, .invalidUtf8)
        }
        XCTAssertThrowsError(try RustBridgeError.checkStatus(3)) { err in
            XCTAssertEqual(err as? RustBridgeError, .bufferTooSmall)
        }
        XCTAssertThrowsError(try RustBridgeError.checkStatus(4)) { err in
            XCTAssertEqual(err as? RustBridgeError, .domainError)
        }
        XCTAssertThrowsError(try RustBridgeError.checkStatus(5)) { err in
            XCTAssertEqual(err as? RustBridgeError, .internalError)
        }
        XCTAssertThrowsError(try RustBridgeError.checkStatus(6)) { err in
            XCTAssertEqual(err as? RustBridgeError, .panicEncountered)
        }

        // Unknown raw status
        XCTAssertThrowsError(try RustBridgeError.checkStatus(0xDEAD_BEEF)) { err in
            XCTAssertEqual(err as? RustBridgeError, .unknownStatus(0xDEAD_BEEF))
        }
    }
}

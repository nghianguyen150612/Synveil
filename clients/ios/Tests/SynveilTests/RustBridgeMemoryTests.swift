import Foundation
import XCTest
@testable import Synveil

final class RustBridgeMemoryTests: XCTestCase {

    func testSHA256ParseSuccess() throws {
        let adapter = try RustBridgeAdapter()
        let canonical =
            "sha256:abababababababababababababababababababababababababababababababab"

        let digest = try adapter.parseSHA256(canonical)

        XCTAssertEqual(digest.count, 32)
        XCTAssertEqual(digest, Data(repeating: 0xab, count: 32))
    }

    func testSHA256FormatSuccess() throws {
        let adapter = try RustBridgeAdapter()
        let digest = Data(repeating: 0xab, count: 32)

        let canonical = try adapter.formatSHA256(digest)

        XCTAssertEqual(
            canonical,
            "sha256:abababababababababababababababababababababababababababababababab"
        )
    }

    func testSHA256RoundTrip() throws {
        let adapter = try RustBridgeAdapter()
        let original =
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

        let digest = try adapter.parseSHA256(original)
        let formatted = try adapter.formatSHA256(digest)

        XCTAssertEqual(formatted, original)
    }

    func testSHA256DomainErrorOnMalformedInput() throws {
        let adapter = try RustBridgeAdapter()
        let malformed = "sha256:invalid_hex_characters_here"

        XCTAssertThrowsError(try adapter.parseSHA256(malformed)) { error in
            XCTAssertEqual(error as? RustBridgeError, .domainError)
        }
    }

    func testSHA256InvalidUtf8Input() throws {
        let adapter = try RustBridgeAdapter()
        let invalidUtf8Bytes: [UInt8] = [0xFF, 0xFE, 0xFD]

        XCTAssertThrowsError(try adapter.parseSHA256UTF8Bytes(invalidUtf8Bytes)) { error in
            XCTAssertEqual(error as? RustBridgeError, .invalidUtf8)
        }
    }

    func testSHA256WrongDigestLengthError() throws {
        let adapter = try RustBridgeAdapter()
        let shortDigest = Data(repeating: 0xab, count: 31)
        let longDigest = Data(repeating: 0xab, count: 33)

        XCTAssertThrowsError(try adapter.formatSHA256(shortDigest)) { error in
            XCTAssertEqual(error as? RustBridgeError, .domainError)
        }

        XCTAssertThrowsError(try adapter.formatSHA256(longDigest)) { error in
            XCTAssertEqual(error as? RustBridgeError, .domainError)
        }
    }

    func testPostReleaseSwiftDataOwnership() throws {
        let adapter = try RustBridgeAdapter()
        let canonical =
            "sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"

        let digest = try adapter.parseSHA256(canonical)
        let formatted = try adapter.formatSHA256(digest)

        // Force Swift heap/data access multiple times after FFI release completed
        for _ in 0..<100 {
            XCTAssertEqual(digest.count, 32)
            XCTAssertEqual(digest.first, 0xcd)
            XCTAssertEqual(formatted, canonical)
            XCTAssertTrue(formatted.hasPrefix("sha256:"))
        }
    }

    func testRepeatedLifecycleSimulatorIterations() throws {
        let adapter = try RustBridgeAdapter()
        let canonical =
            "sha256:11223344556677889900aabbccddeeff11223344556677889900aabbccddeeff"

        for _ in 0..<1_000 {
            let digest = try adapter.parseSHA256(canonical)
            XCTAssertEqual(digest.count, 32)

            let formatted = try adapter.formatSHA256(digest)
            XCTAssertEqual(formatted, canonical)
        }
    }
}

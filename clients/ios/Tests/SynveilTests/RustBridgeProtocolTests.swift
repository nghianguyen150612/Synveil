import XCTest

@testable import Synveil

/// Test-only stub conforming to `RustBridgeProtocol` proving Application components
/// can be unit tested without initializing raw FFI or linking C binaries.
final class MockRustBridge: RustBridgeProtocol, Sendable {
    func parseSHA256(_ canonical: String) async throws -> Data {
        if canonical.hasPrefix("sha256:") && canonical.count == 71 {
            return Data(repeating: 0xAB, count: 32)
        }
        throw RustBridgeError.domainError
    }

    func formatSHA256(_ digest: Data) async throws -> String {
        guard digest.count == 32 else {
            throw RustBridgeError.domainError
        }
        return "sha256:" + String(repeating: "ab", count: 32)
    }

    func validateEnrollmentToken(_ token: String) async throws -> Bool {
        token.hasPrefix("sve1_") && token.count == 69
    }

    func validateDeviceBearerToken(_ token: String) async throws -> Bool {
        token.hasPrefix("svd1_") && token.count == 69
    }

    func validateLibraryID(_ value: String) async throws -> Bool {
        value.count == 36 && value.hasPrefix("018")
    }

    func validateNodeID(_ value: String) async throws -> Bool {
        value.count == 36 && value.hasPrefix("018")
    }

    func validateLogicalName(_ value: String) async throws -> Bool {
        !value.isEmpty && value.utf8.count <= 1024
    }
}

final class RustBridgeProtocolTests: XCTestCase {
    func testProtocolMockabilityWithoutFFI() async throws {
        let bridge: any RustBridgeProtocol = MockRustBridge()

        let validEnrollment = "sve1_" + String(repeating: "a1", count: 32)
        let isValidEnrollment = try await bridge.validateEnrollmentToken(validEnrollment)
        XCTAssertTrue(isValidEnrollment)

        let invalidEnrollment = "invalid_token"
        let isInvalidEnrollment = try await bridge.validateEnrollmentToken(invalidEnrollment)
        XCTAssertFalse(isInvalidEnrollment)

        let validBearer = "svd1_" + String(repeating: "b2", count: 32)
        let isValidBearer = try await bridge.validateDeviceBearerToken(validBearer)
        XCTAssertTrue(isValidBearer)

        let hexStr = String(repeating: "ab", count: 32)
        let parsedData = try await bridge.parseSHA256("sha256:" + hexStr)
        XCTAssertEqual(parsedData.count, 32)

        let formatted = try await bridge.formatSHA256(Data(repeating: 0xAB, count: 32))
        XCTAssertEqual(formatted, "sha256:" + String(repeating: "ab", count: 32))
    }

    @MainActor
    func testRealProtocolExistentialCallOnSimulatorAndMainActor() async throws {
        let bridge: any RustBridgeProtocol = try await RustBridgeAsyncAdapter()

        // 1. Enrollment token validation
        let validEnrollment = "sve1_" + String(repeating: "a1", count: 32)
        let isValidEnrollment = try await bridge.validateEnrollmentToken(validEnrollment)
        XCTAssertTrue(isValidEnrollment)

        let invalidEnrollmentPrefix = "svd1_" + String(repeating: "a1", count: 32)
        let isInvalidEnrollment = try await bridge.validateEnrollmentToken(invalidEnrollmentPrefix)
        XCTAssertFalse(isInvalidEnrollment)

        // 2. Device bearer token validation
        let validBearer = "svd1_" + String(repeating: "b2", count: 32)
        let isValidBearer = try await bridge.validateDeviceBearerToken(validBearer)
        XCTAssertTrue(isValidBearer)

        let isCrossPurposeBearerInvalid = try await bridge
            .validateDeviceBearerToken(validEnrollment)
        XCTAssertFalse(isCrossPurposeBearerInvalid)

        // 3. ID Validation (LibraryId & NodeId)
        let validUUIDv7 = "018f9b9f-5c21-722e-8b1a-9f4a0b2c3d4e"
        let isValidLibID = try await bridge.validateLibraryID(validUUIDv7)
        XCTAssertTrue(isValidLibID)

        let isValidNodeID = try await bridge.validateNodeID(validUUIDv7)
        XCTAssertTrue(isValidNodeID)

        let uppercaseUUID = validUUIDv7.uppercased()
        XCTAssertFalse(try await bridge.validateLibraryID(uppercaseUUID))

        let uuidV4 = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"
        XCTAssertFalse(try await bridge.validateLibraryID(uuidV4))

        // 4. LogicalName Validation
        XCTAssertTrue(try await bridge.validateLogicalName("hello.txt"))
        XCTAssertTrue(try await bridge.validateLogicalName("A/B"))
        XCTAssertTrue(try await bridge.validateLogicalName("synveil_🚀_doc.pdf"))
        XCTAssertFalse(try await bridge.validateLogicalName(""))

        let maxName = String(repeating: "a", count: 1024)
        XCTAssertTrue(try await bridge.validateLogicalName(maxName))

        let overMaxName = String(repeating: "a", count: 1025)
        XCTAssertFalse(try await bridge.validateLogicalName(overMaxName))

        // Multibyte Unicode boundary: 1023 'a's + 4-byte emoji = 1027 bytes -> false
        let unicodeOver = String(repeating: "a", count: 1023) + "🚀"
        XCTAssertEqual(unicodeOver.utf8.count, 1027)
        XCTAssertFalse(try await bridge.validateLogicalName(unicodeOver))

        // 5. SHA256 parse / format
        let canonicalSha = "sha256:" + String(repeating: "12", count: 32)
        let parsedSha = try await bridge.parseSHA256(canonicalSha)
        XCTAssertEqual(parsedSha.count, 32)
        let formattedSha = try await bridge.formatSHA256(parsedSha)
        XCTAssertEqual(formattedSha, canonicalSha)
    }
}

import Foundation
import SynveilRustFFI

/// Swift-only compatibility error for Rust FFI bridge probing.
public enum RustBridgeCompatibilityError: Error, Equatable, Sendable {
    case unsupportedABIVersion(expected: UInt32, actual: UInt32)
}

/// Infrastructure adapter wrapping raw C ABI calls to `synveil-ios-ffi`.
public struct RustBridgeAdapter: Sendable {
    /// Expected C ABI version for Synveil iOS v0.1.
    public static let expectedABIVersion: UInt32 = 1

    /// Actual C ABI version reported by the linked Rust static library.
    public let abiVersion: UInt32

    /// Initializes the adapter and verifies C ABI version compatibility against `expectedABIVersion`.
    public init() throws {
        try self.init(expectedABIVersion: Self.expectedABIVersion)
    }

    /// Internal initializer allowing expected ABI version injection for ABI compatibility testing.
    init(expectedABIVersion: UInt32) throws {
        let actual = synveil_ffi_abi_version()
        guard actual != 0 else {
            throw RustBridgeCompatibilityError.unsupportedABIVersion(
                expected: expectedABIVersion,
                actual: 0
            )
        }

        let rawStatus = synveil_ffi_validate_abi_version(expectedABIVersion)
        if rawStatus == UInt32(SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION) {
            throw RustBridgeCompatibilityError.unsupportedABIVersion(
                expected: expectedABIVersion,
                actual: actual
            )
        }

        try RustBridgeError.checkStatus(rawStatus)
        self.abiVersion = actual
    }

    // MARK: - Safe Buffer Consumption Helper

    /// Helper that manages a local `SynveilFfiBuffer`, invokes a Rust FFI producer call,
    /// defensively validates returned buffer shape, copies bytes into Swift-owned `Data`,
    /// and safely invokes `synveil_ffi_buffer_release`.
    ///
    /// Raw pointers do not escape this method call.
    private func consumeRustBuffer(
        _ operation: (UnsafeMutablePointer<SynveilFfiBuffer>) -> UInt32
    ) throws -> Data {
        var rawBuffer = SynveilFfiBuffer(data: nil, len: 0, capacity: 0)

        let rawStatus = operation(&rawBuffer)
        try RustBridgeError.checkStatus(rawStatus)

        // Validate returned buffer shape
        let isZero =
            rawBuffer.data == nil && rawBuffer.len == 0 && rawBuffer.capacity == 0

        if isZero {
            return Data()
        }

        guard let dataPtr = rawBuffer.data, rawBuffer.len > 0,
            rawBuffer.capacity >= rawBuffer.len
        else {
            // Buffer shape invalid despite status success
            _ = synveil_ffi_buffer_release(&rawBuffer)
            throw RustBridgeError.internalError
        }

        guard rawBuffer.len <= Int.max else {
            _ = synveil_ffi_buffer_release(&rawBuffer)
            throw RustBridgeError.internalError
        }

        let lengthInt = Int(rawBuffer.len)
        let swiftData = Data(bytes: dataPtr, count: lengthInt)

        let releaseStatus = synveil_ffi_buffer_release(&rawBuffer)
        try RustBridgeError.checkStatus(releaseStatus)

        return swiftData
    }

    // MARK: - Infrastructure SHA-256 Operations

    /// Parses a canonical SHA-256 string (e.g. `sha256:<64 hex chars>`) using real Rust shared core
    /// and returns the 32-byte raw digest as Swift `Data`.
    public func parseSHA256(_ canonical: String) throws -> Data {
        let utf8Bytes = Array(canonical.utf8)
        return try parseSHA256UTF8Bytes(utf8Bytes)
    }

    /// Internal Infrastructure test seam allowing raw byte inputs (including invalid UTF-8)
    /// to be passed to `synveil_ffi_sha256_parse`.
    func parseSHA256UTF8Bytes(_ bytes: [UInt8]) throws -> Data {
        try consumeRustBuffer { outBufferPtr in
            if bytes.isEmpty {
                return synveil_ffi_sha256_parse(nil, 0, outBufferPtr)
            } else {
                return bytes.withUnsafeBufferPointer { bufPtr in
                    synveil_ffi_sha256_parse(bufPtr.baseAddress, bufPtr.count, outBufferPtr)
                }
            }
        }
    }

    /// Formats a 32-byte raw SHA-256 digest `Data` into a canonical `sha256:<64 hex chars>` string.
    public func formatSHA256(_ digest: Data) throws -> String {
        let outputData = try consumeRustBuffer { outBufferPtr in
            if digest.isEmpty {
                return synveil_ffi_sha256_format(nil, 0, outBufferPtr)
            } else {
                return digest.withUnsafeBytes { rawBufPtr in
                    let basePtr = rawBufPtr.bindMemory(to: UInt8.self).baseAddress
                    return synveil_ffi_sha256_format(basePtr, digest.count, outBufferPtr)
                }
            }
        }

        guard let resultString = String(data: outputData, encoding: .utf8) else {
            throw RustBridgeError.internalError
        }

        return resultString
    }
}

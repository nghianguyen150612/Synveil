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

    /// Initializes the adapter and verifies C ABI version compatibility.
    public init() throws {
        let actual = synveil_ffi_abi_version()
        guard actual == Self.expectedABIVersion else {
            throw RustBridgeCompatibilityError.unsupportedABIVersion(
                expected: Self.expectedABIVersion,
                actual: actual
            )
        }
        self.abiVersion = actual
    }
}

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
        if rawStatus == SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION {
            throw RustBridgeCompatibilityError.unsupportedABIVersion(
                expected: expectedABIVersion,
                actual: actual
            )
        }

        try RustBridgeError.checkStatus(rawStatus)
        self.abiVersion = actual
    }
}

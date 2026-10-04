import Foundation
import SynveilRustFFI

/// Swift-only compatibility error for Rust FFI bridge probing.
public enum RustBridgeCompatibilityError: Error, Equatable, Sendable {
    case unsupportedABIVersion(expected: UInt32, actual: UInt32)
}

/// Swift representation of raw uint32 FFI status codes returned by `synveil-ios-ffi`.
public enum RustBridgeStatus: Equatable, Sendable {
    case success
    case invalidArgument
    case invalidUtf8
    case bufferTooSmall
    case domainError
    case internalError
    case panicEncountered
    case unsupportedABIVersion
    case unknown(UInt32)

    /// Decodes a raw u32 FFI status code into a Swift `RustBridgeStatus`.
    public init(rawValue: UInt32) {
        switch rawValue {
        case SYNVEIL_FFI_STATUS_SUCCESS:
            self = .success
        case SYNVEIL_FFI_STATUS_INVALID_ARGUMENT:
            self = .invalidArgument
        case SYNVEIL_FFI_STATUS_INVALID_UTF8:
            self = .invalidUtf8
        case SYNVEIL_FFI_STATUS_BUFFER_TOO_SMALL:
            self = .bufferTooSmall
        case SYNVEIL_FFI_STATUS_DOMAIN_ERROR:
            self = .domainError
        case SYNVEIL_FFI_STATUS_INTERNAL_ERROR:
            self = .internalError
        case SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED:
            self = .panicEncountered
        case SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION:
            self = .unsupportedABIVersion
        default:
            self = .unknown(rawValue)
        }
    }
}

/// Swift error representation for Rust FFI status failures.
public enum RustBridgeError: Error, Equatable, Sendable {
    case invalidArgument
    case invalidUtf8
    case bufferTooSmall
    case domainError
    case internalError
    case panicEncountered
    case unknownStatus(UInt32)

    /// Decodes raw FFI status code and throws a Swift error if non-zero, returning normally on `SUCCESS`.
    public static func checkStatus(_ rawStatus: UInt32) throws {
        let status = RustBridgeStatus(rawValue: rawStatus)
        switch status {
        case .success:
            return
        case .invalidArgument:
            throw RustBridgeError.invalidArgument
        case .invalidUtf8:
            throw RustBridgeError.invalidUtf8
        case .bufferTooSmall:
            throw RustBridgeError.bufferTooSmall
        case .domainError:
            throw RustBridgeError.domainError
        case .internalError:
            throw RustBridgeError.internalError
        case .panicEncountered:
            throw RustBridgeError.panicEncountered
        case .unsupportedABIVersion:
            throw RustBridgeCompatibilityError.unsupportedABIVersion(
                expected: RustBridgeAdapter.expectedABIVersion,
                actual: synveil_ffi_abi_version()
            )
        case .unknown(let code):
            throw RustBridgeError.unknownStatus(code)
        }
    }
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

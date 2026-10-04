import Foundation
import SynveilRustFFI

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

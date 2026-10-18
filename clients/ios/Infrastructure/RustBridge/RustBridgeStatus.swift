import Foundation
import SynveilRustFFI

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
        case UInt32(SYNVEIL_FFI_STATUS_SUCCESS):
            self = .success
        case UInt32(SYNVEIL_FFI_STATUS_INVALID_ARGUMENT):
            self = .invalidArgument
        case UInt32(SYNVEIL_FFI_STATUS_INVALID_UTF8):
            self = .invalidUtf8
        case UInt32(SYNVEIL_FFI_STATUS_BUFFER_TOO_SMALL):
            self = .bufferTooSmall
        case UInt32(SYNVEIL_FFI_STATUS_DOMAIN_ERROR):
            self = .domainError
        case UInt32(SYNVEIL_FFI_STATUS_INTERNAL_ERROR):
            self = .internalError
        case UInt32(SYNVEIL_FFI_STATUS_PANIC_ENCOUNTERED):
            self = .panicEncountered
        case UInt32(SYNVEIL_FFI_STATUS_UNSUPPORTED_ABI_VERSION):
            self = .unsupportedABIVersion
        default:
            self = .unknown(rawValue)
        }
    }
}

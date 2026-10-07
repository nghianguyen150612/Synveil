import Foundation

private let deviceCredentialRegex = try! NSRegularExpression(pattern: "^svd1_[0-9a-f]{64}$")

/// Strongly typed encapsulation of a device bearer credential secret (`svd1_<64 hex chars>`).
///
/// # Security Invariants
/// - Output of `description` and `debugDescription` is explicitly redacted to prevent secret leakage in logs.
public struct DeviceCredential: Sendable, Equatable, CustomStringConvertible, CustomDebugStringConvertible {
    public let rawValue: String

    public var description: String {
        "[REDACTED_DEVICE_CREDENTIAL]"
    }

    public var debugDescription: String {
        "[REDACTED_DEVICE_CREDENTIAL]"
    }

    /// Synchronously validates and constructs a `DeviceCredential`.
    ///
    /// - Parameter rawValue: Raw credential candidate string.
    /// - Returns: `DeviceCredential` if format matches `svd1_[0-9a-f]{64}`, else `nil`.
    public static func parse(_ rawValue: String) -> DeviceCredential? {
        guard isValid(rawValue) else { return nil }
        return DeviceCredential(rawValue: rawValue)
    }

    /// Fast synchronous format check for device bearer credential syntax.
    ///
    /// - Parameter rawValue: Raw credential string to test.
    /// - Returns: `true` if valid 69-character lowercase hex `svd1_` credential.
    public static func isValid(_ rawValue: String) -> Bool {
        let range = NSRange(location: 0, length: rawValue.utf16.count)
        return deviceCredentialRegex.firstMatch(in: rawValue, options: [], range: range) != nil
    }

    /// Constructs a `DeviceCredential` with a credential string.
    ///
    /// - Parameter rawValue: Credential string.
    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    /// Constructs a `DeviceCredential` directly when pre-validated (e.g. by Rust shared core).
    ///
    /// - Parameter rawValue: Validated credential string.
    public init(validatedRawValue rawValue: String) {
        self.rawValue = rawValue
    }
}

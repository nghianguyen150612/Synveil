import Foundation

private let enrollmentTokenRegex = try! NSRegularExpression(pattern: "^sve1_[0-9a-f]{64}$")

/// Strongly typed encapsulation of a single-use enrollment token (`sve1_<64 hex chars>`).
///
/// # Security Invariants
/// - Output of `description` and `debugDescription` is explicitly redacted to prevent secret
///   leakage in logs.
public struct EnrollmentToken: Sendable, Equatable, CustomStringConvertible,
    CustomDebugStringConvertible
{
    public let rawValue: String

    public var description: String {
        "[REDACTED_ENROLLMENT_TOKEN]"
    }

    public var debugDescription: String {
        "[REDACTED_ENROLLMENT_TOKEN]"
    }

    /// Synchronously validates and constructs an `EnrollmentToken`.
    ///
    /// - Parameter rawValue: Raw token candidate string.
    /// - Returns: `EnrollmentToken` if format matches `sve1_[0-9a-f]{64}`, else `nil`.
    public static func parse(_ rawValue: String) -> EnrollmentToken? {
        guard isValid(rawValue) else { return nil }
        return EnrollmentToken(rawValue: rawValue)
    }

    /// Fast synchronous format check for enrollment token syntax.
    ///
    /// - Parameter rawValue: Raw token string to test.
    /// - Returns: `true` if valid 69-character lowercase hex `sve1_` token.
    public static func isValid(_ rawValue: String) -> Bool {
        let range = NSRange(location: 0, length: rawValue.utf16.count)
        return enrollmentTokenRegex.firstMatch(in: rawValue, options: [], range: range) != nil
    }

    /// Constructs an `EnrollmentToken` with a token string.
    ///
    /// - Parameter rawValue: Token string.
    public init(rawValue: String) {
        self.rawValue = rawValue
    }

    /// Constructs an `EnrollmentToken` directly when pre-validated (e.g. by Rust shared core).
    ///
    /// - Parameter rawValue: Validated token string.
    public init(validatedRawValue rawValue: String) {
        self.rawValue = rawValue
    }
}

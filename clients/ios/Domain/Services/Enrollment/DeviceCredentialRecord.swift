import Foundation

private let opaqueIdRegex = try! NSRegularExpression(
    pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
)

/// Validated domain record containing device credential details issued upon successful enrollment exchange.
public struct DeviceCredentialRecord: Sendable, Equatable {
    public let ownerUserId: String
    public let deviceId: String
    public let credentialId: String
    public let credential: DeviceCredential
    public let createdAt: String
    public let requestId: String?

    /// Initializes and strictly validates a `DeviceCredentialRecord`.
    ///
    /// - Parameters:
    ///   - ownerUserId: Canonical UUID/opaque ID for owner user.
    ///   - deviceId: Canonical UUID/opaque ID for device.
    ///   - credentialId: Canonical UUID/opaque ID for credential record.
    ///   - credential: Validated `DeviceCredential` secret.
    ///   - createdAt: ISO 8601 formatted date-time string.
    ///   - requestId: Optional diagnostic request tracking ID.
    /// - Throws: `DeviceCredentialValidationError` if any identifier or date format is invalid.
    public init(
        ownerUserId: String,
        deviceId: String,
        credentialId: String,
        credential: DeviceCredential,
        createdAt: String,
        requestId: String? = nil
    ) throws {
        guard DeviceCredentialRecord.isValidOpaqueId(ownerUserId) else {
            throw DeviceCredentialValidationError.invalidOwnerUserId(ownerUserId)
        }
        guard DeviceCredentialRecord.isValidOpaqueId(deviceId) else {
            throw DeviceCredentialValidationError.invalidDeviceId(deviceId)
        }
        guard DeviceCredentialRecord.isValidOpaqueId(credentialId) else {
            throw DeviceCredentialValidationError.invalidCredentialId(credentialId)
        }
        guard DeviceCredentialRecord.isValidISO8601(createdAt) else {
            throw DeviceCredentialValidationError.invalidCreatedAt(createdAt)
        }

        self.ownerUserId = ownerUserId
        self.deviceId = deviceId
        self.credentialId = credentialId
        self.credential = credential
        self.createdAt = createdAt
        self.requestId = requestId
    }

    private static func isValidOpaqueId(_ value: String) -> Bool {
        let range = NSRange(location: 0, length: value.utf16.count)
        return opaqueIdRegex.firstMatch(in: value, options: [], range: range) != nil
    }

    private static func isValidISO8601(_ value: String) -> Bool {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if formatter.date(from: value) != nil {
            return true
        }
        formatter.formatOptions = [.withInternetDateTime]
        return formatter.date(from: value) != nil
    }
}

/// Errors raised during `DeviceCredentialRecord` validation.
public enum DeviceCredentialValidationError: Error, Equatable, Sendable {
    case invalidOwnerUserId(String)
    case invalidDeviceId(String)
    case invalidCredentialId(String)
    case invalidCreatedAt(String)
}

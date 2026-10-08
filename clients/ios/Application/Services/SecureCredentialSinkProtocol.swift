import Foundation

/// Receipt proving a credential was persisted and verified by the secure credential store.
///
/// The initializer is module-internal so callers outside the application module can only obtain a
/// receipt from a `SecureCredentialSinkProtocol` implementation. It contains no credential secret.
public struct SecureCredentialPersistenceReceipt: Equatable, Sendable {
    public let canonicalServerEndpoint: String
    public let ownerUserId: String
    public let deviceId: String
    public let credentialId: String

    init(session: DeviceCredentialSession) {
        canonicalServerEndpoint = session.serverEndpoint.urlString
        ownerUserId = session.record.ownerUserId
        deviceId = session.record.deviceId
        credentialId = session.record.credentialId
    }
}

/// Application boundary for the active device credential stored in Keychain.
///
/// The v0.1 contract supports one active device session per app installation. The protocol exposes
/// no Security.framework types, and `load` is a primitive for future restoration; it does not drive
/// startup routing.
public protocol SecureCredentialSinkProtocol: Sendable {
    /// Checks that secure storage can add, read, verify, and delete a temporary probe item.
    func preflight() async throws

    /// Adds a session or replaces the existing session after enforcing its server scope.
    ///
    /// Returns only after a read-back has verified the persisted session.
    func store(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt

    /// Replaces an existing active session without deleting the old item first.
    ///
    /// Returns only after a read-back has verified the replacement.
    func update(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt

    /// Loads and validates the stored session. If an expected endpoint is supplied, an origin
    /// mismatch is reported as a typed scope failure. A missing item throws `itemNotFound`.
    func load(
        expectedServerEndpoint: ServerEndpoint?
    ) async throws -> DeviceCredentialSession

    /// Deletes the active local Keychain item. Deleting a missing item is successful.
    func delete() async throws
}

/// Typed failures raised by the secure credential boundary.
///
/// These cases intentionally carry no credential or Keychain payload data.
public enum SecureCredentialSinkError: Error, Equatable, Sendable {
    case unavailable
    case itemNotFound
    case duplicateItem
    case corruptPayload
    case unsupportedFormat(Int)
    case scopeMismatch
    case invalidCredential
    case writeFailure
    case readFailure
    case verificationFailure
    case deletionFailure
    case unexpectedOSStatus(operation: String, status: Int32)
}

/// Fail-closed test seam retained for enrollment unit tests and previews.
///
/// Production composition injects `KeychainCredentialStore` whenever Rust validation initializes.
struct StubSecureCredentialSink: SecureCredentialSinkProtocol {
    private let isAvailable: Bool

    init(isAvailable: Bool = false) {
        self.isAvailable = isAvailable
    }

    public func preflight() async throws {
        guard isAvailable else {
            throw SecureCredentialSinkError.unavailable
        }
    }

    public func store(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt {
        guard isAvailable else {
            throw SecureCredentialSinkError.writeFailure
        }
        return SecureCredentialPersistenceReceipt(
            session: DeviceCredentialSession(serverEndpoint: serverEndpoint, record: record)
        )
    }

    public func update(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt {
        try await store(record, for: serverEndpoint)
    }

    public func load(
        expectedServerEndpoint: ServerEndpoint?
    ) async throws -> DeviceCredentialSession {
        guard isAvailable else {
            throw SecureCredentialSinkError.unavailable
        }
        throw SecureCredentialSinkError.itemNotFound
    }

    public func delete() async throws {
        guard isAvailable else {
            throw SecureCredentialSinkError.deletionFailure
        }
    }
}

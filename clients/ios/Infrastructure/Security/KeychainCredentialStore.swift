import CoreFoundation
import Foundation
import Security

/// Keychain identity for the single active v0.1 device session.
///
/// Account and service are stable, versioned metadata and never contain credential material.
struct KeychainCredentialItemIdentity: Equatable, Sendable {
    let service: String
    let account: String

    static let production = KeychainCredentialItemIdentity(
        service: "com.synveil.ios.device-session.v1",
        account: "active-device-session.v1"
    )

    static func testOnly(service: String) -> KeychainCredentialItemIdentity {
        KeychainCredentialItemIdentity(service: service, account: "active-device-session.v1")
    }

    func baseQuery(account overrideAccount: String? = nil) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: overrideAccount ?? account,
            kSecAttrSynchronizable as String: false,
        ]
    }

    func addQuery(valueData: Data, account overrideAccount: String? = nil) -> [String: Any] {
        var query = baseQuery(account: overrideAccount)
        query[kSecValueData as String] = valueData
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return query
    }

    func readQuery(account overrideAccount: String? = nil) -> [String: Any] {
        var query = baseQuery(account: overrideAccount)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        return query
    }

    func existenceQuery() -> [String: Any] {
        var query = baseQuery()
        query[kSecReturnAttributes as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        return query
    }

    func updateQuery(account overrideAccount: String? = nil) -> [String: Any] {
        baseQuery(account: overrideAccount)
    }

    func updateAttributes(valueData: Data) -> [String: Any] {
        [kSecValueData as String: valueData]
    }
}

/// Narrow injectable boundary around the synchronous Security.framework Keychain calls.
protocol SecurityKeychainClientProtocol: Sendable {
    func add(_ attributes: [String: Any]) -> OSStatus
    func update(_ query: [String: Any], attributes: [String: Any]) -> OSStatus
    func copyMatching(_ query: [String: Any]) -> KeychainReadResult
    func delete(_ query: [String: Any]) -> OSStatus
}

struct KeychainReadResult: Sendable {
    let status: OSStatus
    let data: Data?
}

struct SystemSecurityKeychainClient: SecurityKeychainClientProtocol {
    func add(_ attributes: [String: Any]) -> OSStatus {
        SecItemAdd(attributes as CFDictionary, nil)
    }

    func update(_ query: [String: Any], attributes: [String: Any]) -> OSStatus {
        SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
    }

    func copyMatching(_ query: [String: Any]) -> KeychainReadResult {
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        return KeychainReadResult(status: status, data: result as? Data)
    }

    func delete(_ query: [String: Any]) -> OSStatus {
        SecItemDelete(query as CFDictionary)
    }
}

private enum KeychainOperation: String {
    case preflightWrite
    case preflightRead
    case preflightDelete
    case add
    case read
    case update
    case delete
    case deleteVerificationRead
}

/// Maps Security.framework statuses once at the infrastructure boundary.
private enum KeychainStatusMapper {
    static func isSuccess(_ status: OSStatus) -> Bool {
        status == errSecSuccess
    }

    static func isItemNotFound(_ status: OSStatus) -> Bool {
        status == errSecItemNotFound
    }

    static func error(
        for status: OSStatus,
        operation: KeychainOperation
    ) -> SecureCredentialSinkError {
        switch status {
        case errSecNotAvailable, errSecInteractionNotAllowed, errSecAuthFailed,
            errSecMissingEntitlement:
            return .unavailable
        case errSecItemNotFound:
            return .itemNotFound
        case errSecDuplicateItem:
            return .duplicateItem
        case errSecIO, errSecDataTooLarge:
            switch operation {
            case .preflightWrite, .add, .update:
                return .writeFailure
            case .preflightRead, .read:
                return .readFailure
            case .deleteVerificationRead:
                return .readFailure
            case .preflightDelete, .delete:
                return .deletionFailure
            }
        default:
            return .unexpectedOSStatus(operation: operation.rawValue, status: Int32(status))
        }
    }
}

/// Actor-isolated, single-session Keychain storage for validated Synveil device credentials.
public actor KeychainCredentialStore: SecureCredentialSinkProtocol {
    static let formatVersion = 1
    static let maximumPayloadBytes = 4_096
    static let maximumEndpointBytes = 2_048

    private static let probeValue = Data([0x53, 0x59, 0x4E, 0x56, 0x45, 0x49, 0x4C])
    private static let probeAccountPrefix = "preflight-probe.v1."

    private let client: any SecurityKeychainClientProtocol
    private let identity: KeychainCredentialItemIdentity
    private let rustBridge: any RustBridgeProtocol
    private var isLifecycleLocked = false
    private var lifecycleWaiters: [CheckedContinuation<Void, Never>] = []

    public init(rustBridge: any RustBridgeProtocol) {
        client = SystemSecurityKeychainClient()
        identity = .production
        self.rustBridge = rustBridge
    }

    init(
        rustBridge: any RustBridgeProtocol,
        client: any SecurityKeychainClientProtocol,
        identity: KeychainCredentialItemIdentity
    ) {
        self.client = client
        self.identity = identity
        self.rustBridge = rustBridge
    }

    public func preflight() async throws {
        await acquireLifecycleLock()
        defer { releaseLifecycleLock() }

        let probeAccount = Self.probeAccountPrefix + UUID().uuidString.lowercased()
        let identityQuery = identity.updateQuery(account: probeAccount)
        var operationError: SecureCredentialSinkError?

        let probeQuery = identity.addQuery(valueData: Self.probeValue, account: probeAccount)
        let writeStatus = client.add(probeQuery)
        if !KeychainStatusMapper.isSuccess(writeStatus) {
            operationError = KeychainStatusMapper.error(
                for: writeStatus,
                operation: .preflightWrite
            )
        } else {
            let readResult = client.copyMatching(identity.readQuery(account: probeAccount))
            if !KeychainStatusMapper.isSuccess(readResult.status) {
                operationError = KeychainStatusMapper.error(
                    for: readResult.status,
                    operation: .preflightRead
                )
            } else if readResult.data != Self.probeValue {
                operationError = .verificationFailure
            }
        }

        let deleteStatus = client.delete(identityQuery)
        if !KeychainStatusMapper.isSuccess(deleteStatus),
            !KeychainStatusMapper.isItemNotFound(deleteStatus), operationError == nil
        {
            operationError = KeychainStatusMapper.error(
                for: deleteStatus,
                operation: .preflightDelete
            )
        }

        if let operationError {
            throw operationError
        }
    }

    public func store(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt {
        await acquireLifecycleLock()
        defer { releaseLifecycleLock() }

        let session = try await validatedSession(record, for: serverEndpoint)
        let payload = try encode(session)
        let existing = try await loadSession(expectedServerEndpoint: nil)

        if let existing {
            guard existing.serverEndpoint == serverEndpoint else {
                throw SecureCredentialSinkError.scopeMismatch
            }
            try writeUpdate(payload)
        } else {
            do {
                try writeAdd(payload)
            } catch SecureCredentialSinkError.duplicateItem {
                // Another store instance may have won the add race. Revalidate its scope, then
                // replace it in place without introducing a delete/add loss window.
                guard let racedSession = try await loadSession(expectedServerEndpoint: nil) else {
                    throw SecureCredentialSinkError.duplicateItem
                }
                guard racedSession.serverEndpoint == serverEndpoint else {
                    throw SecureCredentialSinkError.scopeMismatch
                }
                try writeUpdate(payload)
            }
        }

        try await verifyReadBack(expected: session)
        return SecureCredentialPersistenceReceipt(session: session)
    }

    public func update(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> SecureCredentialPersistenceReceipt {
        await acquireLifecycleLock()
        defer { releaseLifecycleLock() }

        let existing = try await loadSession(expectedServerEndpoint: nil)
        guard let existing else {
            throw SecureCredentialSinkError.itemNotFound
        }
        guard existing.serverEndpoint == serverEndpoint else {
            throw SecureCredentialSinkError.scopeMismatch
        }

        let session = try await validatedSession(record, for: serverEndpoint)
        let payload = try encode(session)
        try writeUpdate(payload)
        try await verifyReadBack(expected: session)
        return SecureCredentialPersistenceReceipt(session: session)
    }

    public func load(
        expectedServerEndpoint: ServerEndpoint?
    ) async throws -> DeviceCredentialSession {
        await acquireLifecycleLock()
        defer { releaseLifecycleLock() }

        guard
            let session = try await loadSession(expectedServerEndpoint: expectedServerEndpoint)
        else {
            throw SecureCredentialSinkError.itemNotFound
        }
        return session
    }

    public func delete() async throws {
        await acquireLifecycleLock()
        defer { releaseLifecycleLock() }

        let status = client.delete(identity.updateQuery())
        guard
            KeychainStatusMapper.isSuccess(status)
                || KeychainStatusMapper.isItemNotFound(status)
        else {
            throw KeychainStatusMapper.error(for: status, operation: .delete)
        }

        guard try activeCredentialIsAbsentWhileLocked() else {
            throw SecureCredentialSinkError.verificationFailure
        }
    }

    public func isActiveCredentialAbsent() async throws -> Bool {
        await acquireLifecycleLock()
        defer { releaseLifecycleLock() }
        return try activeCredentialIsAbsentWhileLocked()
    }

    private func activeCredentialIsAbsentWhileLocked() throws -> Bool {
        let result = client.copyMatching(identity.existenceQuery())
        if KeychainStatusMapper.isItemNotFound(result.status) {
            return true
        }
        guard KeychainStatusMapper.isSuccess(result.status) else {
            throw KeychainStatusMapper.error(
                for: result.status,
                operation: .deleteVerificationRead
            )
        }
        return false
    }

    private func acquireLifecycleLock() async {
        guard isLifecycleLocked else {
            isLifecycleLocked = true
            return
        }

        await withCheckedContinuation { continuation in
            lifecycleWaiters.append(continuation)
        }
    }

    private func releaseLifecycleLock() {
        guard !lifecycleWaiters.isEmpty else {
            isLifecycleLocked = false
            return
        }

        lifecycleWaiters.removeFirst().resume()
    }

    private func validatedSession(
        _ record: DeviceCredentialRecord,
        for serverEndpoint: ServerEndpoint
    ) async throws -> DeviceCredentialSession {
        guard serverEndpoint.urlString.utf8.count <= Self.maximumEndpointBytes else {
            throw SecureCredentialSinkError.scopeMismatch
        }
        guard DeviceCredential.parse(record.credential.rawValue) != nil else {
            throw SecureCredentialSinkError.invalidCredential
        }

        let isValidCredential: Bool
        do {
            isValidCredential = try await rustBridge.validateDeviceBearerToken(
                record.credential.rawValue
            )
        } catch {
            throw SecureCredentialSinkError.invalidCredential
        }
        guard isValidCredential else {
            throw SecureCredentialSinkError.invalidCredential
        }

        let validatedRecord: DeviceCredentialRecord
        do {
            validatedRecord = try DeviceCredentialRecord(
                ownerUserId: record.ownerUserId,
                deviceId: record.deviceId,
                credentialId: record.credentialId,
                credential: DeviceCredential(validatedRawValue: record.credential.rawValue),
                createdAt: record.createdAt
            )
        } catch {
            throw SecureCredentialSinkError.corruptPayload
        }
        return DeviceCredentialSession(serverEndpoint: serverEndpoint, record: validatedRecord)
    }

    private func encode(_ session: DeviceCredentialSession) throws -> Data {
        let envelope = SessionEnvelope(
            formatVersion: Self.formatVersion,
            canonicalServerEndpoint: session.serverEndpoint.urlString,
            ownerUserId: session.record.ownerUserId,
            deviceId: session.record.deviceId,
            credentialId: session.record.credentialId,
            deviceCredential: session.record.credential.rawValue,
            createdAt: session.record.createdAt
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        let payload: Data
        do {
            payload = try encoder.encode(envelope)
        } catch {
            throw SecureCredentialSinkError.writeFailure
        }
        guard payload.count <= Self.maximumPayloadBytes else {
            throw SecureCredentialSinkError.writeFailure
        }
        return payload
    }

    private func loadSession(
        expectedServerEndpoint: ServerEndpoint?
    ) async throws -> DeviceCredentialSession? {
        let result = client.copyMatching(identity.readQuery())
        if KeychainStatusMapper.isItemNotFound(result.status) {
            return nil
        }
        guard KeychainStatusMapper.isSuccess(result.status) else {
            throw KeychainStatusMapper.error(for: result.status, operation: .read)
        }
        guard let data = result.data else {
            throw SecureCredentialSinkError.readFailure
        }
        return try await decode(data, expectedServerEndpoint: expectedServerEndpoint)
    }

    private func decode(
        _ data: Data,
        expectedServerEndpoint: ServerEndpoint?
    ) async throws -> DeviceCredentialSession {
        guard !data.isEmpty, data.count <= Self.maximumPayloadBytes else {
            throw SecureCredentialSinkError.corruptPayload
        }

        let versionProbe: SessionFormatVersion
        do {
            versionProbe = try JSONDecoder().decode(SessionFormatVersion.self, from: data)
        } catch {
            throw SecureCredentialSinkError.corruptPayload
        }
        guard versionProbe.formatVersion == Self.formatVersion else {
            throw SecureCredentialSinkError.unsupportedFormat(versionProbe.formatVersion)
        }

        let envelope: SessionEnvelope
        do {
            envelope = try JSONDecoder().decode(SessionEnvelope.self, from: data)
        } catch {
            throw SecureCredentialSinkError.corruptPayload
        }

        guard envelope.canonicalServerEndpoint.utf8.count <= Self.maximumEndpointBytes else {
            throw SecureCredentialSinkError.corruptPayload
        }

        let serverEndpoint: ServerEndpoint
        do {
            serverEndpoint = try ServerEndpoint(validating: envelope.canonicalServerEndpoint)
        } catch {
            throw SecureCredentialSinkError.corruptPayload
        }
        guard serverEndpoint.urlString == envelope.canonicalServerEndpoint else {
            throw SecureCredentialSinkError.corruptPayload
        }
        if let expectedServerEndpoint, serverEndpoint != expectedServerEndpoint {
            throw SecureCredentialSinkError.scopeMismatch
        }

        guard DeviceCredential.parse(envelope.deviceCredential) != nil else {
            throw SecureCredentialSinkError.invalidCredential
        }

        let isValidCredential: Bool
        do {
            isValidCredential = try await rustBridge.validateDeviceBearerToken(
                envelope.deviceCredential
            )
        } catch {
            throw SecureCredentialSinkError.invalidCredential
        }
        guard isValidCredential else {
            throw SecureCredentialSinkError.invalidCredential
        }

        let credential = DeviceCredential(validatedRawValue: envelope.deviceCredential)
        let record: DeviceCredentialRecord
        do {
            record = try DeviceCredentialRecord(
                ownerUserId: envelope.ownerUserId,
                deviceId: envelope.deviceId,
                credentialId: envelope.credentialId,
                credential: credential,
                createdAt: envelope.createdAt,
                requestId: nil
            )
        } catch {
            throw SecureCredentialSinkError.corruptPayload
        }

        return DeviceCredentialSession(serverEndpoint: serverEndpoint, record: record)
    }

    private func writeAdd(_ payload: Data) throws {
        let status = client.add(identity.addQuery(valueData: payload))
        guard KeychainStatusMapper.isSuccess(status) else {
            throw KeychainStatusMapper.error(for: status, operation: .add)
        }
    }

    private func writeUpdate(_ payload: Data) throws {
        let query = identity.updateQuery()
        let attributes = identity.updateAttributes(valueData: payload)
        let status = client.update(query, attributes: attributes)
        guard KeychainStatusMapper.isSuccess(status) else {
            throw KeychainStatusMapper.error(for: status, operation: .update)
        }
    }

    private func verifyReadBack(expected: DeviceCredentialSession) async throws {
        guard
            let actual = try await loadSession(expectedServerEndpoint: expected.serverEndpoint)
        else {
            throw SecureCredentialSinkError.verificationFailure
        }
        guard
            actual.serverEndpoint == expected.serverEndpoint,
            actual.record.ownerUserId == expected.record.ownerUserId,
            actual.record.deviceId == expected.record.deviceId,
            actual.record.credentialId == expected.record.credentialId,
            actual.record.credential.rawValue == expected.record.credential.rawValue,
            actual.record.createdAt == expected.record.createdAt,
            actual.record.requestId == nil
        else {
            throw SecureCredentialSinkError.verificationFailure
        }
    }

    private struct SessionFormatVersion: Decodable {
        let formatVersion: Int
    }

    private struct SessionEnvelope: Codable {
        let formatVersion: Int
        let canonicalServerEndpoint: String
        let ownerUserId: String
        let deviceId: String
        let credentialId: String
        let deviceCredential: String
        let createdAt: String
    }
}

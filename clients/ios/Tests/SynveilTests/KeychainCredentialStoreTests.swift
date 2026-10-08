import Foundation
import Security
import XCTest

@testable import Synveil

@MainActor
final class KeychainCredentialStoreTests: XCTestCase {
    private let endpoint = try! ServerEndpoint(validating: "https://example.synveil.com")
    private let otherEndpoint = try! ServerEndpoint(validating: "https://other.synveil.com")

    private let credentialA =
        "svd1_abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
    private let credentialB =
        "svd1_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

    private func makeRecord(
        ownerUserId: String = "11111111-2222-3333-4444-555555555555",
        deviceId: String = "66666666-7777-8888-9999-000000000000",
        credentialId: String = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        credential: String? = nil,
        createdAt: String = "2026-10-07T12:00:00Z",
        requestId: String? = nil
    ) throws -> DeviceCredentialRecord {
        try DeviceCredentialRecord(
            ownerUserId: ownerUserId,
            deviceId: deviceId,
            credentialId: credentialId,
            credential: DeviceCredential(validatedRawValue: credential ?? credentialA),
            createdAt: createdAt,
            requestId: requestId
        )
    }

    private func makeStore(
        client: FakeSecurityKeychainClient,
        identity: KeychainCredentialItemIdentity = .production,
        rustBridge: KeychainTestRustBridge? = nil
    ) -> KeychainCredentialStore {
        KeychainCredentialStore(
            rustBridge: rustBridge ?? KeychainTestRustBridge(),
            client: client,
            identity: identity
        )
    }

    func testItemIdentityIsStableVersionedAndContainsNoCredential() {
        let identity = KeychainCredentialItemIdentity.production

        XCTAssertEqual(identity.service, "com.synveil.ios.device-session.v1")
        XCTAssertEqual(identity.account, "active-device-session.v1")
        XCTAssertFalse(identity.service.contains(credentialA))
        XCTAssertFalse(identity.account.contains(credentialA))
    }

    func testAddQueryUsesGenericPasswordAndRequiredDeviceLocalAttributes() {
        let query = KeychainCredentialItemIdentity.production.addQuery(valueData: Data([1, 2, 3]))

        XCTAssertEqual(query[kSecClass as String] as? String, kSecClassGenericPassword as String)
        XCTAssertEqual(
            query[kSecAttrService as String] as? String,
            "com.synveil.ios.device-session.v1"
        )
        XCTAssertEqual(query[kSecAttrAccount as String] as? String, "active-device-session.v1")
        XCTAssertEqual(
            query[kSecAttrAccessible as String] as? String,
            kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String
        )
        XCTAssertEqual(query[kSecAttrSynchronizable as String] as? Bool, false)
        XCTAssertNil(query[kSecAttrAccessGroup as String])
    }

    func testPreflightWritesReadsVerifiesAndDeletesIsolatedProbe() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)

        try await store.preflight()

        XCTAssertEqual(client.operations, ["add", "copy", "delete"])
        XCTAssertEqual(client.addedQueries.count, 1)
        let probeQuery = try XCTUnwrap(client.addedQueries.first)
        let probeAccount = try XCTUnwrap(probeQuery[kSecAttrAccount as String] as? String)
        XCTAssertTrue(probeAccount.hasPrefix("preflight-probe.v1."))
        XCTAssertNotEqual(probeAccount, KeychainCredentialItemIdentity.production.account)
        XCTAssertNil(client.data(for: KeychainCredentialItemIdentity.production))
        XCTAssertNil(
            client.data(
                service: KeychainCredentialItemIdentity.production.service,
                account: probeAccount
            )
        )
    }

    func testPreflightWriteFailureStillAttemptsCleanupAndDoesNotTouchProductionItem() async {
        let client = FakeSecurityKeychainClient()
        client.addStatusOverride = errSecIO
        let store = makeStore(client: client)

        do {
            try await store.preflight()
            XCTFail("Expected Keychain preflight failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .writeFailure)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }

        XCTAssertEqual(client.operations, ["add", "delete"])
        XCTAssertNotEqual(
            client.addedQueries.first?[kSecAttrAccount as String] as? String,
            KeychainCredentialItemIdentity.production.account
        )
        XCTAssertNil(client.data(for: KeychainCredentialItemIdentity.production))
    }

    func testPreflightReadFailureIsTypedAndCleansProbe() async {
        let client = FakeSecurityKeychainClient()
        client.copyStatusOverride = errSecIO
        let store = makeStore(client: client)

        do {
            try await store.preflight()
            XCTFail("Expected Keychain preflight read failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .readFailure)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }

        XCTAssertEqual(client.operations, ["add", "copy", "delete"])
        let probeAccount = client.addedQueries.first?[kSecAttrAccount as String] as? String
        XCTAssertNotEqual(probeAccount, KeychainCredentialItemIdentity.production.account)
        if let probeAccount {
            XCTAssertNil(
                client.data(
                    service: KeychainCredentialItemIdentity.production.service,
                    account: probeAccount
                )
            )
        }
    }

    func testPreflightReadMismatchFailsAndCleansProbe() async {
        let client = FakeSecurityKeychainClient()
        client.readbackOverride = Data([0x00])
        let store = makeStore(client: client)

        do {
            try await store.preflight()
            XCTFail("Expected verification failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .verificationFailure)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }

        XCTAssertEqual(client.operations, ["add", "copy", "delete"])
        let account = client.addedQueries.first?[kSecAttrAccount as String] as? String
        XCTAssertNotNil(account)
        if let account {
            XCTAssertNil(
                client.data(
                    service: KeychainCredentialItemIdentity.production.service,
                    account: account
                )
            )
        }
    }

    func testUnavailableAndFailedProbeCleanupStatusesAreTyped() async {
        let unavailableClient = FakeSecurityKeychainClient()
        unavailableClient.addStatusOverride = errSecNotAvailable
        let unavailableStore = makeStore(client: unavailableClient)
        do {
            try await unavailableStore.preflight()
            XCTFail("Expected unavailable status")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .unavailable)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }

        let missingEntitlementClient = FakeSecurityKeychainClient()
        missingEntitlementClient.addStatusOverride = errSecMissingEntitlement
        let missingEntitlementStore = makeStore(client: missingEntitlementClient)
        do {
            try await missingEntitlementStore.preflight()
            XCTFail("Expected missing entitlement to make Keychain unavailable")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .unavailable)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
        XCTAssertEqual(missingEntitlementClient.operations, ["add", "delete"])

        let cleanupClient = FakeSecurityKeychainClient()
        cleanupClient.deleteStatusOverride = errSecIO
        let cleanupStore = makeStore(client: cleanupClient)
        do {
            try await cleanupStore.preflight()
            XCTFail("Expected cleanup failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .deletionFailure)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
        XCTAssertEqual(cleanupClient.operations, ["add", "copy", "delete"])
    }

    func testInitialStoreUsesAddAndReadBackVerification() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)

        let receipt = try await store.store(try makeRecord(), for: endpoint)

        XCTAssertEqual(receipt.canonicalServerEndpoint, endpoint.urlString)
        XCTAssertEqual(client.operations, ["copy", "add", "copy"])
        XCTAssertEqual(client.addedQueries.count, 1)
        XCTAssertTrue(client.updatedQueries.isEmpty)
    }

    func testStoreReplacementUsesUpdateWithoutDeleteFirst() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)
        client.resetOperations()

        _ = try await store.store(try makeRecord(credential: credentialB), for: endpoint)

        XCTAssertEqual(client.operations, ["copy", "update", "copy"])
        XCTAssertEqual(client.updatedQueries.count, 1)
        XCTAssertFalse(client.operations.contains("delete"))
    }

    func testConcurrentLifecycleCallsSerializeInitialStoreAndReplacement() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        let firstRecord = try makeRecord()
        let secondRecord = try makeRecord(credential: credentialB)
        let storeEndpoint = endpoint

        async let firstReceipt = store.store(firstRecord, for: storeEndpoint)
        async let secondReceipt = store.store(secondRecord, for: storeEndpoint)
        let receipts = try await (firstReceipt, secondReceipt)
        let loaded = try await store.load(expectedServerEndpoint: endpoint)

        XCTAssertEqual(receipts.0.canonicalServerEndpoint, endpoint.urlString)
        XCTAssertEqual(receipts.1.canonicalServerEndpoint, endpoint.urlString)
        XCTAssertEqual(client.addedQueries.count, 1)
        XCTAssertEqual(client.updatedQueries.count, 1)
        XCTAssertTrue(
            loaded.record.credential.rawValue == credentialA
                || loaded.record.credential.rawValue == credentialB
        )
    }

    func testExplicitUpdateUsesSecItemUpdateAndVerifiesNewRecord() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)
        client.resetOperations()

        _ = try await store.update(try makeRecord(credential: credentialB), for: endpoint)
        let loaded = try await store.load(expectedServerEndpoint: endpoint)

        XCTAssertEqual(client.operations, ["copy", "update", "copy", "copy"])
        XCTAssertEqual(loaded.record.credential.rawValue, credentialB)
        XCTAssertFalse(client.operations.contains("delete"))
    }

    func testReadBackMismatchFailsVerification() async {
        let client = FakeSecurityKeychainClient()
        client.readbackOverrideAfterWrite = payload(credential: credentialB)
        let store = makeStore(client: client)

        do {
            _ = try await store.store(try makeRecord(), for: endpoint)
            XCTFail("Expected verification failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .verificationFailure)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testLoadReturnsTypedNotFoundWhenItemIsMissing() async {
        let store = makeStore(client: FakeSecurityKeychainClient())

        do {
            _ = try await store.load(expectedServerEndpoint: endpoint)
            XCTFail("Expected explicit item-not-found error")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .itemNotFound)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testValidStoredSessionLoadsAndOmitsTransientRequestId() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        let record = try makeRecord(requestId: "request-123456")
        _ = try await store.store(record, for: endpoint)

        let loaded = try await store.load(expectedServerEndpoint: endpoint)

        XCTAssertEqual(loaded.serverEndpoint, endpoint)
        XCTAssertEqual(loaded.record.ownerUserId, record.ownerUserId)
        XCTAssertEqual(loaded.record.deviceId, record.deviceId)
        XCTAssertEqual(loaded.record.credentialId, record.credentialId)
        XCTAssertEqual(loaded.record.credential.rawValue, credentialA)
        XCTAssertEqual(loaded.record.createdAt, record.createdAt)
        XCTAssertNil(loaded.record.requestId)
        let storedData = try XCTUnwrap(client.data(for: KeychainCredentialItemIdentity.production))
        let storedJSON = String(decoding: storedData, as: UTF8.self)
        XCTAssertFalse(storedJSON.contains("requestId"))
        XCTAssertFalse(storedJSON.contains("request-123456"))
    }

    func testCorruptStoredDataFailsClosed() async {
        await assertLoadFailure(Data([0xFF, 0x00]), expected: .corruptPayload)
    }

    func testOversizedStoredDataFailsClosed() async {
        await assertLoadFailure(
            Data(repeating: 0x41, count: KeychainCredentialStore.maximumPayloadBytes + 1),
            expected: .corruptPayload
        )
    }

    func testUnknownFormatVersionIsUnsupported() async {
        await assertLoadFailure(payload(formatVersion: 2), expected: .unsupportedFormat(2))
    }

    func testMalformedStoredCredentialIsRejected() async {
        await assertLoadFailure(
            payload(credential: "svd1_NOT_A_VALID_CREDENTIAL"),
            expected: .invalidCredential
        )
    }

    func testRustBridgeRejectsWellFormedStoredCredential() async {
        let client = FakeSecurityKeychainClient()
        client.seed(payload(), identity: .production)
        let store = makeStore(
            client: client,
            rustBridge: KeychainTestRustBridge(rejectDeviceCredential: true)
        )

        do {
            _ = try await store.load(expectedServerEndpoint: endpoint)
            XCTFail("Expected Rust validation failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .invalidCredential)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testMalformedOwnerUserIdIsRejected() async {
        await assertLoadFailure(payload(ownerUserId: "owner"), expected: .corruptPayload)
    }

    func testMalformedDeviceIdIsRejected() async {
        await assertLoadFailure(payload(deviceId: "device"), expected: .corruptPayload)
    }

    func testMalformedCredentialIdIsRejected() async {
        await assertLoadFailure(payload(credentialId: "credential"), expected: .corruptPayload)
    }

    func testInvalidTimestampIsRejected() async {
        await assertLoadFailure(payload(createdAt: "not-a-time"), expected: .corruptPayload)
    }

    func testOriginMismatchIsRejected() async {
        await assertLoadFailure(payload(), endpoint: otherEndpoint, expected: .scopeMismatch)
    }

    func testStoreCannotMoveExistingCredentialToAnotherOrigin() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)

        do {
            _ = try await store.store(try makeRecord(credential: credentialB), for: otherEndpoint)
            XCTFail("Expected server scope mismatch")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .scopeMismatch)
        }
    }

    func testDeleteSucceedsAndIsIdempotent() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)

        try await store.delete()
        try await store.delete()

        XCTAssertNil(client.data(for: KeychainCredentialItemIdentity.production))
    }

    func testDeleteVerifiesAbsenceWithoutRequestingCredentialData() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)
        client.resetOperations()

        try await store.delete()

        XCTAssertEqual(client.operations, ["delete", "copy"])
        let verificationQuery = try XCTUnwrap(client.copiedQueries.last)
        XCTAssertEqual(verificationQuery[kSecReturnAttributes as String] as? Bool, true)
        XCTAssertNil(verificationQuery[kSecReturnData as String])
        XCTAssertNil(client.data(for: KeychainCredentialItemIdentity.production))
    }

    func testDeleteStatusSuccessWithRemainingItemFailsVerification() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)
        client.deleteStatusOverride = errSecSuccess

        do {
            try await store.delete()
            XCTFail("A successful delete status is not sufficient when the item remains.")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .verificationFailure)
        }

        XCTAssertNotNil(client.data(for: KeychainCredentialItemIdentity.production))
    }

    func testDeleteFailureIsTyped() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)
        client.deleteStatusOverride = errSecIO

        do {
            try await store.delete()
            XCTFail("Expected delete failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .deletionFailure)
        }
    }

    func testAddDuplicateMapsToTypedDuplicateFailure() async {
        let client = FakeSecurityKeychainClient()
        client.addStatusOverride = errSecDuplicateItem
        let store = makeStore(client: client)

        do {
            _ = try await store.store(try makeRecord(), for: endpoint)
            XCTFail("Expected duplicate item failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .duplicateItem)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testReadAndUpdateFailuresMapToTypedErrors() async throws {
        let readClient = FakeSecurityKeychainClient()
        readClient.copyStatusOverride = errSecIO
        let readStore = makeStore(client: readClient)
        do {
            _ = try await readStore.load(expectedServerEndpoint: endpoint)
            XCTFail("Expected read failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .readFailure)
        }

        let updateClient = FakeSecurityKeychainClient()
        let updateStore = makeStore(client: updateClient)
        _ = try await updateStore.store(try makeRecord(), for: endpoint)
        updateClient.updateStatusOverride = errSecIO
        do {
            _ = try await updateStore.update(try makeRecord(credential: credentialB), for: endpoint)
            XCTFail("Expected update failure")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, .writeFailure)
        }
    }

    func testUnknownOSStatusIsReportedWithoutPayloadData() async {
        let client = FakeSecurityKeychainClient()
        client.copyStatusOverride = errSecParam
        let store = makeStore(client: client)

        do {
            _ = try await store.load(expectedServerEndpoint: endpoint)
            XCTFail("Expected unknown OSStatus")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(
                error,
                .unexpectedOSStatus(operation: "read", status: Int32(errSecParam))
            )
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testCredentialNeverAppearsInServiceOrAccountMetadata() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(), for: endpoint)

        let query = try XCTUnwrap(client.addedQueries.first)
        let service = try XCTUnwrap(query[kSecAttrService as String] as? String)
        let account = try XCTUnwrap(query[kSecAttrAccount as String] as? String)
        XCTAssertFalse(service.contains(credentialA))
        XCTAssertFalse(account.contains(credentialA))
        XCTAssertFalse(service.contains("svd1_"))
        XCTAssertFalse(account.contains("svd1_"))
    }

    func testStoredEnvelopeHasOnlyVersionedSessionAuthorityFields() async throws {
        let client = FakeSecurityKeychainClient()
        let store = makeStore(client: client)
        _ = try await store.store(try makeRecord(requestId: "diag-123456"), for: endpoint)
        let data = try XCTUnwrap(client.data(for: KeychainCredentialItemIdentity.production))
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])

        XCTAssertEqual(object["formatVersion"] as? Int, 1)
        XCTAssertEqual(object["canonicalServerEndpoint"] as? String, endpoint.urlString)
        XCTAssertNotNil(object["ownerUserId"])
        XCTAssertNotNil(object["deviceId"])
        XCTAssertNotNil(object["credentialId"])
        XCTAssertNotNil(object["deviceCredential"])
        XCTAssertNotNil(object["createdAt"])
        XCTAssertNil(object["requestId"])
        XCTAssertLessThanOrEqual(data.count, KeychainCredentialStore.maximumPayloadBytes)
    }

    #if targetEnvironment(simulator)
        func testSimulatorKeychainRoundTripUsesUniqueTestService() async throws {
            let identity = KeychainCredentialItemIdentity.testOnly(
                service: "com.synveil.ios.tests.keychain.\(UUID().uuidString.lowercased())"
            )
            let store = KeychainCredentialStore(
                rustBridge: KeychainTestRustBridge(),
                client: SystemSecurityKeychainClient(),
                identity: identity
            )

            do {
                try await store.preflight()
            } catch SecureCredentialSinkError.unavailable {
                throw XCTSkip(
                    "The unsigned Simulator test process has no Keychain access entitlement."
                )
            }

            try await store.delete()
            do {
                _ = try await store.store(try makeRecord(), for: endpoint)
                let loaded = try await store.load(expectedServerEndpoint: endpoint)
                XCTAssertEqual(loaded.record.credential.rawValue, credentialA)
                try await store.delete()
            } catch {
                try? await store.delete()
                throw error
            }
        }
    #endif

    private func assertLoadFailure(
        _ data: Data,
        endpoint expectedEndpoint: ServerEndpoint? = nil,
        expected: SecureCredentialSinkError
    ) async {
        let client = FakeSecurityKeychainClient()
        client.seed(data, identity: .production)
        let store = makeStore(client: client)

        do {
            _ = try await store.load(expectedServerEndpoint: expectedEndpoint)
            XCTFail("Expected stored session to be rejected")
        } catch let error as SecureCredentialSinkError {
            XCTAssertEqual(error, expected)
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    private func payload(
        formatVersion: Int = 1,
        endpoint: String? = nil,
        ownerUserId: String = "11111111-2222-3333-4444-555555555555",
        deviceId: String = "66666666-7777-8888-9999-000000000000",
        credentialId: String = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        credential: String? = nil,
        createdAt: String = "2026-10-07T12:00:00Z"
    ) -> Data {
        let object: [String: Any] = [
            "formatVersion": formatVersion,
            "canonicalServerEndpoint": endpoint ?? self.endpoint.urlString,
            "ownerUserId": ownerUserId,
            "deviceId": deviceId,
            "credentialId": credentialId,
            "deviceCredential": credential ?? credentialA,
            "createdAt": createdAt,
        ]
        return try! JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    }
}

private actor KeychainTestRustBridge: RustBridgeProtocol {
    private let rejectDeviceCredential: Bool

    init(rejectDeviceCredential: Bool = false) {
        self.rejectDeviceCredential = rejectDeviceCredential
    }

    func parseSHA256(_ canonical: String) async throws -> Data { Data() }
    func formatSHA256(_ digest: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ token: String) async throws -> Bool {
        EnrollmentToken.isValid(token)
    }
    func validateDeviceBearerToken(_ token: String) async throws -> Bool {
        !rejectDeviceCredential && DeviceCredential.isValid(token)
    }
    func validateLibraryID(_ value: String) async throws -> Bool { false }
    func validateNodeID(_ value: String) async throws -> Bool { false }
    func validateLogicalName(_ value: String) async throws -> Bool { !value.isEmpty }
}

private final class FakeSecurityKeychainClient:
    SecurityKeychainClientProtocol, @unchecked Sendable
{
    private let lock = NSLock()
    private var items: [String: Data] = [:]
    private var operationLog: [String] = []
    private var addLog: [[String: Any]] = []
    private var updateLog: [[String: Any]] = []
    private var copyLog: [[String: Any]] = []
    private var writes = 0

    var addStatusOverride: OSStatus?
    var updateStatusOverride: OSStatus?
    var copyStatusOverride: OSStatus?
    var deleteStatusOverride: OSStatus?
    var readbackOverride: Data?
    var readbackOverrideAfterWrite: Data?

    var operations: [String] {
        lock.lock()
        defer { lock.unlock() }
        return operationLog
    }

    var addedQueries: [[String: Any]] {
        lock.lock()
        defer { lock.unlock() }
        return addLog
    }

    var updatedQueries: [[String: Any]] {
        lock.lock()
        defer { lock.unlock() }
        return updateLog
    }

    var copiedQueries: [[String: Any]] {
        lock.lock()
        defer { lock.unlock() }
        return copyLog
    }

    func resetOperations() {
        lock.lock()
        defer { lock.unlock() }
        operationLog.removeAll()
        addLog.removeAll()
        updateLog.removeAll()
        copyLog.removeAll()
    }

    func add(_ attributes: [String: Any]) -> OSStatus {
        lock.lock()
        defer { lock.unlock() }
        operationLog.append("add")
        addLog.append(attributes)
        if let addStatusOverride { return addStatusOverride }
        guard
            let key = key(from: attributes),
            let data = attributes[kSecValueData as String] as? Data
        else {
            return errSecParam
        }
        guard items[key] == nil else { return errSecDuplicateItem }
        items[key] = data
        writes += 1
        return errSecSuccess
    }

    func update(_ query: [String: Any], attributes: [String: Any]) -> OSStatus {
        lock.lock()
        defer { lock.unlock() }
        operationLog.append("update")
        updateLog.append(query)
        if let updateStatusOverride { return updateStatusOverride }
        guard
            let key = key(from: query),
            items[key] != nil,
            let data = attributes[kSecValueData as String] as? Data
        else {
            return errSecItemNotFound
        }
        items[key] = data
        writes += 1
        return errSecSuccess
    }

    func copyMatching(_ query: [String: Any]) -> KeychainReadResult {
        lock.lock()
        defer { lock.unlock() }
        operationLog.append("copy")
        copyLog.append(query)
        if let copyStatusOverride {
            return KeychainReadResult(status: copyStatusOverride, data: nil)
        }
        guard let key = key(from: query) else {
            return KeychainReadResult(status: errSecParam, data: nil)
        }
        if let readbackOverrideAfterWrite, writes > 0 {
            return KeychainReadResult(
                status: errSecSuccess,
                data: (query[kSecReturnData as String] as? Bool == true)
                    ? readbackOverrideAfterWrite
                    : nil
            )
        }
        if let readbackOverride {
            return KeychainReadResult(
                status: errSecSuccess,
                data: (query[kSecReturnData as String] as? Bool == true)
                    ? readbackOverride
                    : nil
            )
        }
        guard let data = items[key] else {
            return KeychainReadResult(status: errSecItemNotFound, data: nil)
        }
        return KeychainReadResult(
            status: errSecSuccess,
            data: (query[kSecReturnData as String] as? Bool == true) ? data : nil
        )
    }

    func delete(_ query: [String: Any]) -> OSStatus {
        lock.lock()
        defer { lock.unlock() }
        operationLog.append("delete")
        if let deleteStatusOverride { return deleteStatusOverride }
        guard let key = key(from: query) else { return errSecParam }
        guard items.removeValue(forKey: key) != nil else { return errSecItemNotFound }
        return errSecSuccess
    }

    func seed(_ data: Data, identity: KeychainCredentialItemIdentity) {
        lock.lock()
        defer { lock.unlock() }
        items[identityKey(service: identity.service, account: identity.account)] = data
    }

    func data(for identity: KeychainCredentialItemIdentity) -> Data? {
        data(service: identity.service, account: identity.account)
    }

    func data(service: String, account: String?) -> Data? {
        guard let account else { return nil }
        lock.lock()
        defer { lock.unlock() }
        return items[identityKey(service: service, account: account)]
    }

    private func key(from query: [String: Any]) -> String? {
        guard
            let service = query[kSecAttrService as String] as? String,
            let account = query[kSecAttrAccount as String] as? String
        else {
            return nil
        }
        return identityKey(service: service, account: account)
    }

    private func identityKey(service: String, account: String) -> String {
        "\(service)|\(account)"
    }
}

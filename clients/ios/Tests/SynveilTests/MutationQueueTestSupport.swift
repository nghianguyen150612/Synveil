import Foundation
import SQLite3
import XCTest

@testable import Synveil

func queueUUID(_ number: Int) -> String { String(format: "018f0010-abcd-7000-8000-%012x", number) }
let queueBearer = "svd1_" + String(repeating: "a", count: 64)

struct QueueValidator: RustBridgeProtocol {
    func parseSHA256(_ value: String) async throws -> Data { Data() }
    func formatSHA256(_ value: Data) async throws -> String { "" }
    func validateEnrollmentToken(_ value: String) async throws -> Bool { false }
    func validateDeviceBearerToken(_ value: String) async throws -> Bool {
        DeviceCredential.isValid(value)
    }
    func validateLibraryID(_ value: String) async throws -> Bool {
        LibraryWireValidation.matches(
            value, pattern: "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
    }
    func validateNodeID(_ value: String) async throws -> Bool { try await validateLibraryID(value) }
    func validateLogicalName(_ value: String) async throws -> Bool {
        !value.isEmpty && value.utf8.count <= 1024
    }
}

func queueScope(
    device: Int = 91, library: Int = 80, owner: Int = 90,
    endpoint: String = "https://queue.example:8443/synveil"
) async throws -> ClientMutationScope {
    ClientMutationScope(
        serverEndpoint: try ServerEndpoint(validating: endpoint), ownerUserId: queueUUID(owner),
        deviceId: try await ClientMutationDeviceId.validated(
            queueUUID(device), using: QueueValidator()),
        libraryId: try await LibraryId.validated(queueUUID(library), using: QueueValidator()))
}

func queuePrepared(
    id: Int = 10, node: Int = 1, name: String = "Private logical/A 🚀", epoch: String = "1",
    sequence: String = "0", scope: ClientMutationScope? = nil, intent: ClientMutationIntent? = nil,
    bridge: any RustBridgeProtocol = QueueValidator()
) async throws -> PreparedClientMutation {
    let resolvedScope: ClientMutationScope
    if let scope { resolvedScope = scope } else { resolvedScope = try await queueScope() }
    let scope = resolvedScope
    let base = try ClientMutationBase(
        scope: scope, epoch: ClientMutationDecimal(validating: epoch),
        sequence: ClientMutationDecimal(validating: sequence))
    let node = try await NodeId.validated(queueUUID(node), using: bridge)
    return try await PreparedClientMutation(
        id: ClientMutationId.validated(queueUUID(id), using: bridge), base: base,
        intent: intent
            ?? .renameNode(
                nodeId: node, expectedRevision: NodeRevision(validating: "3"), newName: name),
        bridge: bridge)
}

func queueCheckpointObject(
    scope: ClientMutationScope, epoch: String = "1", sequence: String = "0",
    watermark: String? = nil
) -> [String: Any] {
    var data: [String: Any] = [
        "device_id": scope.deviceId.rawValue, "library_id": scope.libraryId.rawValue,
        "epoch": epoch, "acknowledged_sequence": sequence, "created_at": "2026-10-09T12:00:00Z",
        "updated_at": "2026-10-09T12:00:01Z",
    ]
    data["last_seen_high_watermark"] = watermark
    return ["data": data, "meta": ["request_id": "checkpoint-request-01"]]
}

func queueHTTP(
    _ object: [String: Any], status: Int = 200,
    headers: [String: String] = ["Content-Type": "application/json"]
) throws -> HTTPTransportResponse {
    HTTPTransportResponse(
        statusCode: status, headers: headers,
        body: try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]))
}

func queueCheckpoint(scope: ClientMutationScope, epoch: String = "1", sequence: String = "0")
    async throws -> SyncCheckpoint
{
    try await SyncCheckpointResponseDecoder(bridge: QueueValidator()).decode(
        queueHTTP(queueCheckpointObject(scope: scope, epoch: epoch, sequence: sequence)),
        scope: scope)
}

func queueApplied(_ mutation: PreparedClientMutation) async throws -> ClientMutationSubmissionResult
{
    let object: [String: Any] = [
        "data": [
            "outcome": "APPLIED", "mutation_id": mutation.id.rawValue,
            "kind": mutation.kind.rawValue, "replayed": true, "journal_event_id": queueUUID(12),
            "journal_sequence": "9007199254740993",
            "node": [
                "id": queueUUID(1), "library_id": mutation.base.scope.libraryId.rawValue,
                "parent_node_id": queueUUID(2),
                "kind": "FILE", "state": "ACTIVE", "name": "Authoritative name", "revision": "4",
                "created_at": "2026-10-09T12:00:00Z", "updated_at": "2026-10-09T12:00:01Z",
            ],
        ],
        "meta": ["request_id": "mutation-request-01"],
    ]
    return try await ClientMutationResponseDecoder(bridge: QueueValidator()).decode(
        queueHTTP(object), for: mutation)
}

func queueConflict(_ mutation: PreparedClientMutation, details: Bool = true) async throws
    -> ClientMutationSubmissionResult
{
    var error: [String: Any] = [
        "code": "mutation_conflict", "message": "Do not persist raw diagnostics",
        "retryable": false,
        "request_id": "mutation-request-01",
    ]
    if details {
        error["details"] = [
            "outcome": "CONFLICT", "replayed": true, "conflict_id": queueUUID(13),
            "reason": "REVISION_MISMATCH", "resource_id": queueUUID(1), "expected_revision": "3",
            "current_revision": "4",
            "current_state": "ACTIVE", "current_parent_id": queueUUID(2),
            "current_name": "Current private name",
            "server_epoch": "1", "server_sequence": "9007199254740993",
        ]
    }
    return try await ClientMutationResponseDecoder(bridge: QueueValidator()).decode(
        queueHTTP(["error": error], status: 409), for: mutation)
}

@MainActor
func queueTemporaryURL(_ test: XCTestCase) -> URL {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
        "SynveilQueueTests-" + UUID().uuidString)
    test.addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
    return directory.appendingPathComponent("mutations.sqlite")
}

/// Parameterized mutation helper and scalar inspection use actual file-backed SQLite.
func queueRawSQL(_ url: URL, _ sql: String, bindings: [Data] = []) throws {
    var db: OpaquePointer?
    guard sqlite3_open(url.path, &db) == SQLITE_OK, let db else {
        throw MutationQueueFailure.databaseOpen
    }
    defer { sqlite3_close(db) }
    var statement: OpaquePointer?
    guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
        throw MutationQueueFailure.corrupt
    }
    defer { sqlite3_finalize(statement) }
    for (index, blob) in bindings.enumerated() {
        let code = blob.withUnsafeBytes {
            sqlite3_bind_blob(
                statement, Int32(index + 1), $0.baseAddress, Int32($0.count),
                unsafeBitCast(-1, to: sqlite3_destructor_type.self))
        }
        guard code == SQLITE_OK else { throw MutationQueueFailure.io }
    }
    var result = sqlite3_step(statement)
    while result == SQLITE_ROW { result = sqlite3_step(statement) }
    guard result == SQLITE_DONE else { throw MutationQueueFailure.corrupt }
}

func queueRawScalar(_ url: URL, _ sql: String) throws -> String {
    var db: OpaquePointer?
    guard sqlite3_open(url.path, &db) == SQLITE_OK, let db else {
        throw MutationQueueFailure.databaseOpen
    }
    defer { sqlite3_close(db) }
    var statement: OpaquePointer?
    guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else {
        throw MutationQueueFailure.corrupt
    }
    defer { sqlite3_finalize(statement) }
    guard sqlite3_step(statement) == SQLITE_ROW, let bytes = sqlite3_column_text(statement, 0)
    else { throw MutationQueueFailure.corrupt }
    return String(cString: bytes)
}

final class QueueFaultInjector: @unchecked Sendable {
    private let lock = NSLock()
    private var point: MutationQueueFaultPoint?
    private var rollbackFailure = false
    private var failure: MutationQueueFailure = .diskFull
    private var action: (@Sendable () -> Void)?
    func arm(
        _ point: MutationQueueFaultPoint, failure: MutationQueueFailure = .diskFull,
        action: (@Sendable () -> Void)? = nil
    ) {
        lock.lock()
        defer { lock.unlock() }
        self.point = point
        self.failure = failure
        self.action = action
    }
    func armRollbackFailure() {
        lock.lock()
        defer { lock.unlock() }
        rollbackFailure = true
    }
    func hit(_ point: MutationQueueFaultPoint) throws {
        lock.lock()
        if point == .beforeRollback && rollbackFailure {
            rollbackFailure = false
            lock.unlock()
            throw MutationQueueFailure.io
        }
        guard self.point == point else {
            lock.unlock()
            return
        }
        self.point = nil
        let failure = failure
        let action = action
        lock.unlock()
        action?()
        throw failure
    }
}

actor QueueGate {
    private var reached = false
    private var observers: [CheckedContinuation<Void, Never>] = []
    private var suspended: CheckedContinuation<Void, Never>?
    func arrive() async {
        reached = true
        observers.forEach { $0.resume() }
        observers.removeAll()
        await withCheckedContinuation { suspended = $0 }
    }
    func wait() async { if !reached { await withCheckedContinuation { observers.append($0) } } }
    func release() {
        suspended?.resume()
        suspended = nil
    }
}

actor QueueCredentialStore: SecureCredentialSinkProtocol {
    private var session: DeviceCredentialSession
    private var gate: QueueGate?
    private var deleted = false
    private var replacementAfterLoad: DeviceCredentialSession?
    init(_ session: DeviceCredentialSession) { self.session = session }
    func preflight() async throws {}
    func store(_ record: DeviceCredentialRecord, for endpoint: ServerEndpoint) async throws
        -> SecureCredentialPersistenceReceipt
    {
        session = DeviceCredentialSession(serverEndpoint: endpoint, record: record)
        deleted = false
        return SecureCredentialPersistenceReceipt(session: session)
    }
    func update(_ record: DeviceCredentialRecord, for endpoint: ServerEndpoint) async throws
        -> SecureCredentialPersistenceReceipt
    {
        try await store(record, for: endpoint)
    }
    func isActiveCredentialAbsent() async throws -> Bool { deleted }
    func load(expectedServerEndpoint: ServerEndpoint?) async throws -> DeviceCredentialSession {
        if let gate {
            self.gate = nil
            await gate.arrive()
        }
        if deleted { throw SecureCredentialSinkError.itemNotFound }
        let loaded = session
        if let replacementAfterLoad {
            session = replacementAfterLoad
            self.replacementAfterLoad = nil
        }
        return loaded
    }
    func delete() async throws { deleted = true }
    func replace(_ session: DeviceCredentialSession) {
        self.session = session
        deleted = false
    }
    func replaceAfterNextLoad(_ replacement: DeviceCredentialSession) {
        replacementAfterLoad = replacement
    }
    func suspendNextLoad(_ gate: QueueGate) { self.gate = gate }
}

actor QueueTransport: HTTPTransportProtocol {
    private var response: HTTPTransportResponse
    private var error: SynveilTransportError?
    private var sent: [HTTPTransportRequest] = []
    private var gate: QueueGate?
    init(_ response: HTTPTransportResponse) { self.response = response }
    func send(_ request: HTTPTransportRequest) async throws -> HTTPTransportResponse {
        sent.append(request)
        if let gate {
            self.gate = nil
            await gate.arrive()
        }
        if let error { throw error }
        return response
    }
    func set(_ response: HTTPTransportResponse) {
        self.response = response
        error = nil
    }
    func fail(_ error: SynveilTransportError) { self.error = error }
    func requests() -> [HTTPTransportRequest] { sent }
    func suspendNextRequest(_ gate: QueueGate) { self.gate = gate }
}

func queueSession(_ scope: ClientMutationScope, credential: Int = 92, bearer: String = queueBearer)
    throws -> DeviceCredentialSession
{
    DeviceCredentialSession(
        serverEndpoint: scope.serverEndpoint,
        record: try DeviceCredentialRecord(
            ownerUserId: scope.ownerUserId, deviceId: scope.deviceId.rawValue,
            credentialId: queueUUID(credential),
            credential: DeviceCredential(validatedRawValue: bearer),
            createdAt: "2026-10-09T12:00:00Z"))
}

@MainActor
struct QueueFixture {
    let url: URL
    let scope: ClientMutationScope
    let database: MutationQueueSQLiteStore
    let controller: SessionController
    let credentials: QueueCredentialStore
    let transport: QueueTransport
    let provider: AuthenticatedLibraryRequestProvider
    let queue: DurableMutationQueue
    let service: SyncCheckpointService
}

@MainActor
func queueFixture(
    _ test: XCTestCase, initializeBase: Bool = true,
    fault: QueueFaultInjector? = nil,
    maximumOutstanding: Int = MutationQueuePolicy.maximumOutstandingPerScope,
    maximumBytes: Int = MutationQueuePolicy.maximumStoredBytes,
    maximumRecords: Int = MutationQueuePolicy.maximumRecords
) async throws -> QueueFixture {
    let url = queueTemporaryURL(test)
    let scope = try await queueScope()
    let faultCallback: (@Sendable (MutationQueueFaultPoint) throws -> Void)?
    if let fault { faultCallback = { try fault.hit($0) } } else { faultCallback = nil }
    let database = try MutationQueueSQLiteStore(
        url: url, maximumOutstanding: maximumOutstanding, maximumBytes: maximumBytes,
        maximumRecords: maximumRecords, fault: faultCallback)
    let session = try queueSession(scope)
    let credentials = QueueCredentialStore(session)
    let controller = SessionController(
        logoutService: SessionLogoutService(credentialStore: credentials))
    controller.showServerProfileSetup()
    controller.configureServerEndpoint(scope.serverEndpoint)
    controller.requireEnrollment()
    controller.markAuthenticated(after: SecureCredentialPersistenceReceipt(session: session))
    let transport = QueueTransport(try queueHTTP(queueCheckpointObject(scope: scope)))
    let provider = AuthenticatedLibraryRequestProvider(
        controller: controller, store: credentials, transport: transport)
    let queue = DurableMutationQueue(store: database, provider: provider, bridge: QueueValidator())
    let service = SyncCheckpointService(provider: provider, queue: queue, bridge: QueueValidator())
    controller.installMutationSessionInvalidator { [weak queue] in queue?.invalidateSession() }
    if initializeBase {
        let result = await service.prepare(scope: scope)
        guard case .prepared = result else { throw MutationQueueFailure.syncBaseUnavailable }
    }
    return QueueFixture(
        url: url, scope: scope, database: database, controller: controller,
        credentials: credentials,
        transport: transport, provider: provider, queue: queue, service: service)
}

@MainActor
func queueEnqueued(_ fixture: QueueFixture, mutation: PreparedClientMutation? = nil) async throws
    -> MutationQueueRecord
{
    let resolvedMutation: PreparedClientMutation
    if let mutation {
        resolvedMutation = mutation
    } else {
        resolvedMutation = try await queuePrepared(scope: fixture.scope)
    }
    let mutation = resolvedMutation
    guard case .enqueued(let row) = await fixture.queue.enqueue(mutation) else {
        throw MutationQueueFailure.invalidOperation
    }
    return row
}

@MainActor
func queueRecord(_ fixture: QueueFixture, id: Int = 10) async throws -> MutationQueueRecord {
    let id = try await ClientMutationId.validated(queueUUID(id), using: QueueValidator())
    guard case .record(let record) = await fixture.queue.get(scope: fixture.scope, mutationId: id)
    else { throw MutationQueueFailure.notFound }
    return record
}

@MainActor
func queueAssertFailure<T>(
    _ expected: MutationQueueFailure, file: StaticString = #filePath, line: UInt = #line,
    operation: () async throws -> T
) async {
    do {
        _ = try await operation()
        XCTFail("Expected typed failure", file: file, line: line)
    } catch { XCTAssertEqual(error as? MutationQueueFailure, expected, file: file, line: line) }
}

/// Suspends after a real SQLite commit, before the queue receives its acknowledgement.
struct QueueDelayedStorage: MutationQueueStorageProtocol {
    let base: MutationQueueSQLiteStore
    let gate: QueueGate
    func bindSession(scope: ClientMutationScope, credentialId: String) async throws {
        try await base.bindSession(scope: scope, credentialId: credentialId)
    }
    func quarantineSession(scope: ClientMutationScope) async throws {
        try await base.quarantineSession(scope: scope)
    }
    func syncBase(scope: ClientMutationScope) async throws -> StoredSyncBase? {
        try await base.syncBase(scope: scope)
    }
    func persistCheckpoint(_ checkpoint: SyncCheckpoint) async throws -> SyncBaseStatus {
        try await base.persistCheckpoint(checkpoint)
    }
    func invalidateBase(scope: ClientMutationScope) async throws {
        try await base.invalidateBase(scope: scope)
    }
    func enqueue(_ mutation: PreparedClientMutation, payload: Data) async throws -> (
        StoredMutationRecord, Bool
    ) {
        let committed = try await base.enqueue(mutation, payload: payload)
        await gate.arrive()
        return committed
    }
    func records(scope: ClientMutationScope, limit: Int) async throws -> [StoredMutationRecord] {
        try await base.records(scope: scope, limit: limit)
    }
    func record(scope: ClientMutationScope, id: String) async throws -> StoredMutationRecord? {
        try await base.record(scope: scope, id: id)
    }
    func recoverInterruptedOperations() async throws -> Int {
        try await base.recoverInterruptedOperations()
    }
    func beginAttempt(scope: ClientMutationScope, id: String, owner: String, attemptId: String)
        async throws -> StoredMutationRecord
    {
        try await base.beginAttempt(scope: scope, id: id, owner: owner, attemptId: attemptId)
    }
    func authorizeAttempt(_ mutation: PreparedClientMutation, owner: String, attemptId: String)
        async throws
    { try await base.authorizeAttempt(mutation, owner: owner, attemptId: attemptId) }
    func finishAttempt(
        scope: ClientMutationScope, id: String, owner: String, attemptId: String,
        state: MutationQueueState, evidence: Data
    ) async throws {
        try await base.finishAttempt(
            scope: scope, id: id, owner: owner, attemptId: attemptId, state: state,
            evidence: evidence)
    }
}

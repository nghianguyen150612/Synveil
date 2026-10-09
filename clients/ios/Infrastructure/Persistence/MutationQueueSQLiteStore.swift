import Foundation
import SQLite3

/// Deterministic boundary injection for tests; absent in production composition.
enum MutationQueueFaultPoint: Equatable, Sendable {
    case migration, beforeInsert, afterInsert, beforeCommit, afterCommit
    case afterSubmitting, afterDispatchOwnership, beforeResultPersistence, duringConflictPersistence
}

/// One owned connection, accessed exclusively by MutationQueueSQLiteStore's actor.
/// This wrapper only permits actor-independent destruction of the C handle.
private final class MutationSQLiteConnection: @unchecked Sendable {
    let handle: OpaquePointer
    init(_ handle: OpaquePointer) { self.handle = handle }
    deinit { sqlite3_close_v2(handle) }
}

/// Serial transactions never suspend. The application creates exactly one store; independent
/// connections still use SQLite's writer lock, unique keys, and bounded busy timeout.
actor MutationQueueSQLiteStore: MutationQueueStorageProtocol {
    private let connection: MutationSQLiteConnection
    private let url: URL
    private let fault: (@Sendable (MutationQueueFaultPoint) throws -> Void)?
    private let maximumOutstanding: Int
    private let maximumBytes: Int
    private let maximumRecords: Int
    static let schemaVersion = 1
    private static let scopePredicate = "endpoint=? AND owner_id=? AND device_id=? AND library_id=?"
    private static let columns =
        "mutation_id,epoch,sequence,kind,payload,request,encoding_version,enqueue_order,created_at,state,attempt_id,attempt_owner,attempt_started,dispatch_recorded,evidence"
    private static let schema = [
        """
        CREATE TABLE scopes (
          scope_id INTEGER PRIMARY KEY,
          endpoint TEXT NOT NULL, owner_id TEXT NOT NULL, device_id TEXT NOT NULL, library_id TEXT NOT NULL,
          credential_id TEXT, quarantined INTEGER NOT NULL DEFAULT 0 CHECK(quarantined IN (0,1)),
          UNIQUE(endpoint,owner_id,device_id,library_id)
        )
        """,
        """
        CREATE TABLE sync_bases (
          scope_id INTEGER PRIMARY KEY REFERENCES scopes(scope_id),
          epoch TEXT NOT NULL, sequence TEXT NOT NULL, response BLOB NOT NULL CHECK(length(response) BETWEEN 1 AND 16384),
          status TEXT NOT NULL CHECK(status IN ('VERIFIED','INVALID','RECONCILIATION_REQUIRED'))
        )
        """,
        """
        CREATE TABLE mutations (
          enqueue_order INTEGER PRIMARY KEY AUTOINCREMENT,
          scope_id INTEGER NOT NULL REFERENCES scopes(scope_id), mutation_id TEXT NOT NULL,
          epoch TEXT NOT NULL, sequence TEXT NOT NULL,
          kind TEXT NOT NULL CHECK(kind IN ('CREATE_DIRECTORY','RENAME_NODE','MOVE_NODE','TRASH_NODE','RESTORE_NODE')),
          payload BLOB NOT NULL CHECK(length(payload) BETWEEN 1 AND 16384),
          request BLOB NOT NULL CHECK(length(request) BETWEEN 1 AND 16384),
          encoding_version INTEGER NOT NULL CHECK(encoding_version=1), created_at REAL NOT NULL,
          state TEXT NOT NULL CHECK(state IN ('PENDING','SUBMITTING','OUTCOME_UNKNOWN','APPLIED','CONFLICT','BLOCKED_REBASELINE','FAILED_PERMANENT')),
          attempt_id TEXT, attempt_owner TEXT, attempt_started REAL,
          dispatch_recorded INTEGER NOT NULL DEFAULT 0 CHECK(dispatch_recorded IN (0,1)),
          evidence BLOB CHECK(evidence IS NULL OR length(evidence) BETWEEN 1 AND 65536),
          UNIQUE(scope_id,mutation_id),
          CHECK((state='PENDING' AND attempt_id IS NULL AND attempt_owner IS NULL AND attempt_started IS NULL AND dispatch_recorded=0 AND evidence IS NULL)
            OR (state<>'PENDING' AND attempt_id IS NOT NULL AND attempt_owner IS NOT NULL AND attempt_started IS NOT NULL)),
          CHECK((state IN ('PENDING','SUBMITTING') AND evidence IS NULL) OR (state NOT IN ('PENDING','SUBMITTING') AND evidence IS NOT NULL))
        )
        """,
        """
        CREATE TABLE dependencies (
          enqueue_order INTEGER NOT NULL REFERENCES mutations(enqueue_order), node_id TEXT NOT NULL,
          PRIMARY KEY(enqueue_order,node_id)
        )
        """,
        "CREATE INDEX mutation_scope_order ON mutations(scope_id,enqueue_order)",
        "CREATE INDEX dependency_node ON dependencies(node_id,enqueue_order)",
        """
        CREATE TRIGGER mutation_immutable BEFORE UPDATE OF scope_id,mutation_id,epoch,sequence,kind,payload,request,encoding_version,enqueue_order,created_at ON mutations
        BEGIN SELECT RAISE(ABORT,'immutable mutation'); END
        """,
        """
        CREATE TRIGGER mutation_transition BEFORE UPDATE OF state ON mutations
        WHEN NOT (OLD.state='PENDING' AND NEW.state='SUBMITTING')
          AND NOT (OLD.state='SUBMITTING' AND NEW.state IN ('OUTCOME_UNKNOWN','APPLIED','CONFLICT','BLOCKED_REBASELINE','FAILED_PERMANENT'))
        BEGIN SELECT RAISE(ABORT,'invalid transition'); END
        """,
    ]

    init(
        url: URL, busyTimeoutMilliseconds: Int32 = 250,
        maximumOutstanding: Int = MutationQueuePolicy.maximumOutstandingPerScope,
        maximumBytes: Int = MutationQueuePolicy.maximumStoredBytes,
        maximumRecords: Int = MutationQueuePolicy.maximumRecords,
        fault: (@Sendable (MutationQueueFaultPoint) throws -> Void)? = nil
    ) throws {
        guard url.isFileURL, busyTimeoutMilliseconds >= 0, busyTimeoutMilliseconds <= 5000,
            maximumOutstanding > 0, maximumBytes > 0, maximumRecords > 0
        else { throw MutationQueueFailure.databaseOpen }
        self.url = url
        self.fault = fault
        self.maximumOutstanding = maximumOutstanding
        self.maximumBytes = maximumBytes
        self.maximumRecords = maximumRecords
        try MutationQueueFilePolicy.prepare(url)
        var handle: OpaquePointer?
        let status = sqlite3_open_v2(
            url.path, &handle, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX,
            nil)
        guard status == SQLITE_OK, let handle else {
            if let handle { sqlite3_close_v2(handle) }
            throw MutationQueueFailure.databaseOpen
        }
        connection = MutationSQLiteConnection(handle)
        sqlite3_extended_result_codes(handle, 1)
        sqlite3_busy_timeout(handle, busyTimeoutMilliseconds)
        // Initialization uses only local helpers; no actor reference escapes before schema validation.
        try Self.execute(handle, "PRAGMA foreign_keys=ON")
        let version = try Self.scalar(handle, "PRAGMA user_version")
        guard version == 0 || version == Int64(Self.schemaVersion) else {
            throw MutationQueueFailure.unsupportedSchema
        }
        if version == 0 {
            guard
                try Self.scalar(
                    handle, "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
                    == 0
            else {
                throw MutationQueueFailure.corrupt
            }
            try Self.execute(handle, "BEGIN IMMEDIATE")
            do {
                for statement in Self.schema { try Self.execute(handle, statement) }
                try fault?(.migration)
                try Self.execute(handle, "PRAGMA user_version=1")
                try Self.execute(handle, "COMMIT")
            } catch {
                try? Self.execute(handle, "ROLLBACK")
                throw error
            }
        }
        let registered = try Self.query(
            handle, "SELECT sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY sql"
        ) { s in
            try Self.text(s, 0)
        }
        guard registered.sorted() == Self.schema.sorted() else {
            throw MutationQueueFailure.corrupt
        }
        let integrity = try Self.query(handle, "PRAGMA quick_check") { try Self.text($0, 0) }
        guard integrity == ["ok"],
            try Self.query(handle, "PRAGMA foreign_key_check", map: { _ in 1 }).isEmpty
        else {
            throw MutationQueueFailure.corrupt
        }
        let journal = try Self.query(handle, "PRAGMA journal_mode=WAL") { try Self.text($0, 0) }
        guard journal == ["wal"] else { throw MutationQueueFailure.io }
        try Self.execute(handle, "PRAGMA synchronous=FULL")
        try Self.execute(handle, "PRAGMA wal_autocheckpoint=64")
        try Self.execute(handle, "PRAGMA journal_size_limit=2097152")
        let pageSize = try Self.scalar(handle, "PRAGMA page_size")
        guard pageSize > 0 else { throw MutationQueueFailure.corrupt }
        guard try Self.scalar(handle, "PRAGMA page_count") * pageSize <= 64 * 1024 * 1024 else {
            throw MutationQueueFailure.capacity
        }
        // This is a static, validated local integer policy, never untrusted SQL text.
        try Self.execute(handle, "PRAGMA max_page_count=\(64 * 1024 * 1024 / pageSize)")
        try MutationQueueFilePolicy.protect(url)
    }

    /// Internal deterministic configuration inspection for portable/native persistence tests.
    func durabilityConfiguration() throws -> [Int64] {
        try [
            scalar("PRAGMA foreign_keys"), scalar("PRAGMA synchronous"),
            scalar("PRAGMA wal_autocheckpoint"),
        ]
    }

    static func productionURL() throws -> URL {
        guard
            let root = FileManager.default.urls(
                for: .applicationSupportDirectory, in: .userDomainMask
            ).first
        else {
            throw MutationQueueFailure.databaseOpen
        }
        return root.appendingPathComponent("Synveil/MutationQueue", isDirectory: true)
            .appendingPathComponent("mutations.sqlite")
    }

    func bindSession(scope: ClientMutationScope, credentialId: String) throws {
        if let id = try scopeId(scope) {
            let binding = try query(
                "SELECT credential_id,quarantined FROM scopes WHERE scope_id=?", [.integer(id)]
            ) { s in
                (
                    sqlite3_column_type(s, 0) == SQLITE_NULL ? nil : try Self.text(s, 0),
                    sqlite3_column_int(s, 1)
                )
            }.first!
            guard binding.1 == 0, binding.0 == nil || binding.0 == credentialId else {
                throw MutationQueueFailure.scopeMismatch
            }
            if binding.0 == credentialId { return }
        }
        try transaction {
            let id = try ensureScope(scope)
            let binding = try query(
                "SELECT credential_id,quarantined FROM scopes WHERE scope_id=?", [.integer(id)]
            ) { s in
                (
                    sqlite3_column_type(s, 0) == SQLITE_NULL ? nil : try Self.text(s, 0),
                    sqlite3_column_int(s, 1)
                )
            }.first!
            guard binding.1 == 0, binding.0 == nil || binding.0 == credentialId else {
                throw MutationQueueFailure.scopeMismatch
            }
            if binding.0 == nil {
                try execute(
                    "UPDATE scopes SET credential_id=? WHERE scope_id=?",
                    [.text(credentialId), .integer(id)])
            }
        }
    }

    func quarantineSession(scope: ClientMutationScope) throws {
        try transaction {
            if let id = try scopeId(scope) {
                try execute("UPDATE scopes SET quarantined=1 WHERE scope_id=?", [.integer(id)])
            }
        }
    }

    func syncBase(scope: ClientMutationScope) throws -> StoredSyncBase? {
        guard let id = try scopeId(scope) else { return nil }
        return try query(
            "SELECT epoch,sequence,response,status FROM sync_bases WHERE scope_id=?", [.integer(id)]
        ) { s in
            guard let status = SyncBaseStatus(rawValue: try Self.text(s, 3)) else {
                throw MutationQueueFailure.malformedRecord
            }
            return StoredSyncBase(
                scope: scope, epoch: try Self.text(s, 0), sequence: try Self.text(s, 1),
                responseBody: try Self.blob(
                    s, 2, maximum: MutationQueuePolicy.maximumCheckpointBytes), status: status)
        }.first
    }

    func persistCheckpoint(_ checkpoint: SyncCheckpoint) throws -> SyncBaseStatus {
        try transaction {
            let scope = checkpoint.base.scope
            let id = try ensureScope(scope)
            let epoch = checkpoint.base.epoch.rawValue
            let sequence = checkpoint.base.sequence.rawValue
            let incompatible =
                try scalar(
                    "SELECT count(*) FROM mutations WHERE scope_id=? AND state NOT IN ('APPLIED','FAILED_PERMANENT') AND (epoch<>? OR sequence<>?)",
                    [.integer(id), .text(epoch), .text(sequence)]) > 0
            let old = try syncBase(scope: scope)
            // Never silently rewind an otherwise usable persisted checkpoint.
            let rewind =
                old.map {
                    SyncDecimalValidation.less(epoch, $0.epoch)
                        || epoch == $0.epoch && SyncDecimalValidation.less(sequence, $0.sequence)
                } ?? false
            let status: SyncBaseStatus =
                incompatible || rewind || old?.status == .reconciliationRequired
                ? .reconciliationRequired : .verified
            let oldBytes = old?.responseBody.count ?? 0
            try checkCapacity(additionalBytes: checkpoint.responseBody.count - oldBytes)
            try execute(
                "INSERT INTO sync_bases(scope_id,epoch,sequence,response,status) VALUES(?,?,?,?,?) ON CONFLICT(scope_id) DO UPDATE SET epoch=excluded.epoch,sequence=excluded.sequence,response=excluded.response,status=excluded.status",
                [
                    .integer(id), .text(epoch), .text(sequence), .blob(checkpoint.responseBody),
                    .text(status.rawValue),
                ])
            return status
        }
    }

    func invalidateBase(scope: ClientMutationScope) throws {
        try transaction {
            if let id = try scopeId(scope) {
                try execute(
                    "UPDATE sync_bases SET status='INVALID' WHERE scope_id=? AND status='VERIFIED'",
                    [.integer(id)])
            }
        }
    }

    func enqueue(_ mutation: PreparedClientMutation, payload: Data) throws -> (
        StoredMutationRecord, Bool
    ) {
        try transaction {
            let scope = mutation.base.scope
            guard let id = try scopeId(scope) else {
                throw MutationQueueFailure.syncBaseUnavailable
            }
            try requireUnquarantinedScope(scope)
            if let existing = try record(scope: scope, id: mutation.id.rawValue) {
                guard same(existing, mutation: mutation, payload: payload) else {
                    throw MutationQueueFailure.duplicateIdentity
                }
                return (existing, false)
            }
            try requireBase(mutation)
            guard payload.count <= ClientMutationPolicy.maximumRequestBytes,
                mutation.requestBody.count <= ClientMutationPolicy.maximumRequestBytes
            else { throw MutationQueueFailure.invalidOperation }
            guard
                try scalar(
                    "SELECT count(*) FROM mutations WHERE scope_id=? AND state NOT IN ('APPLIED','FAILED_PERMANENT')",
                    [.integer(id)]) < maximumOutstanding,
                try scalar("SELECT count(*) FROM mutations") < maximumRecords
            else { throw MutationQueueFailure.capacity }
            try checkCapacity(additionalBytes: payload.count + mutation.requestBody.count)
            for resource in mutation.payload.intent.resourceIds {
                let conflict = try scalar(
                    "SELECT count(*) FROM dependencies d JOIN mutations m ON m.enqueue_order=d.enqueue_order WHERE m.scope_id=? AND m.state NOT IN ('APPLIED','FAILED_PERMANENT') AND d.node_id=?",
                    [.integer(id), .text(resource.rawValue)])
                guard conflict == 0 else { throw MutationQueueFailure.dependencyConflict }
            }
            try fault?(.beforeInsert)
            try execute(
                "INSERT INTO mutations(scope_id,mutation_id,epoch,sequence,kind,payload,request,encoding_version,created_at,state) VALUES(?,?,?,?,?,?,?,?,?,'PENDING')",
                [
                    .integer(id), .text(mutation.id.rawValue), .text(mutation.base.epoch.rawValue),
                    .text(mutation.base.sequence.rawValue),
                    .text(mutation.kind.rawValue), .blob(payload), .blob(mutation.requestBody),
                    .integer(1), .real(Date().timeIntervalSince1970),
                ])
            let order = sqlite3_last_insert_rowid(connection.handle)
            for resource in mutation.payload.intent.resourceIds {
                try execute(
                    "INSERT INTO dependencies(enqueue_order,node_id) VALUES(?,?)",
                    [.integer(order), .text(resource.rawValue)])
            }
            try fault?(.afterInsert)
            guard let row = try record(scope: scope, id: mutation.id.rawValue) else {
                throw MutationQueueFailure.corrupt
            }
            return (row, true)
        }
    }

    func records(scope: ClientMutationScope, limit: Int) throws -> [StoredMutationRecord] {
        guard (1...MutationQueuePolicy.maximumReadBatch).contains(limit) else {
            throw MutationQueueFailure.invalidLimit
        }
        guard let id = try scopeId(scope) else { return [] }
        return try query(
            "SELECT \(Self.columns) FROM mutations WHERE scope_id=? AND state NOT IN ('APPLIED','FAILED_PERMANENT') ORDER BY enqueue_order LIMIT ?",
            [.integer(id), .integer(Int64(limit))]
        ) { try read($0, scope: scope) }
    }

    func record(scope: ClientMutationScope, id: String) throws -> StoredMutationRecord? {
        guard let scopeId = try scopeId(scope) else { return nil }
        return try query(
            "SELECT \(Self.columns) FROM mutations WHERE scope_id=? AND mutation_id=?",
            [.integer(scopeId), .text(id)]
        ) { try read($0, scope: scope) }.first
    }

    func recoverInterruptedOperations() throws -> Int {
        let evidence = try JSONEncoder().encode(
            MutationOutcomeEvidence(
                category: .outcomeUnknown, responseStatus: nil,
                responseBody: nil, rejection: nil, uncertainty: "INTERRUPTED"))
        return try transaction {
            let count = try scalar("SELECT count(*) FROM mutations WHERE state='SUBMITTING'")
            try checkCapacity(additionalBytes: Int(count) * evidence.count)
            try execute(
                "UPDATE mutations SET state='OUTCOME_UNKNOWN',evidence=? WHERE state='SUBMITTING'",
                [.blob(evidence)])
            return Int(count)
        }
    }

    func beginAttempt(scope: ClientMutationScope, id: String, owner: String, attemptId: String)
        throws -> StoredMutationRecord
    {
        try transaction {
            try requireUnquarantinedScope(scope)
            guard let row = try record(scope: scope, id: id) else {
                throw MutationQueueFailure.notFound
            }
            guard row.state.permits(.submitting), UUID(uuidString: owner) != nil,
                UUID(uuidString: attemptId) != nil
            else { throw MutationQueueFailure.invalidTransition }
            guard let base = try syncBase(scope: scope), base.status == .verified,
                row.epoch == base.epoch, row.sequence == base.sequence
            else { throw MutationQueueFailure.reconciliationRequired }
            let scopeId = try requireScopeId(scope)
            try execute(
                "UPDATE mutations SET state='SUBMITTING',attempt_id=?,attempt_owner=?,attempt_started=? WHERE scope_id=? AND mutation_id=? AND state='PENDING'",
                [
                    .text(attemptId), .text(owner), .real(Date().timeIntervalSince1970),
                    .integer(scopeId), .text(id),
                ])
            guard sqlite3_changes(connection.handle) == 1 else {
                throw MutationQueueFailure.invalidTransition
            }
            try fault?(.afterSubmitting)
            return try requireRecord(scope: scope, id: id)
        }
    }

    func authorizeAttempt(_ mutation: PreparedClientMutation, owner: String, attemptId: String)
        throws
    {
        try transaction {
            try requireUnquarantinedScope(mutation.base.scope)
            let row = try requireRecord(scope: mutation.base.scope, id: mutation.id.rawValue)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
            guard same(row, mutation: mutation, payload: try encoder.encode(mutation.payload))
            else { throw MutationQueueFailure.duplicateIdentity }
            try requireBase(mutation)
            guard row.state == .submitting, let attempt = row.attempt, attempt.id == attemptId,
                attempt.owner == owner, !attempt.dispatchRecorded
            else { throw MutationQueueFailure.ownershipRequired }
            try execute(
                "UPDATE mutations SET dispatch_recorded=1 WHERE scope_id=? AND mutation_id=? AND state='SUBMITTING' AND attempt_id=? AND attempt_owner=? AND dispatch_recorded=0",
                [
                    .integer(try requireScopeId(mutation.base.scope)), .text(mutation.id.rawValue),
                    .text(attemptId), .text(owner),
                ])
            guard sqlite3_changes(connection.handle) == 1 else {
                throw MutationQueueFailure.ownershipRequired
            }
            try fault?(.afterDispatchOwnership)
        }
    }

    func finishAttempt(
        scope: ClientMutationScope, id: String, owner: String, attemptId: String,
        state: MutationQueueState, evidence: Data
    ) throws {
        try transaction {
            try requireUnquarantinedScope(scope)
            let row = try requireRecord(scope: scope, id: id)
            guard row.state.permits(state), let attempt = row.attempt, attempt.id == attemptId,
                attempt.owner == owner
            else { throw MutationQueueFailure.invalidTransition }
            guard evidence.count <= ClientMutationPolicy.maximumResponseBytes else {
                throw MutationQueueFailure.capacity
            }
            try checkCapacity(additionalBytes: evidence.count)
            try fault?(.beforeResultPersistence)
            try execute(
                "UPDATE mutations SET state=?,evidence=? WHERE scope_id=? AND mutation_id=? AND state='SUBMITTING' AND attempt_id=? AND attempt_owner=?",
                [
                    .text(state.rawValue), .blob(evidence), .integer(try requireScopeId(scope)),
                    .text(id), .text(attemptId), .text(owner),
                ])
            guard sqlite3_changes(connection.handle) == 1 else {
                throw MutationQueueFailure.invalidTransition
            }
            if state == .conflict { try fault?(.duringConflictPersistence) }
            if state == .blockedRebaseline {
                try execute(
                    "UPDATE sync_bases SET status='RECONCILIATION_REQUIRED' WHERE scope_id=?",
                    [.integer(try requireScopeId(scope))])
            }
        }
    }

    private func requireUnquarantinedScope(_ scope: ClientMutationScope) throws {
        let id = try requireScopeId(scope)
        guard try scalar("SELECT quarantined FROM scopes WHERE scope_id=?", [.integer(id)]) == 0
        else {
            throw MutationQueueFailure.scopeMismatch
        }
    }

    private func requireBase(_ mutation: PreparedClientMutation) throws {
        guard let base = try syncBase(scope: mutation.base.scope) else {
            throw MutationQueueFailure.syncBaseUnavailable
        }
        guard base.status == .verified, base.epoch == mutation.base.epoch.rawValue,
            base.sequence == mutation.base.sequence.rawValue
        else { throw MutationQueueFailure.reconciliationRequired }
    }

    private func same(_ row: StoredMutationRecord, mutation: PreparedClientMutation, payload: Data)
        -> Bool
    {
        row.scope == mutation.base.scope && row.mutationId == mutation.id.rawValue
            && row.epoch == mutation.base.epoch.rawValue
            && row.sequence == mutation.base.sequence.rawValue && row.kind == mutation.kind.rawValue
            && row.encodingVersion == MutationQueuePolicy.encodingVersion && row.payload == payload
            && row.request == mutation.requestBody
    }

    private func read(_ s: OpaquePointer, scope: ClientMutationScope) throws -> StoredMutationRecord
    {
        guard let state = MutationQueueState(rawValue: try Self.text(s, 9)) else {
            throw MutationQueueFailure.malformedRecord
        }
        let attempt: MutationAttemptMetadata?
        if sqlite3_column_type(s, 10) != SQLITE_NULL {
            guard
                sqlite3_column_type(s, 12) == SQLITE_FLOAT
                    || sqlite3_column_type(s, 12) == SQLITE_INTEGER,
                sqlite3_column_type(s, 13) == SQLITE_INTEGER,
                [0, 1].contains(sqlite3_column_int(s, 13))
            else { throw MutationQueueFailure.malformedRecord }
            attempt = MutationAttemptMetadata(
                id: try Self.text(s, 10), owner: try Self.text(s, 11),
                startedAt: Date(timeIntervalSince1970: sqlite3_column_double(s, 12)),
                dispatchRecorded: sqlite3_column_int(s, 13) == 1)
        } else {
            guard sqlite3_column_type(s, 11) == SQLITE_NULL,
                sqlite3_column_type(s, 12) == SQLITE_NULL,
                sqlite3_column_int(s, 13) == 0
            else { throw MutationQueueFailure.malformedRecord }
            attempt = nil
        }
        guard sqlite3_column_type(s, 6) == SQLITE_INTEGER,
            sqlite3_column_type(s, 7) == SQLITE_INTEGER,
            sqlite3_column_type(s, 8) == SQLITE_FLOAT || sqlite3_column_type(s, 8) == SQLITE_INTEGER
        else { throw MutationQueueFailure.malformedRecord }
        let row = StoredMutationRecord(
            scope: scope, mutationId: try Self.text(s, 0), epoch: try Self.text(s, 1),
            sequence: try Self.text(s, 2), kind: try Self.text(s, 3),
            payload: try Self.blob(s, 4, maximum: 16384),
            request: try Self.blob(s, 5, maximum: 16384),
            encodingVersion: Int(sqlite3_column_int64(s, 6)),
            order: sqlite3_column_int64(s, 7),
            createdAt: Date(timeIntervalSince1970: sqlite3_column_double(s, 8)),
            state: state, attempt: attempt,
            evidence: sqlite3_column_type(s, 14) == SQLITE_NULL
                ? nil : try Self.blob(s, 14, maximum: 65536))
        // Derived dependency rows must exactly match payload IDs; higher-level codec validates IDs/semantics.
        guard
            let payload = try? JSONSerialization.jsonObject(with: row.payload) as? [String: String]
        else { throw MutationQueueFailure.malformedRecord }
        let expected = Set(
            [
                payload["node_id"], payload["parent_node_id"], payload["new_parent_node_id"],
                payload["expected_parent_node_id"],
            ].compactMap { $0 })
        let actual = try query(
            "SELECT node_id FROM dependencies WHERE enqueue_order=?", [.integer(row.order)]
        ) { try Self.text($0, 0) }
        guard Set(actual) == expected else { throw MutationQueueFailure.malformedRecord }
        return row
    }

    private func requireRecord(scope: ClientMutationScope, id: String) throws
        -> StoredMutationRecord
    {
        guard let row = try record(scope: scope, id: id) else {
            throw MutationQueueFailure.notFound
        }
        return row
    }
    private func scopeValues(_ scope: ClientMutationScope) -> [SQLValue] {
        [
            .text(scope.serverEndpoint.urlString), .text(scope.ownerUserId),
            .text(scope.deviceId.rawValue), .text(scope.libraryId.rawValue),
        ]
    }
    private func scopeId(_ scope: ClientMutationScope) throws -> Int64? {
        try query("SELECT scope_id FROM scopes WHERE \(Self.scopePredicate)", scopeValues(scope)) {
            sqlite3_column_int64($0, 0)
        }.first
    }
    private func requireScopeId(_ scope: ClientMutationScope) throws -> Int64 {
        guard let id = try scopeId(scope) else { throw MutationQueueFailure.notFound }
        return id
    }
    private func ensureScope(_ scope: ClientMutationScope) throws -> Int64 {
        if let id = try scopeId(scope) { return id }
        guard try scalar("SELECT count(*) FROM scopes") < 1024 else {
            throw MutationQueueFailure.capacity
        }
        try execute(
            "INSERT INTO scopes(endpoint,owner_id,device_id,library_id) VALUES(?,?,?,?)",
            scopeValues(scope))
        return try requireScopeId(scope)
    }
    private func checkCapacity(additionalBytes: Int) throws {
        let bytes = try scalar(
            "SELECT coalesce((SELECT sum(length(request)+length(payload)+coalesce(length(evidence),0)) FROM mutations),0)+coalesce((SELECT sum(length(response)) FROM sync_bases),0)"
        )
        guard bytes + Int64(additionalBytes) <= Int64(maximumBytes) else {
            throw MutationQueueFailure.capacity
        }
    }
    private func transaction<T>(_ operation: () throws -> T) throws -> T {
        try execute("BEGIN IMMEDIATE")
        var committed = false
        do {
            let value = try operation()
            try MutationQueueFilePolicy.protect(url)
            try fault?(.beforeCommit)
            try execute("COMMIT")
            committed = true
            try fault?(.afterCommit)
            return value
        } catch {
            if committed { throw MutationQueueFailure.commitAcknowledgementLost }
            try? execute("ROLLBACK")
            throw error
        }
    }

    private enum SQLValue { case text(String), blob(Data), integer(Int64), real(Double) }
    private static func statement(_ handle: OpaquePointer, _ sql: String, _ values: [SQLValue])
        throws -> OpaquePointer
    {
        var statement: OpaquePointer?
        try check(sqlite3_prepare_v2(handle, sql, -1, &statement, nil))
        guard let statement else { throw MutationQueueFailure.corrupt }
        do {
            guard Int(sqlite3_bind_parameter_count(statement)) == values.count else {
                throw MutationQueueFailure.corrupt
            }
            let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
            for (index, value) in values.enumerated() {
                let position = Int32(index + 1)
                switch value {
                case .text(let text):
                    try check(
                        text.withCString {
                            sqlite3_bind_text(statement, position, $0, -1, transient)
                        })
                case .blob(let data):
                    try check(
                        data.withUnsafeBytes {
                            sqlite3_bind_blob(
                                statement, position, $0.baseAddress, Int32($0.count), transient)
                        })
                case .integer(let number):
                    try check(sqlite3_bind_int64(statement, position, number))
                case .real(let number): try check(sqlite3_bind_double(statement, position, number))
                }
            }
            return statement
        } catch {
            sqlite3_finalize(statement)
            throw error
        }
    }
    private static func execute(_ handle: OpaquePointer, _ sql: String, _ values: [SQLValue] = [])
        throws
    {
        let s = try statement(handle, sql, values)
        defer { sqlite3_finalize(s) }
        var code = sqlite3_step(s)
        while code == SQLITE_ROW { code = sqlite3_step(s) }
        guard code == SQLITE_DONE else {
            try check(code)
            throw MutationQueueFailure.io
        }
    }
    private static func query<T>(
        _ handle: OpaquePointer, _ sql: String, _ values: [SQLValue] = [],
        map: (OpaquePointer) throws -> T
    ) throws -> [T] {
        let s = try statement(handle, sql, values)
        defer { sqlite3_finalize(s) }
        var result: [T] = []
        var code = sqlite3_step(s)
        while code == SQLITE_ROW {
            result.append(try map(s))
            code = sqlite3_step(s)
        }
        guard code == SQLITE_DONE else {
            try check(code)
            throw MutationQueueFailure.io
        }
        return result
    }
    private static func scalar(_ handle: OpaquePointer, _ sql: String, _ values: [SQLValue] = [])
        throws -> Int64
    {
        guard let value = try query(handle, sql, values, map: { sqlite3_column_int64($0, 0) }).first
        else { throw MutationQueueFailure.corrupt }
        return value
    }
    private func execute(_ sql: String, _ values: [SQLValue] = []) throws {
        try Self.execute(connection.handle, sql, values)
    }
    private func query<T>(
        _ sql: String, _ values: [SQLValue] = [], map: (OpaquePointer) throws -> T
    ) throws -> [T] { try Self.query(connection.handle, sql, values, map: map) }
    private func scalar(_ sql: String, _ values: [SQLValue] = []) throws -> Int64 {
        try Self.scalar(connection.handle, sql, values)
    }
    private static func text(_ s: OpaquePointer, _ index: Int32) throws -> String {
        guard sqlite3_column_type(s, index) == SQLITE_TEXT, sqlite3_column_bytes(s, index) <= 16384,
            let bytes = sqlite3_column_text(s, index),
            let text = String(
                data: Data(bytes: bytes, count: Int(sqlite3_column_bytes(s, index))),
                encoding: .utf8),
            !text.contains("\0")
        else { throw MutationQueueFailure.malformedRecord }
        return text
    }
    private static func blob(_ s: OpaquePointer, _ index: Int32, maximum: Int) throws -> Data {
        let count = Int(sqlite3_column_bytes(s, index))
        guard sqlite3_column_type(s, index) == SQLITE_BLOB, count > 0, count <= maximum,
            let pointer = sqlite3_column_blob(s, index)
        else { throw MutationQueueFailure.malformedRecord }
        return Data(bytes: pointer, count: count)
    }
    private static func check(_ code: Int32) throws {
        if code == SQLITE_OK { return }
        switch code & 0xff {
        case SQLITE_BUSY, SQLITE_LOCKED: throw MutationQueueFailure.busy
        case SQLITE_FULL: throw MutationQueueFailure.diskFull
        case SQLITE_CORRUPT, SQLITE_NOTADB, SQLITE_SCHEMA, SQLITE_CONSTRAINT:
            throw MutationQueueFailure.corrupt
        case SQLITE_CANTOPEN: throw MutationQueueFailure.databaseOpen
        default: throw MutationQueueFailure.io
        }
    }
}

/// Protection after first unlock matches the device-only Keychain accessibility model. Metadata
/// remains platform-protected local plaintext; this is not application-level encryption.
enum MutationQueueFilePolicy {
    static func prepare(_ url: URL) throws {
        let parent = url.deletingLastPathComponent()
        do {
            try rejectSymbolicLink(parent)
            try FileManager.default.createDirectory(
                at: parent, withIntermediateDirectories: true,
                attributes: attributes(directory: true))
            try rejectSymbolicLink(url)
            if !FileManager.default.fileExists(atPath: url.path) {
                guard
                    FileManager.default.createFile(
                        atPath: url.path, contents: Data(), attributes: attributes(directory: false)
                    )
                else { throw MutationQueueFailure.databaseOpen }
            }
            try protect(url)
        } catch let failure as MutationQueueFailure { throw failure } catch {
            throw MutationQueueFailure.io
        }
    }
    static func protect(_ url: URL) throws {
        do {
            let parent = url.deletingLastPathComponent()
            try rejectSymbolicLink(parent)
            try FileManager.default.setAttributes(
                attributes(directory: true), ofItemAtPath: parent.path)
            #if canImport(Darwin)
                var excluded = URLResourceValues()
                excluded.isExcludedFromBackup = true
                var directory = parent
                try directory.setResourceValues(excluded)
            #endif
            for path in [url.path, url.path + "-wal", url.path + "-shm"] {
                let file = URL(fileURLWithPath: path)
                try rejectSymbolicLink(file)
                if FileManager.default.fileExists(atPath: path) {
                    try FileManager.default.setAttributes(
                        attributes(directory: false), ofItemAtPath: path)
                    #if canImport(Darwin)
                        var protectedFile = file
                        try protectedFile.setResourceValues(excluded)
                    #endif
                }
            }
        } catch let failure as MutationQueueFailure { throw failure } catch {
            throw MutationQueueFailure.io
        }
    }
    private static func attributes(directory: Bool) -> [FileAttributeKey: Any] {
        var result: [FileAttributeKey: Any] = [.posixPermissions: directory ? 0o700 : 0o600]
        #if os(iOS)
            result[.protectionKey] = FileProtectionType.completeUntilFirstUserAuthentication
        #endif
        return result
    }
    private static func rejectSymbolicLink(_ url: URL) throws {
        if let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            attributes[.type] as? FileAttributeType == .typeSymbolicLink
        {
            throw MutationQueueFailure.databaseOpen
        }
    }
}

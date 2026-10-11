import Foundation
import SQLite3

#if canImport(Darwin)
    import Darwin
#else
    import Glibc
#endif

/// Deterministic boundary injection for tests; absent in production composition.
enum MutationQueueFaultPoint: Equatable, Sendable {
    case beforeMaterialization, afterMaterialization, afterFirstNodeWrite, afterFinalNodeWrite
    case beforeProjectionCommit, afterProjectionCommit, beforeAckLeaseCommit, afterAckLeaseCommit
    case beforeAckConfirmationCommit, afterAckConfirmationCommit, beforeQuarantine
    case beforeSnapshotPageCommit, beforeSnapshotPreparationCommit,
        beforeSnapshotConfirmationCommit, beforeSnapshotActivationCommit
    case migration, beforeInsert, afterInsert, beforeCommit, afterCommit, beforeRollback
    case afterSubmitting, afterDispatchOwnership, beforeResultPersistence, duringConflictPersistence
}

/// One owned connection, accessed exclusively by MutationQueueSQLiteStore's actor.
/// This wrapper only permits actor-independent destruction of the C handle.
private final class MutationSQLiteConnection: @unchecked Sendable {
    let handle: OpaquePointer
    // Accessed only synchronously by the actor and SQLite callbacks on this connection.
    var projectionAuthority: Int32 = 0
    var ackLockDescriptor: Int32 = -1
    var ackLockHeld = false
    var inboundLockDescriptor: Int32 = -1
    var inboundLockOwner: UUID?
    func acquireInboundLock(path: String, owner: UUID) throws {
        guard inboundLockOwner == nil else { throw MutationQueueFailure.concurrentExecution }
        if inboundLockDescriptor == -1 {
            inboundLockDescriptor = open(path, O_RDWR | O_CREAT | O_NOFOLLOW, mode_t(0o600))
        }
        guard inboundLockDescriptor >= 0 else { throw MutationQueueFailure.io }
        guard flock(inboundLockDescriptor, LOCK_EX | LOCK_NB) == 0 else {
            throw MutationQueueFailure.concurrentExecution
        }
        inboundLockOwner = owner
    }
    func releaseInboundLock(owner: UUID) {
        guard inboundLockOwner == owner else { return }
        _ = flock(inboundLockDescriptor, LOCK_UN)
        inboundLockOwner = nil
    }
    func acquireAckLock(path: String) throws {
        guard !ackLockHeld else { throw SyncFeedFailure.applicationCommitRequired }
        if ackLockDescriptor == -1 {
            ackLockDescriptor = open(path, O_RDWR | O_CREAT | O_NOFOLLOW, mode_t(0o600))
        }
        guard ackLockDescriptor >= 0 else { throw MutationQueueFailure.io }
        guard flock(ackLockDescriptor, LOCK_EX | LOCK_NB) == 0 else {
            throw SyncFeedFailure.applicationCommitRequired
        }
        ackLockHeld = true
    }
    func releaseAckLock() {
        if ackLockHeld {
            _ = flock(ackLockDescriptor, LOCK_UN)
            ackLockHeld = false
        }
    }
    init(_ handle: OpaquePointer) { self.handle = handle }
    deinit {
        sqlite3_close_v2(handle)
        if ackLockDescriptor >= 0 { close(ackLockDescriptor) }
        if inboundLockDescriptor >= 0 { close(inboundLockDescriptor) }
    }
}

/// Serial transactions never suspend. The application creates exactly one store; independent
/// connections still use SQLite's writer lock, unique keys, and bounded busy timeout.
actor MutationQueueSQLiteStore: MutationQueueStorageProtocol, InboundSyncStorageProtocol {
    private let connection: MutationSQLiteConnection
    private var poisoned = false
    private var projectionQuarantineFailed = false
    private let url: URL
    private let fault: (@Sendable (MutationQueueFaultPoint) throws -> Void)?
    private let maximumOutstanding: Int
    private let maximumBytes: Int
    private let maximumRecords: Int
    static let schemaVersion = 5
    private let processOwner = UUID().uuidString
    private var activeAckAttempts: Set<String> = []
    private static let scopePredicate = "endpoint=? AND owner_id=? AND device_id=? AND library_id=?"
    private static let columns =
        "mutation_id,epoch,sequence,kind,payload,request,encoding_version,enqueue_order,created_at,state,attempt_id,attempt_owner,attempt_started,dispatch_recorded,evidence"
    static let version1Schema = [
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

    private static let recoverySchema = [
        """
        CREATE TABLE mutation_attempt_history (
          enqueue_order INTEGER NOT NULL REFERENCES mutations(enqueue_order),
          attempt_id TEXT NOT NULL, attempt_owner TEXT NOT NULL, attempt_started REAL NOT NULL,
          dispatch_recorded INTEGER NOT NULL CHECK(dispatch_recorded IN (0,1)),
          evidence BLOB NOT NULL CHECK(length(evidence) BETWEEN 1 AND 65536),
          PRIMARY KEY(enqueue_order,attempt_id)
        )
        """,
        """
        CREATE TRIGGER attempt_history_immutable BEFORE UPDATE ON mutation_attempt_history
        BEGIN SELECT RAISE(ABORT,'immutable attempt history'); END
        """,
        """
        CREATE TRIGGER attempt_history_retained BEFORE DELETE ON mutation_attempt_history
        BEGIN SELECT RAISE(ABORT,'retained attempt history'); END
        """,
        """
        CREATE TRIGGER mutation_transition BEFORE UPDATE OF state ON mutations
        WHEN NOT (OLD.state='PENDING' AND NEW.state='SUBMITTING')
          AND NOT (OLD.state='SUBMITTING' AND NEW.state IN ('OUTCOME_UNKNOWN','APPLIED','CONFLICT','BLOCKED_REBASELINE','FAILED_PERMANENT'))
          AND NOT (OLD.state='OUTCOME_UNKNOWN' AND NEW.state='SUBMITTING'
            AND NEW.attempt_id<>OLD.attempt_id AND NEW.dispatch_recorded=0 AND NEW.evidence IS NULL
            AND EXISTS(SELECT 1 FROM mutation_attempt_history h WHERE h.enqueue_order=OLD.enqueue_order
              AND h.attempt_id=OLD.attempt_id AND h.attempt_owner=OLD.attempt_owner
              AND h.attempt_started=OLD.attempt_started AND h.dispatch_recorded=OLD.dispatch_recorded
              AND h.evidence=OLD.evidence))
        BEGIN SELECT RAISE(ABORT,'invalid transition'); END
        """,
    ]
    static let version2Schema = Array(version1Schema.dropLast()) + recoverySchema
    static let inboundSchema = [
        """
        CREATE TABLE inbound_pages (
          scope_id INTEGER NOT NULL REFERENCES scopes(scope_id), epoch TEXT NOT NULL, from_sequence TEXT NOT NULL,
          through_sequence TEXT NOT NULL, high_watermark TEXT NOT NULL,
          response BLOB NOT NULL CHECK(length(response) BETWEEN 1 AND 2097152),
          canonical_data BLOB NOT NULL CHECK(length(canonical_data) BETWEEN 1 AND 2097152),
          encoding_version INTEGER NOT NULL CHECK(encoding_version=1),
          state TEXT NOT NULL CHECK(state IN ('RECEIVED_UNAPPLIED','APPLIED_ACK_PENDING','ACK_IN_FLIGHT','ACK_CONFIRMED','BLOCKED_REBASELINE')),
          created_at REAL NOT NULL, updated_at REAL NOT NULL,
          PRIMARY KEY(scope_id,epoch,from_sequence)
        )
        """,
        """
        CREATE TRIGGER inbound_immutable BEFORE UPDATE OF scope_id,epoch,from_sequence,through_sequence,high_watermark,response,canonical_data,encoding_version,created_at ON inbound_pages
        BEGIN SELECT RAISE(ABORT,'immutable inbound page'); END
        """,
        """
        CREATE TRIGGER inbound_application_gate BEFORE UPDATE OF state ON inbound_pages
        WHEN NEW.state<>OLD.state AND NEW.state<>'BLOCKED_REBASELINE'
        BEGIN SELECT RAISE(ABORT,'projection commit required'); END
        """,
    ]
    static let version3Schema = version2Schema + inboundSchema
    static let version4Schema =
        version2Schema + Array(inboundSchema.dropLast()) + NodeProjectionSQLiteSchema.statements
    private static let schema =
        version2Schema + Array(inboundSchema.dropLast()) + RebaselineSQLiteSchema.projection
        + RebaselineSQLiteSchema.statements

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
        guard [0, 1, 2, 3, 4, Int64(Self.schemaVersion)].contains(version) else {
            throw MutationQueueFailure.unsupportedSchema
        }
        if version > 0 {
            let priorIntegrity = try Self.query(handle, "PRAGMA quick_check") {
                try Self.text($0, 0)
            }
            guard priorIntegrity == ["ok"],
                try Self.query(handle, "PRAGMA foreign_key_check", map: { _ in 1 }).isEmpty
            else {
                throw MutationQueueFailure.corrupt
            }
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
                try Self.execute(handle, "PRAGMA user_version=5")
                try Self.execute(handle, "COMMIT")
            } catch {
                try? Self.execute(handle, "ROLLBACK")
                throw error
            }
        }
        func verifySchema(_ expected: [String]) throws {
            let registered = try Self.query(
                handle, "SELECT sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY sql"
            ) { try Self.text($0, 0) }
            guard registered.sorted() == expected.sorted() else {
                throw MutationQueueFailure.corrupt
            }
        }
        if version == 1 {
            // Verify the exact P035 schema before changing anything; never repair damaged schemas.
            try verifySchema(Self.version1Schema)
            try Self.execute(handle, "BEGIN IMMEDIATE")
            do {
                try Self.execute(handle, "DROP TRIGGER mutation_transition")
                for statement in Self.recoverySchema + Array(Self.inboundSchema.dropLast())
                    + NodeProjectionSQLiteSchema.statements
                {
                    try Self.execute(handle, statement)
                }
                try fault?(.migration)
                try Self.execute(handle, "PRAGMA user_version=4")
                try Self.execute(handle, "COMMIT")
            } catch {
                try? Self.execute(handle, "ROLLBACK")
                throw error
            }
        }
        if version == 2 {
            try verifySchema(Self.version2Schema)
            try Self.execute(handle, "BEGIN IMMEDIATE")
            do {
                for statement in Array(Self.inboundSchema.dropLast())
                    + NodeProjectionSQLiteSchema.statements
                { try Self.execute(handle, statement) }
                try fault?(.migration)
                try Self.execute(handle, "PRAGMA user_version=4")
                try Self.execute(handle, "COMMIT")
            } catch {
                try? Self.execute(handle, "ROLLBACK")
                throw error
            }
        }
        if version == 3 {
            try verifySchema(Self.version3Schema)
            try Self.execute(handle, "BEGIN IMMEDIATE")
            do {
                try Self.execute(handle, "DROP TRIGGER inbound_application_gate")
                for statement in NodeProjectionSQLiteSchema.statements {
                    try Self.execute(handle, statement)
                }
                try fault?(.migration)
                try Self.execute(handle, "PRAGMA user_version=4")
                try Self.execute(handle, "COMMIT")
            } catch {
                try? Self.execute(handle, "ROLLBACK")
                throw error
            }
        }
        if (1...4).contains(version) {
            try verifySchema(Self.version4Schema)
            try Self.execute(handle, "BEGIN IMMEDIATE")
            do {
                for (old, new) in zip(
                    NodeProjectionSQLiteSchema.statements, RebaselineSQLiteSchema.projection)
                where old != new {
                    let name = old.split(separator: " ")[2]
                    try Self.execute(handle, "DROP TRIGGER \(name)")
                    try Self.execute(handle, new)
                }
                for statement in RebaselineSQLiteSchema.statements {
                    try Self.execute(handle, statement)
                }
                try fault?(.migration)
                try Self.execute(handle, "PRAGMA user_version=5")
                try Self.execute(handle, "COMMIT")
            } catch {
                try? Self.execute(handle, "ROLLBACK")
                throw error
            }
        }
        let context = Unmanaged.passUnretained(connection).toOpaque()
        try Self.check(
            sqlite3_create_function_v2(
                handle, "synveil_projection_authorized", 0, SQLITE_UTF8, context,
                { context, _, _ in
                    guard let context, let pointer = sqlite3_user_data(context) else { return }
                    let connection = Unmanaged<MutationSQLiteConnection>.fromOpaque(pointer)
                        .takeUnretainedValue()
                    sqlite3_result_int(context, connection.projectionAuthority)
                }, nil, nil, nil))
        try verifySchema(Self.schema)
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

    func quarantineSessions() throws {
        do {
            try transaction {
                try fault?(.beforeQuarantine)
                // Preserve all evidence while making every dormant authenticated scope inaccessible.
                try execute("UPDATE scopes SET quarantined=1 WHERE quarantined=0")
            }
        } catch {
            // A failed durable logout must not leave projection/ACK authority usable in this process.
            projectionQuarantineFailed = true
            throw error
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
            try requireNoRebaseline(scope)
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
            let projection = try projectionRow(scope)
            let beyondProjection =
                projection.map {
                    $0.epoch != epoch || SyncDecimalValidation.less($0.applied, sequence)
                } ?? false
            if rewind || beyondProjection {
                connection.projectionAuthority = 4
                defer { connection.projectionAuthority = 0 }
                try execute(
                    "UPDATE node_projection_state SET completeness='REBASELINE_REQUIRED' WHERE scope_id=?",
                    [.integer(id)])
                try execute(
                    "UPDATE sync_bases SET status='RECONCILIATION_REQUIRED' WHERE scope_id=?",
                    [.integer(id)])
                return .reconciliationRequired
            }
            let status: SyncBaseStatus =
                incompatible || rewind || old?.status == .reconciliationRequired
                ? .reconciliationRequired : .verified
            if projection != nil {
                connection.projectionAuthority = 3
                defer { connection.projectionAuthority = 0 }
                try execute(
                    "UPDATE node_projection_state SET confirmed_sequence=?,completeness=? WHERE scope_id=?",
                    [
                        .text(sequence),
                        .text(status == .verified ? "PARTIAL" : "REBASELINE_REQUIRED"),
                        .integer(id),
                    ])
            }
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

    /// Bounded activity snapshot. Unresolved work is selected before terminal history so a
    /// terminal-heavy scope cannot hide an older queued operation; returned rows keep enqueue order.
    func activityRecords(scope: ClientMutationScope, limit: Int) throws
        -> [StoredMutationRecord]
    {
        guard (1...MutationQueuePolicy.maximumReadBatch).contains(limit) else {
            throw MutationQueueFailure.invalidLimit
        }
        guard let id = try scopeId(scope) else { return [] }
        let outstanding = try query(
            "SELECT \(Self.columns) FROM mutations WHERE scope_id=? AND state NOT IN ('APPLIED','FAILED_PERMANENT') ORDER BY enqueue_order LIMIT ?",
            [.integer(id), .integer(Int64(limit))]
        ) { try read($0, scope: scope) }
        let remaining = limit - outstanding.count
        guard remaining > 0 else { return outstanding }
        let terminal = try query(
            "SELECT \(Self.columns) FROM mutations WHERE scope_id=? AND state IN ('APPLIED','FAILED_PERMANENT') ORDER BY enqueue_order LIMIT ?",
            [.integer(id), .integer(Int64(remaining))]
        ) { try read($0, scope: scope) }
        return (outstanding + terminal).sorted { $0.order < $1.order }
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
            try requireNoRebaseline(scope)
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
            try requireExecutionOrder(row)
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

    /// Explicit recovery only. Archive uncertainty and acquire fresh authority in one transaction.
    func beginRecoveryAttempt(
        scope: ClientMutationScope, id: String, owner: String, attemptId: String
    )
        throws -> StoredMutationRecord
    {
        try transaction {
            try requireUnquarantinedScope(scope)
            let row = try requireRecord(scope: scope, id: id)
            guard row.state == .outcomeUnknown, let old = row.attempt, let evidence = row.evidence,
                UUID(uuidString: owner) != nil, UUID(uuidString: attemptId) != nil,
                attemptId != old.id
            else { throw MutationQueueFailure.invalidTransition }
            guard let base = try syncBase(scope: scope), base.status == .verified,
                base.epoch == row.epoch, base.sequence == row.sequence
            else { throw MutationQueueFailure.reconciliationRequired }
            try requireExecutionOrder(row)
            guard
                try attemptHistory(scope: scope, id: id).count
                    < MutationQueuePolicy.maximumRecoveryAttempts
            else { throw MutationQueueFailure.recoveryLimit }
            // The old evidence moves to history: total stored BLOB bytes are unchanged here.
            try execute(
                "INSERT INTO mutation_attempt_history VALUES(?,?,?,?,?,?)",
                [
                    .integer(row.order), .text(old.id), .text(old.owner),
                    .real(old.startedAt.timeIntervalSince1970),
                    .integer(old.dispatchRecorded ? 1 : 0),
                    .blob(evidence),
                ])
            try execute(
                "UPDATE mutations SET state='SUBMITTING',attempt_id=?,attempt_owner=?,attempt_started=?,dispatch_recorded=0,evidence=NULL WHERE scope_id=? AND mutation_id=? AND state='OUTCOME_UNKNOWN'",
                [
                    .text(attemptId), .text(owner), .real(Date().timeIntervalSince1970),
                    .integer(try requireScopeId(scope)), .text(id),
                ])
            guard sqlite3_changes(connection.handle) == 1 else {
                throw MutationQueueFailure.invalidTransition
            }
            try fault?(.afterSubmitting)
            return try requireRecord(scope: scope, id: id)
        }
    }

    func attemptHistory(scope: ClientMutationScope, id: String) throws
        -> [MutationHistoricalAttempt]
    {
        let row = try requireRecord(scope: scope, id: id)
        let history = try query(
            "SELECT attempt_id,attempt_owner,attempt_started,dispatch_recorded,evidence FROM mutation_attempt_history WHERE enqueue_order=? ORDER BY rowid LIMIT ?",
            [.integer(row.order), .integer(Int64(MutationQueuePolicy.maximumRecoveryAttempts + 1))]
        ) { s in
            guard UUID(uuidString: try Self.text(s, 0)) != nil,
                UUID(uuidString: try Self.text(s, 1)) != nil,
                [SQLITE_FLOAT, SQLITE_INTEGER].contains(sqlite3_column_type(s, 2)),
                sqlite3_column_double(s, 2).isFinite,
                sqlite3_column_type(s, 3) == SQLITE_INTEGER,
                [0, 1].contains(sqlite3_column_int(s, 3))
            else { throw MutationQueueFailure.malformedRecord }
            return MutationHistoricalAttempt(
                attempt: MutationAttemptMetadata(
                    id: try Self.text(s, 0), owner: try Self.text(s, 1),
                    startedAt: Date(timeIntervalSince1970: sqlite3_column_double(s, 2)),
                    dispatchRecorded: sqlite3_column_int(s, 3) == 1),
                evidence: try Self.blob(s, 4, maximum: ClientMutationPolicy.maximumResponseBytes))
        }
        guard history.count <= MutationQueuePolicy.maximumRecoveryAttempts else {
            throw MutationQueueFailure.malformedRecord
        }
        return history
    }

    /// Conservative scope policy: never overtake any older outstanding row; one SUBMITTING per
    /// scope across connections. Recovery may resolve the earliest unknown, but never pass a conflict.
    private func requireExecutionOrder(_ row: StoredMutationRecord) throws {
        let scopeId = try requireScopeId(row.scope)
        guard
            try scalar(
                "SELECT count(*) FROM mutations WHERE scope_id=? AND state='SUBMITTING' AND enqueue_order<>?",
                [.integer(scopeId), .integer(row.order)]) == 0
        else { throw MutationQueueFailure.concurrentExecution }
        guard
            try scalar(
                "SELECT count(*) FROM mutations WHERE scope_id=? AND enqueue_order<? AND state NOT IN ('APPLIED','FAILED_PERMANENT')",
                [.integer(scopeId), .integer(row.order)]) == 0
        else { throw MutationQueueFailure.dependencyConflict }
        // P035 prevents overlapping outstanding enqueues. Check the reservation again under the
        // write lock, including imported/damaged rows and later unknown/conflict reservations.
        guard
            try scalar(
                "SELECT count(*) FROM dependencies a JOIN dependencies b ON a.node_id=b.node_id JOIN mutations m ON m.enqueue_order=b.enqueue_order WHERE a.enqueue_order=? AND b.enqueue_order<>? AND m.scope_id=? AND m.state NOT IN ('APPLIED','FAILED_PERMANENT')",
                [.integer(row.order), .integer(row.order), .integer(scopeId)]) == 0
        else { throw MutationQueueFailure.dependencyConflict }
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
            try requireExecutionOrder(row)
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

    /// Revalidate every event before entering the synchronous SQLite transaction.
    func stageFeed(_ page: SyncFeedPage, credentialId: String, bridge: any RustBridgeProtocol)
        async throws -> (InboundSyncPageRecord, Bool)
    {
        let verified = try await SyncFeedResponseDecoder(bridge: bridge).decode(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: page.responseBody),
            scope: page.scope, expected: page.start)
        guard verified == page, !page.events.isEmpty else { throw SyncFeedFailure.protocolFailure }
        try Task.checkCancellation()
        return try transaction {
            try requireFeedScope(page.scope, credentialId: credentialId)
            try requireNoRebaseline(page.scope)
            guard let base = try syncBase(scope: page.scope), base.status == .verified,
                base.epoch == page.start.epoch.rawValue,
                base.sequence == page.start.sequence.rawValue
            else { throw SyncFeedFailure.checkpointConflict }
            let id = try requireScopeId(page.scope)
            if let old = try rawInbound(scope: page.scope, position: page.start) {
                guard old.canonical == page.canonicalData, old.through == page.through.rawValue,
                    old.high == page.highWatermark.rawValue
                else { throw SyncFeedFailure.checkpointConflict }
                guard old.state != .blockedRebaseline else {
                    throw SyncFeedFailure.rebaselineRequired
                }
                return (
                    InboundSyncPageRecord(
                        page: page, state: old.state, createdAt: old.created,
                        updatedAt: old.updated, encodingVersion: 1), false
                )
            }
            guard
                try scalar("SELECT count(*) FROM inbound_pages WHERE scope_id=?", [.integer(id)])
                    < Int64(SyncFeedPolicy.maximumPagesPerScope),
                try scalar("SELECT count(*) FROM inbound_pages") < Int64(maximumRecords)
            else { throw MutationQueueFailure.capacity }
            try checkCapacity(additionalBytes: page.responseBody.count + page.canonicalData.count)
            let now = Date()
            try fault?(.beforeInsert)
            try execute(
                "INSERT INTO inbound_pages(scope_id,epoch,from_sequence,through_sequence,high_watermark,response,canonical_data,encoding_version,state,created_at,updated_at) VALUES(?,?,?,?,?,?,?,1,'RECEIVED_UNAPPLIED',?,?)",
                [
                    .integer(id), .text(page.start.epoch.rawValue),
                    .text(page.start.sequence.rawValue),
                    .text(page.through.rawValue), .text(page.highWatermark.rawValue),
                    .blob(page.responseBody),
                    .blob(page.canonicalData), .real(now.timeIntervalSince1970),
                    .real(now.timeIntervalSince1970),
                ])
            try fault?(.afterInsert)
            return (
                InboundSyncPageRecord(
                    page: page, state: .receivedUnapplied, createdAt: now,
                    updatedAt: now, encodingVersion: 1), true
            )
        }
    }

    private struct RawInbound: Equatable {
        let response: Data
        let canonical: Data
        let through: String
        let high: String
        let state: InboundSyncPageState
        let created: Date
        let updated: Date
    }

    private func rawInbound(scope: ClientMutationScope, position: SyncJournalPosition) throws
        -> RawInbound?
    {
        guard let id = try scopeId(scope) else { return nil }
        return try query(
            "SELECT response,canonical_data,through_sequence,high_watermark,state,created_at,updated_at,encoding_version FROM inbound_pages WHERE scope_id=? AND epoch=? AND from_sequence=?",
            [.integer(id), .text(position.epoch.rawValue), .text(position.sequence.rawValue)]
        ) { s in
            guard let state = InboundSyncPageState(rawValue: try Self.text(s, 4)),
                sqlite3_column_int(s, 7) == 1,
                [SQLITE_FLOAT, SQLITE_INTEGER].contains(sqlite3_column_type(s, 5)),
                [SQLITE_FLOAT, SQLITE_INTEGER].contains(sqlite3_column_type(s, 6))
            else { throw MutationQueueFailure.malformedRecord }
            return RawInbound(
                response: try Self.blob(s, 0, maximum: SyncFeedPolicy.maximumResponseBytes),
                canonical: try Self.blob(s, 1, maximum: SyncFeedPolicy.maximumResponseBytes),
                through: try Self.text(s, 2), high: try Self.text(s, 3), state: state,
                created: Date(timeIntervalSince1970: sqlite3_column_double(s, 5)),
                updated: Date(timeIntervalSince1970: sqlite3_column_double(s, 6)))
        }.first
    }

    func inboundPage(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String,
        bridge: any RustBridgeProtocol
    ) async throws -> InboundSyncPageRecord? {
        try requireFeedScope(scope, credentialId: credentialId)
        guard let raw = try rawInbound(scope: scope, position: position) else { return nil }
        let page = try await SyncFeedResponseDecoder(bridge: bridge).decode(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"], body: raw.response),
            scope: scope, expected: position)
        guard page.canonicalData == raw.canonical, page.through.rawValue == raw.through,
            page.highWatermark.rawValue == raw.high, !page.events.isEmpty,
            raw.created <= raw.updated,
            InboundSyncPageState(rawValue: raw.state.rawValue) != nil
        else { throw MutationQueueFailure.malformedRecord }
        try requireFeedScope(scope, credentialId: credentialId)
        // Decoding suspends on Rust validation. A concurrent rebaseline may have blocked this row.
        guard let current = try rawInbound(scope: scope, position: position) else {
            throw MutationQueueFailure.reconciliationRequired
        }
        if current != raw {
            // Concurrent atomic application may legitimately replace only staging state/timestamp.
            guard raw.state == .receivedUnapplied,
                [.appliedAckPending, .ackInFlight, .ackConfirmed].contains(current.state),
                current.response == raw.response, current.canonical == raw.canonical,
                current.through == raw.through,
                current.high == raw.high, current.created == raw.created
            else { throw MutationQueueFailure.reconciliationRequired }
        }
        return InboundSyncPageRecord(
            page: page, state: current.state, createdAt: current.created,
            updatedAt: current.updated, encodingVersion: 1)
    }

    /// Conservative global ownership across coordinators, connections and processes. The private
    /// zero-byte advisory lock is released on process death. SQLite still authorizes every write.
    func claimInboundRun(scope: ClientMutationScope, credentialId: String, owner: UUID) throws {
        try requireFeedScope(scope, credentialId: credentialId)
        try requireNoRebaseline(scope)
        try connection.acquireInboundLock(path: url.path + ".inbound.lock", owner: owner)
    }

    func finishInboundRun(owner: UUID) { connection.releaseInboundLock(owner: owner) }

    /// Bounded deterministic selection across epochs; an old unresolved epoch must block a run.
    /// Canonical unsigned decimal order uses length then text, never floating point or SQL casts.
    func oldestUnresolvedInbound(
        scope: ClientMutationScope, credentialId: String, bridge: any RustBridgeProtocol
    ) async throws -> InboundSyncPageRecord? {
        try requireFeedScope(scope, credentialId: credentialId)
        let positions = try query(
            "SELECT epoch,from_sequence FROM inbound_pages WHERE scope_id=? AND state<>'ACK_CONFIRMED' AND NOT EXISTS(SELECT 1 FROM rebaseline_active a JOIN rebaseline_sessions r ON r.bootstrap_id=a.bootstrap_id WHERE a.scope_id=inbound_pages.scope_id AND (r.epoch<>inbound_pages.epoch OR length(inbound_pages.from_sequence)<length(r.cut) OR (length(inbound_pages.from_sequence)=length(r.cut) AND inbound_pages.from_sequence<r.cut))) ORDER BY length(epoch),epoch,length(from_sequence),from_sequence LIMIT 1",
            [.integer(try requireScopeId(scope))]
        ) { s in
            SyncJournalPosition(
                epoch: try SyncDecimalValidation.validate(Self.text(s, 0), nonzero: true),
                sequence: try SyncDecimalValidation.validate(Self.text(s, 1)))
        }
        guard let position = positions.first else { return nil }
        let record = try await inboundPage(
            scope: scope, position: position, credentialId: credentialId, bridge: bridge)
        try requireFeedScope(scope, credentialId: credentialId)
        return record
    }

    /// Only a bounded count leaves storage; signed tokens/attempt identities never enter UI state.
    func inboundAckAttemptCount(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String
    ) throws -> Int {
        try requireFeedScope(scope, credentialId: credentialId)
        let count = try scalar(
            "SELECT count(*) FROM sync_ack_attempts WHERE scope_id=? AND epoch=? AND from_sequence=?",
            [
                .integer(try requireScopeId(scope)), .text(position.epoch.rawValue),
                .text(position.sequence.rawValue),
            ])
        guard (0...Int64(MutationQueuePolicy.maximumRecoveryAttempts)).contains(count) else {
            throw MutationQueueFailure.malformedRecord
        }
        return Int(count)
    }

    /// Validate latest confirmed evidence without loading an unbounded page history.
    func latestConfirmedInbound(
        scope: ClientMutationScope, credentialId: String, bridge: any RustBridgeProtocol
    ) async throws -> InboundSyncPageRecord? {
        try requireFeedScope(scope, credentialId: credentialId)
        let positions = try query(
            "SELECT epoch,from_sequence FROM inbound_pages WHERE scope_id=? AND state='ACK_CONFIRMED' AND NOT EXISTS(SELECT 1 FROM rebaseline_active a JOIN rebaseline_sessions r ON r.bootstrap_id=a.bootstrap_id WHERE a.scope_id=inbound_pages.scope_id AND (r.epoch<>inbound_pages.epoch OR length(inbound_pages.through_sequence)<length(r.cut) OR (length(inbound_pages.through_sequence)=length(r.cut) AND inbound_pages.through_sequence<=r.cut))) ORDER BY length(epoch) DESC,epoch DESC,length(from_sequence) DESC,from_sequence DESC LIMIT 1",
            [.integer(try requireScopeId(scope))]
        ) { s in
            SyncJournalPosition(
                epoch: try SyncDecimalValidation.validate(Self.text(s, 0), nonzero: true),
                sequence: try SyncDecimalValidation.validate(Self.text(s, 1)))
        }
        guard let position = positions.first,
            let record = try await inboundPage(
                scope: scope, position: position, credentialId: credentialId, bridge: bridge)
        else { return nil }
        try requireFeedScope(scope, credentialId: credentialId)
        guard record.state == .ackConfirmed else { throw SyncFeedFailure.checkpointConflict }
        try requireProjectionCommit(record.page)
        guard
            try scalar(
                "SELECT count(*) FROM sync_ack_attempts WHERE scope_id=? AND epoch=? AND from_sequence=? AND completed=1 AND dispatched=1 AND checkpoint IS NOT NULL",
                [
                    .integer(try requireScopeId(scope)), .text(position.epoch.rawValue),
                    .text(position.sequence.rawValue),
                ]) > 0
        else { throw SyncFeedFailure.checkpointConflict }
        return record
    }

    func blockInbound(scope: ClientMutationScope, credentialId: String) throws {
        try transaction {
            try requireFeedScope(scope, credentialId: credentialId)
            let id = try requireScopeId(scope)
            try execute(
                "UPDATE inbound_pages SET state='BLOCKED_REBASELINE',updated_at=? WHERE scope_id=?",
                [.real(Date().timeIntervalSince1970), .integer(id)])
            connection.projectionAuthority = 4
            defer { connection.projectionAuthority = 0 }
            try execute(
                "UPDATE node_projection_state SET completeness='REBASELINE_REQUIRED' WHERE scope_id=?",
                [.integer(id)])
            try execute(
                "UPDATE sync_bases SET status='RECONCILIATION_REQUIRED' WHERE scope_id=?",
                [.integer(id)])
        }
    }

    private func requireFeedScope(_ scope: ClientMutationScope, credentialId: String) throws {
        guard !projectionQuarantineFailed else { throw MutationQueueFailure.unavailable }
        try requireUnquarantinedScope(scope)
        guard
            try scalar(
                "SELECT count(*) FROM scopes WHERE scope_id=? AND credential_id=?",
                [.integer(try requireScopeId(scope)), .text(credentialId)]) == 1
        else { throw MutationQueueFailure.scopeMismatch }
    }

    private func requireUnquarantinedScope(_ scope: ClientMutationScope) throws {
        let id = try requireScopeId(scope)
        guard try scalar("SELECT quarantined FROM scopes WHERE scope_id=?", [.integer(id)]) == 0
        else {
            throw MutationQueueFailure.scopeMismatch
        }
    }

    private func requireBase(_ mutation: PreparedClientMutation) throws {
        try requireNoRebaseline(mutation.base.scope)
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
            "SELECT coalesce((SELECT sum(length(request)+length(payload)+coalesce(length(evidence),0)) FROM mutations),0)+coalesce((SELECT sum(length(response)) FROM sync_bases),0)+coalesce((SELECT sum(length(evidence)) FROM mutation_attempt_history),0)+coalesce((SELECT sum(length(response)+length(canonical_data)) FROM inbound_pages),0)+coalesce((SELECT sum(coalesce(length(metadata),0)+length(node_id)+length(revision)+length(sequence)+coalesce(length(parent_id),0)+128) FROM cached_nodes),0)+coalesce((SELECT sum(length(metadata)+64) FROM cached_libraries),0)+coalesce((SELECT sum(length(canonical_data)+128) FROM projection_commits),0)+coalesce((SELECT count(*)*256 FROM projection_events),0)+coalesce((SELECT sum(coalesce(length(checkpoint),0)+256) FROM sync_ack_attempts),0)+coalesce((SELECT count(*)*256 FROM node_projection_state),0)+coalesce((SELECT sum(length(response)+length(canonical)+512) FROM rebaseline_pages),0)+coalesce((SELECT sum(length(metadata)+256) FROM rebaseline_nodes),0)+coalesce((SELECT sum(length(bootstrap)+length(library)+coalesce(length(completion),0)+1024) FROM rebaseline_sessions),0)+coalesce((SELECT sum(coalesce(length(completion),0)+512) FROM rebaseline_completion_attempts),0)"
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
            do {
                try fault?(.beforeRollback)
                try Self.execute(connection.handle, "ROLLBACK")
            } catch {
                // A failed cleanup must never let a later read expose uncommitted working rows.
                poisoned = true
            }
            throw error
        }
    }

    private enum SQLValue { case text(String), blob(Data), integer(Int64), real(Double), null }
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
                case .null: try check(sqlite3_bind_null(statement, position))
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
        guard !poisoned else { throw MutationQueueFailure.io }
        try Self.execute(connection.handle, sql, values)
    }
    private func query<T>(
        _ sql: String, _ values: [SQLValue] = [], map: (OpaquePointer) throws -> T
    ) throws -> [T] {
        guard !poisoned else { throw MutationQueueFailure.io }
        return try Self.query(connection.handle, sql, values, map: map)
    }
    private func scalar(_ sql: String, _ values: [SQLValue] = []) throws -> Int64 {
        guard !poisoned else { throw MutationQueueFailure.io }
        return try Self.scalar(connection.handle, sql, values)
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
                attributes: requestedAttributes(directory: true))
            try rejectSymbolicLink(url)
            if !FileManager.default.fileExists(atPath: url.path) {
                guard
                    FileManager.default.createFile(
                        atPath: url.path, contents: Data(),
                        attributes: requestedAttributes(directory: false)
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
                requestedAttributes(directory: true), ofItemAtPath: parent.path)
            try verifyDeviceProtection(parent.path)
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
                        requestedAttributes(directory: false), ofItemAtPath: path)
                    try verifyDeviceProtection(path)
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
    static func requestedAttributes(directory: Bool) -> [FileAttributeKey: Any] {
        var result: [FileAttributeKey: Any] = [.posixPermissions: directory ? 0o700 : 0o600]
        #if os(iOS)
            result[.protectionKey] = FileProtectionType.completeUntilFirstUserAuthentication
        #endif
        return result
    }
    #if os(iOS)
        /// Foundation attribute dictionaries can expose an NSString/String raw value, rather than
        /// Swift's FileProtectionType wrapper. Normalize both representations without losing checks.
        static func protectionName(_ attributes: [FileAttributeKey: Any]) -> String? {
            if let value = attributes[.protectionKey] as? FileProtectionType {
                return value.rawValue
            }
            return attributes[.protectionKey] as? String
        }
    #endif

    private static func verifyDeviceProtection(_ path: String) throws {
        #if os(iOS) && !targetEnvironment(simulator)
            let actual = try FileManager.default.attributesOfItem(atPath: path)
            guard
                protectionName(actual)
                    == FileProtectionType.completeUntilFirstUserAuthentication.rawValue
            else {
                throw MutationQueueFailure.io
            }
        #endif
    }

    private static func rejectSymbolicLink(_ url: URL) throws {
        if let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
            attributes[.type] as? FileAttributeType == .typeSymbolicLink
        {
            throw MutationQueueFailure.databaseOpen
        }
    }
}

extension MutationQueueSQLiteStore {
    private func activeSnapshotId(_ scope: ClientMutationScope, epoch: String) throws -> String? {
        try query(
            "SELECT r.bootstrap_id FROM rebaseline_active a JOIN rebaseline_sessions r ON r.bootstrap_id=a.bootstrap_id WHERE a.scope_id=? AND r.scope_id=a.scope_id AND r.epoch=? AND r.stage='ACTIVE_COMPLETE' AND r.prepared_id IS NOT NULL AND r.completion IS NOT NULL AND r.terminal_token IS NOT NULL",
            [.integer(try requireScopeId(scope)), .text(epoch)]
        ) { try Self.text($0, 0) }.first
    }
    private func activeSnapshotCut(_ scope: ClientMutationScope) throws -> SyncJournalPosition? {
        try query(
            "SELECT r.epoch,r.cut FROM rebaseline_active a JOIN rebaseline_sessions r ON r.bootstrap_id=a.bootstrap_id WHERE a.scope_id=? AND r.stage='ACTIVE_COMPLETE'",
            [.integer(try requireScopeId(scope))]
        ) {
            SyncJournalPosition(
                epoch: try SyncDecimalValidation.validate(Self.text($0, 0), nonzero: true),
                sequence: try SyncDecimalValidation.validate(Self.text($0, 1)))
        }.first
    }
    private func snapshotNode(
        scope: ClientMutationScope, epoch: String, nodeId: NodeId, bridge: any RustBridgeProtocol
    ) async throws -> RebaselineSnapshotNode? {
        guard let id = try activeSnapshotId(scope, epoch: epoch) else { return nil }
        let bytes = try query(
            "SELECT metadata FROM rebaseline_nodes WHERE bootstrap_id=? AND node_id=?",
            [.text(id), .text(nodeId.rawValue)]
        ) { try Self.blob($0, 0, maximum: RebaselinePolicy.maximumNodeBytes) }.first
        guard let bytes else { return nil }
        let node = try await JSONDecoder().decode(RebaselineNodeDTO.self, from: bytes).validated(
            bridge: bridge)
        guard node.id == nodeId, try activeSnapshotId(scope, epoch: epoch) == id else {
            throw SyncProjectionFailure.stalePage
        }
        return node
    }
    private struct ProjectionRow: Equatable {
        let epoch: String, anchor: String, applied: String, confirmed: String
        let completeness: NodeProjectionCompleteness
    }
    private struct CacheRow: Equatable {
        let id: String, revision: String, sequence: String, parent: String?
        let lifecycle: CachedNodeLifecycle, provenance: CachedNodeProvenance
        let metadata: Data?
    }
    private func projectionRow(_ scope: ClientMutationScope) throws -> ProjectionRow? {
        try query(
            "SELECT epoch,anchor_sequence,applied_sequence,confirmed_sequence,completeness,encoding_version FROM node_projection_state WHERE scope_id=?",
            [.integer(try requireScopeId(scope))]
        ) { s in
            guard let completeness = NodeProjectionCompleteness(rawValue: try Self.text(s, 4)),
                sqlite3_column_int(s, 5) == 1
            else { throw MutationQueueFailure.malformedRecord }
            return ProjectionRow(
                epoch: try Self.text(s, 0), anchor: try Self.text(s, 1),
                applied: try Self.text(s, 2), confirmed: try Self.text(s, 3),
                completeness: completeness)
        }.first
    }
    private func cacheRows(
        _ scope: ClientMutationScope, epoch: String, node: NodeId? = nil, parent: NodeId? = nil,
        limit: Int = 1
    ) throws -> [CacheRow] {
        var values: [SQLValue] = [.integer(try requireScopeId(scope)), .text(epoch)]
        var predicate = "scope_id=? AND epoch=?"
        if let node {
            predicate += " AND node_id=?"
            values.append(.text(node.rawValue))
        }
        if let parent {
            predicate += " AND parent_id=? AND lifecycle='ACTIVE' AND provenance='CANONICAL'"
            values.append(.text(parent.rawValue))
        }
        values.append(.integer(Int64(limit)))
        return try query(
            "SELECT node_id,revision,sequence,parent_id,lifecycle,provenance,metadata,encoding_version FROM cached_nodes WHERE \(predicate) ORDER BY node_id LIMIT ?",
            values
        ) { s in
            guard let lifecycle = CachedNodeLifecycle(rawValue: try Self.text(s, 4)),
                let provenance = CachedNodeProvenance(rawValue: try Self.text(s, 5)),
                sqlite3_column_int(s, 7) == 1
            else { throw MutationQueueFailure.malformedRecord }
            return CacheRow(
                id: try Self.text(s, 0), revision: try Self.text(s, 1),
                sequence: try Self.text(s, 2),
                parent: sqlite3_column_type(s, 3) == SQLITE_NULL ? nil : try Self.text(s, 3),
                lifecycle: lifecycle, provenance: provenance,
                metadata: sqlite3_column_type(s, 6) == SQLITE_NULL
                    ? nil : try Self.blob(s, 6, maximum: NodeProjectionPolicy.maximumMetadataBytes))
        }
    }
    private func libraryBytes(_ scope: ClientMutationScope, epoch: String) throws -> Data? {
        try query(
            "SELECT metadata,encoding_version FROM cached_libraries WHERE scope_id=? AND epoch=?",
            [.integer(try requireScopeId(scope)), .text(epoch)]
        ) { s in
            guard sqlite3_column_int(s, 1) == 1 else { throw MutationQueueFailure.malformedRecord }
            return try Self.blob(s, 0, maximum: NodeProjectionPolicy.maximumMetadataBytes)
        }.first
    }
    private func currentEpoch(_ scope: ClientMutationScope) throws -> String {
        guard let base = try syncBase(scope: scope) else {
            throw MutationQueueFailure.syncBaseUnavailable
        }
        _ = try SyncDecimalValidation.validate(base.epoch, nonzero: true)
        return base.epoch
    }
    private func decodeCache(
        _ raw: CacheRow, scope: ClientMutationScope, epoch: String, bridge: any RustBridgeProtocol
    ) async throws -> CachedNodeRecord {
        let id = try await NodeId.validated(raw.id, using: bridge)
        var node: Node?
        if let bytes = raw.metadata {
            node = try await JSONDecoder().decode(NodeProjectionMetadata.self, from: bytes)
                .validated(using: bridge)
        }
        let revision = try SyncDecimalValidation.validate(raw.revision, nonzero: true)
        let position = SyncJournalPosition(
            epoch: try SyncDecimalValidation.validate(epoch, nonzero: true),
            sequence: try SyncDecimalValidation.validate(raw.sequence))
        if let node {
            guard node.id == id, node.libraryId == scope.libraryId,
                !SyncDecimalValidation.less(raw.revision, node.revision.rawValue)
            else { throw MutationQueueFailure.malformedRecord }
            if raw.provenance == .canonical {
                guard node.revision.rawValue == raw.revision, node.parentId?.rawValue == raw.parent,
                    node.state == .active, raw.lifecycle == .active
                else { throw MutationQueueFailure.malformedRecord }
            }
        } else if raw.provenance != .event {
            throw MutationQueueFailure.malformedRecord
        }
        if raw.lifecycle == .purged, node != nil || raw.parent != nil || raw.provenance != .event {
            throw MutationQueueFailure.malformedRecord
        }
        return CachedNodeRecord(
            scope: scope, position: position, id: id, revision: revision, lifecycle: raw.lifecycle,
            provenance: raw.provenance, metadata: node)
    }
    func cachedProjectionState(
        scope: ClientMutationScope, credentialId: String, bridge: any RustBridgeProtocol
    ) async throws -> NodeProjectionState {
        try requireFeedScope(scope, credentialId: credentialId)
        let row = try projectionRow(scope)
        let base = try syncBase(scope: scope)
        let epoch = try currentEpoch(scope)
        guard row == nil || row?.epoch == epoch else {
            throw SyncProjectionFailure.reconciliationRequired
        }
        let bytes = try libraryBytes(scope, epoch: epoch)
        let library: CachedLibraryRecord?
        if let bytes {
            let value = try await JSONDecoder().decode(LibraryProjectionMetadata.self, from: bytes)
                .validated(bridge: bridge)
            guard value.id == scope.libraryId else { throw SyncProjectionFailure.scopeMismatch }
            library = CachedLibraryRecord(
                scope: scope, epoch: try SyncDecimalValidation.validate(epoch, nonzero: true),
                library: value)
        } else {
            library = nil
        }
        let state: NodeProjectionState
        func position(_ value: String) throws -> SyncJournalPosition {
            SyncJournalPosition(
                epoch: try SyncDecimalValidation.validate(epoch, nonzero: true),
                sequence: try SyncDecimalValidation.validate(value))
        }
        if let row {
            let snapshotId = try activeSnapshotId(scope, epoch: epoch)
            guard !SyncDecimalValidation.less(row.applied, row.anchor),
                !SyncDecimalValidation.less(row.applied, row.confirmed),
                row.completeness != .complete || snapshotId != nil
            else { throw MutationQueueFailure.malformedRecord }
            state = NodeProjectionState(
                completeness: base?.status == .verified ? row.completeness : .rebaselineRequired,
                incrementalAnchor: try position(row.anchor),
                locallyApplied: try position(row.applied),
                serverConfirmed: try position(row.confirmed), library: library)
        } else {
            state = NodeProjectionState(
                completeness: base?.status == .verified ? .uninitialized : .rebaselineRequired,
                incrementalAnchor: nil, locallyApplied: nil,
                serverConfirmed: try base.map { try position($0.sequence) }, library: library)
        }
        try requireFeedScope(scope, credentialId: credentialId)
        guard try projectionRow(scope) == row,
            try syncBase(scope: scope)?.responseBody == base?.responseBody,
            try libraryBytes(scope, epoch: epoch) == bytes
        else { throw SyncProjectionFailure.stalePage }
        return state
    }
    func cachedNode(
        scope: ClientMutationScope, nodeId: NodeId, credentialId: String,
        bridge: any RustBridgeProtocol
    ) async throws -> CachedNodeRecord? {
        try requireFeedScope(scope, credentialId: credentialId)
        let epoch = try currentEpoch(scope)
        guard try projectionRow(scope).map({ $0.epoch == epoch }) ?? true else {
            throw SyncProjectionFailure.reconciliationRequired
        }
        let raw = try cacheRows(scope, epoch: epoch, node: nodeId).first
        let result: CachedNodeRecord?
        if let raw {
            result = try await decodeCache(raw, scope: scope, epoch: epoch, bridge: bridge)
        } else {
            if let snapshot = try await snapshotNode(
                scope: scope, epoch: epoch, nodeId: nodeId, bridge: bridge),
                let cut = try activeSnapshotCut(scope)
            {
                result = CachedNodeRecord(
                    scope: scope, position: cut, id: snapshot.id,
                    revision: try SyncDecimalValidation.validate(
                        snapshot.revision.rawValue, nonzero: true),
                    lifecycle: snapshot.state == .active ? .active : .trashed,
                    provenance: .snapshotManifest,
                    metadata: nil, snapshotMetadata: snapshot)
            } else {
                result = nil
            }
        }
        try requireFeedScope(scope, credentialId: credentialId)
        guard try currentEpoch(scope) == epoch,
            try cacheRows(scope, epoch: epoch, node: nodeId).first == raw
        else { throw SyncProjectionFailure.stalePage }
        return result
    }
    func cachedChildren(
        scope: ClientMutationScope, parentId: NodeId, limit: Int, credentialId: String,
        bridge: any RustBridgeProtocol
    ) async throws -> CachedChildren {
        guard (1...NodeProjectionPolicy.maximumChildren).contains(limit) else {
            throw SyncProjectionFailure.storageCapacity
        }
        try requireFeedScope(scope, credentialId: credentialId)
        let epoch = try currentEpoch(scope)
        let snapshot = try projectionRow(scope)
        let parent = try cacheRows(scope, epoch: epoch, node: parentId).first
        let rows = try cacheRows(scope, epoch: epoch, parent: parentId, limit: limit + 1)
        var nodes: [Node] = []
        var snapshots: [RebaselineSnapshotNode] = []
        // An inactive known ancestor cannot become an active offline directory through its children.
        let visible = try activeAncestry(scope: scope, epoch: epoch, parent: parentId)
        if visible {
            for row in rows.prefix(limit) {
                guard
                    let node = try await decodeCache(
                        row, scope: scope, epoch: epoch, bridge: bridge
                    ).completeNode
                else { throw MutationQueueFailure.malformedRecord }
                nodes.append(node)
            }
            if let bootstrap = try activeSnapshotId(scope, epoch: epoch) {
                let saved = try query(
                    "SELECT n.metadata FROM rebaseline_nodes n WHERE n.bootstrap_id=? AND n.parent_id=? AND n.lifecycle='ACTIVE' AND NOT EXISTS(SELECT 1 FROM cached_nodes c WHERE c.scope_id=? AND c.epoch=? AND c.node_id=n.node_id) ORDER BY n.node_id LIMIT ?",
                    [
                        .text(bootstrap), .text(parentId.rawValue),
                        .integer(try requireScopeId(scope)), .text(epoch),
                        .integer(Int64(limit + 1)),
                    ]
                ) { try Self.blob($0, 0, maximum: RebaselinePolicy.maximumNodeBytes) }
                for bytes in saved {
                    snapshots.append(
                        try await JSONDecoder().decode(RebaselineNodeDTO.self, from: bytes)
                            .validated(bridge: bridge))
                }
            }
        }
        let identities = (nodes.map { $0.id.rawValue } + snapshots.map { $0.id.rawValue }).sorted()
        let included = Set(identities.prefix(limit))
        nodes = nodes.filter { included.contains($0.id.rawValue) }
        snapshots = snapshots.filter { included.contains($0.id.rawValue) }
        let state = try await cachedProjectionState(
            scope: scope, credentialId: credentialId, bridge: bridge)
        try requireFeedScope(scope, credentialId: credentialId)
        guard try projectionRow(scope) == snapshot,
            try cacheRows(scope, epoch: epoch, node: parentId).first == parent,
            try cacheRows(scope, epoch: epoch, parent: parentId, limit: limit + 1) == rows
        else { throw SyncProjectionFailure.stalePage }
        let knowledge: CachedDirectoryKnowledge =
            state.completeness == .rebaselineRequired || !visible
            ? .staleKnown
            : (state.completeness == .complete
                ? .complete : (parent == nil && rows.isEmpty ? .missing : .partial))
        return CachedChildren(
            nodes: nodes, knowledge: knowledge,
            hasMore: visible && (rows.count > limit || identities.count > limit),
            projection: state, snapshotNodes: snapshots)
    }
    private func activeAncestry(scope: ClientMutationScope, epoch: String, parent: NodeId) throws
        -> Bool
    {
        var next: String? = parent.rawValue
        var seen: Set<String> = []
        for _ in 0..<128 {
            guard let id = next else { return true }
            guard seen.insert(id).inserted else { throw SyncProjectionFailure.invalidParent }
            let values: [SQLValue] = [.integer(try requireScopeId(scope)), .text(epoch), .text(id)]
            let row = try query(
                "SELECT parent_id,lifecycle FROM cached_nodes WHERE scope_id=? AND epoch=? AND node_id=?",
                values
            ) { s in
                (
                    sqlite3_column_type(s, 0) == SQLITE_NULL ? nil : try Self.text(s, 0),
                    try Self.text(s, 1)
                )
            }.first
            guard let row else {
                if let bootstrap = try activeSnapshotId(scope, epoch: epoch) {
                    let snapshot = try query(
                        "SELECT parent_id,lifecycle FROM rebaseline_nodes WHERE bootstrap_id=? AND node_id=?",
                        [.text(bootstrap), .text(id)]
                    ) { s in
                        (
                            sqlite3_column_type(s, 0) == SQLITE_NULL ? nil : try Self.text(s, 0),
                            try Self.text(s, 1)
                        )
                    }.first
                    guard let snapshot else { throw SyncProjectionFailure.invalidParent }
                    guard snapshot.1 == "ACTIVE" else { return false }
                    next = snapshot.0
                    continue
                }
                return true
            }
            guard row.1 == "ACTIVE" else { return false }
            next = row.0
        }
        throw SyncProjectionFailure.invalidParent
    }

    func rememberLibrary(
        _ library: Library, scope: ClientMutationScope, credentialId: String,
        bridge: any RustBridgeProtocol
    ) async throws {
        let metadata = LibraryProjectionMetadata(library)
        _ = try await metadata.validated(bridge: bridge)
        guard library.id == scope.libraryId else { throw SyncProjectionFailure.scopeMismatch }
        let bytes = try JSONEncoder().encode(metadata)
        guard bytes.count <= NodeProjectionPolicy.maximumMetadataBytes else {
            throw SyncProjectionFailure.storageCapacity
        }
        try Task.checkCancellation()
        try transaction {
            try requireFeedScope(scope, credentialId: credentialId)
            let epoch = try currentEpoch(scope)
            let old = try libraryBytes(scope, epoch: epoch)
            if let old {
                let prior = try JSONDecoder().decode(LibraryProjectionMetadata.self, from: old)
                guard !SyncDecimalValidation.less(library.revision.rawValue, prior.revision),
                    library.revision.rawValue != prior.revision || old == bytes
                else { throw SyncProjectionFailure.revisionRegression }
            }
            try checkCapacity(additionalBytes: bytes.count - (old?.count ?? 0) + 64)
            connection.projectionAuthority = 1
            defer { connection.projectionAuthority = 0 }
            try execute(
                "INSERT INTO cached_libraries(scope_id,epoch,metadata,encoding_version) VALUES(?,?,?,1) ON CONFLICT(scope_id,epoch) DO UPDATE SET metadata=excluded.metadata",
                [.integer(try requireScopeId(scope)), .text(epoch), .blob(bytes)])
        }
    }

    func projectionFault(_ point: MutationQueueFaultPoint) throws { try fault?(point) }

    func applyProjection(
        _ plan: SyncNodeMaterializationPlan, credentialId: String, bridge: any RustBridgeProtocol
    ) async throws -> Bool {
        let page = plan.page
        // Revalidate immutable durable page outside the non-suspending write transaction.
        guard
            let record = try await inboundPage(
                scope: page.scope, position: page.start, credentialId: credentialId, bridge: bridge),
            record.page == page
        else { throw SyncProjectionFailure.stalePage }
        let bytes = plan.nodes.values.reduce(0, { $0 + $1.bytes.count })
        guard plan.nodes.count + plan.parents.count <= NodeProjectionPolicy.maximumNodesPerPage,
            bytes <= NodeProjectionPolicy.maximumPlanBytes
        else { throw SyncProjectionFailure.storageCapacity }
        try Task.checkCancellation()
        var committed = false
        do {
            let existing = try transaction {
                try requireFeedScope(page.scope, credentialId: credentialId)
                try requireNoRebaseline(page.scope)
                guard let raw = try rawInbound(scope: page.scope, position: page.start),
                    raw.canonical == page.canonicalData
                else { throw SyncProjectionFailure.stalePage }
                if [.appliedAckPending, .ackInFlight, .ackConfirmed].contains(raw.state) {
                    try requireProjectionCommit(page)
                    return true
                }
                guard raw.state == .receivedUnapplied, let evidence = page.evidence,
                    let base = try syncBase(scope: page.scope), base.status == .verified,
                    base.epoch == page.start.epoch.rawValue
                else { throw SyncProjectionFailure.reconciliationRequired }
                let previous = try projectionRow(page.scope)
                guard previous == nil || previous?.epoch == base.epoch,
                    previous?.completeness != .rebaselineRequired,
                    (previous?.applied ?? base.sequence) == page.start.sequence.rawValue,
                    base.sequence == (previous?.confirmed ?? base.sequence)
                else { throw SyncProjectionFailure.stalePage }
                let scopeId = try requireScopeId(page.scope)
                let epoch = page.start.epoch.rawValue
                guard
                    try scalar("SELECT count(*) FROM cached_nodes")
                        + Int64(plan.nodes.count + page.events.count)
                        <= Int64(NodeProjectionPolicy.maximumNodes),
                    try scalar("SELECT count(*) FROM projection_events") + Int64(page.events.count)
                        <= Int64(NodeProjectionPolicy.maximumEvents)
                else { throw SyncProjectionFailure.storageCapacity }
                try checkCapacity(
                    additionalBytes: bytes + page.canonicalData.count + page.events.count * 512
                        + 512)
                connection.projectionAuthority = 1
                defer { connection.projectionAuthority = 0 }
                if previous == nil {
                    try execute(
                        "INSERT INTO node_projection_state VALUES(?,?,?,?,?,'PARTIAL',1)",
                        [
                            .integer(scopeId), .text(epoch), .text(base.sequence),
                            .text(base.sequence), .text(base.sequence),
                        ])
                }
                let commitId = UUID().uuidString
                try execute(
                    "INSERT INTO projection_commits VALUES(?,?,?,?,?,?,?,?)",
                    [
                        .integer(scopeId), .text(epoch), .text(evidence.from.rawValue),
                        .text(evidence.through.rawValue), .text(commitId),
                        .integer(Int64(page.events.count)), .blob(page.canonicalData),
                        .real(Date().timeIntervalSince1970),
                    ])
                var final: [NodeId: SyncJournalEvent] = [:]
                for event in page.events {
                    try SyncNodeMaterializer.validateEvent(event)
                    if let previousEvent = final[event.resourceId] {
                        guard previousEvent.kind != .nodePurged,
                            SyncDecimalValidation.less(
                                previousEvent.resourceRevision.rawValue,
                                event.resourceRevision.rawValue)
                                || (event.kind == .nodePurged
                                    && previousEvent.resourceRevision == event.resourceRevision)
                        else { throw SyncProjectionFailure.revisionRegression }
                    } else if let old = try cacheRows(
                        page.scope, epoch: epoch, node: event.resourceId
                    ).first {
                        guard old.lifecycle != .purged else {
                            throw SyncProjectionFailure.reconciliationRequired
                        }
                        guard
                            SyncDecimalValidation.less(
                                old.revision, event.resourceRevision.rawValue)
                                || (event.kind == .nodePurged
                                    && old.revision == event.resourceRevision.rawValue)
                        else { throw SyncProjectionFailure.revisionRegression }
                    }
                    if final[event.resourceId] == nil,
                        try cacheRows(page.scope, epoch: epoch, node: event.resourceId).isEmpty,
                        let bootstrap = try activeSnapshotId(page.scope, epoch: epoch),
                        let revision = try query(
                            "SELECT revision FROM rebaseline_nodes WHERE bootstrap_id=? AND node_id=?",
                            [.text(bootstrap), .text(event.resourceId.rawValue)],
                            map: { try Self.text($0, 0) }
                        ).first
                    {
                        guard
                            SyncDecimalValidation.less(revision, event.resourceRevision.rawValue)
                                || (event.kind == .nodePurged
                                    && revision == event.resourceRevision.rawValue)
                        else { throw SyncProjectionFailure.revisionRegression }
                    }
                    if event.kind != .nodePurged,
                        try scalar(
                            "SELECT count(*) FROM projection_events WHERE scope_id=? AND node_id=? AND kind='NODE_PURGED'",
                            [.integer(scopeId), .text(event.resourceId.rawValue)]) > 0
                    {
                        throw SyncProjectionFailure.reconciliationRequired
                    }
                    guard
                        try scalar(
                            "SELECT count(*) FROM projection_events WHERE scope_id=? AND epoch=? AND (sequence=? OR event_id=?)",
                            [
                                .integer(scopeId), .text(epoch), .text(event.sequence.rawValue),
                                .text(event.id.rawValue),
                            ]) == 0
                    else { throw SyncProjectionFailure.conflictingEvent }
                    try execute(
                        "INSERT INTO projection_events VALUES(?,?,?,?,?,?,?,?)",
                        [
                            .integer(scopeId), .text(epoch), .text(event.sequence.rawValue),
                            .text(event.id.rawValue), .text(page.start.sequence.rawValue),
                            .text(event.resourceId.rawValue),
                            .text(event.resourceRevision.rawValue), .text(event.kind.rawValue),
                        ])
                    final[event.resourceId] = event
                }
                var writes = 0
                for id in final.keys.sorted(by: { $0.rawValue < $1.rawValue }) {
                    let event = final[id]!
                    let old = try cacheRows(page.scope, epoch: epoch, node: id).first
                    let lifecycle: CachedNodeLifecycle
                    let provenance: CachedNodeProvenance
                    let metadata: Data?
                    let parent: String?
                    switch event.kind {
                    case .nodePurged:
                        lifecycle = .purged
                        provenance = .event
                        metadata = nil
                        parent = nil
                    case .nodeTrashed:
                        lifecycle = .trashed
                        provenance = old?.metadata == nil ? .event : .lastKnown
                        metadata = old?.metadata
                        parent = event.parentId?.rawValue
                    case .nodeCreated, .nodeRenamed, .nodeMoved, .nodeRestored,
                        .fileContentCommitted, .fileVersionRestored:
                        guard let prepared = plan.nodes[id] else {
                            throw SyncProjectionFailure.missingMaterialization
                        }
                        let node = prepared.node
                        guard node.id == id, node.libraryId == page.scope.libraryId,
                            node.revision.rawValue == event.resourceRevision.rawValue,
                            node.state == .active,
                            node.parentId == event.parentId, node.parentId != id,
                            event.nodeKind == nil || event.nodeKind == node.kind,
                            event.nodeState == nil || event.nodeState == node.state,
                            node.currentVersionId == event.currentVersionId,
                            ![.fileContentCommitted, .fileVersionRestored].contains(event.kind)
                                || node.kind == .file,
                            let parentId = node.parentId,
                            let parentNode = plan.parents[parentId]?.node,
                            parentNode.id == parentId, parentNode.libraryId == page.scope.libraryId,
                            parentNode.kind == .directory, parentNode.state == .active
                        else { throw SyncProjectionFailure.malformedMetadata }
                        if let parentRow = try cacheRows(page.scope, epoch: epoch, node: parentId)
                            .first,
                            parentRow.lifecycle != .active, final[parentId]?.kind != .nodeRestored
                        {
                            throw SyncProjectionFailure.invalidParent
                        }
                        lifecycle = .active
                        provenance = .canonical
                        metadata = prepared.bytes
                        parent = node.parentId?.rawValue
                    }
                    let values: [SQLValue] = [
                        .integer(scopeId), .text(epoch), .text(id.rawValue),
                        .text(event.resourceRevision.rawValue), .text(event.sequence.rawValue),
                        parent.map(SQLValue.text) ?? .null, .text(lifecycle.rawValue),
                        .text(provenance.rawValue), metadata.map(SQLValue.blob) ?? .null,
                    ]
                    try execute(
                        "INSERT INTO cached_nodes VALUES(?,?,?,?,?,?,?,?,?,1) ON CONFLICT(scope_id,epoch,node_id) DO UPDATE SET revision=excluded.revision,sequence=excluded.sequence,parent_id=excluded.parent_id,lifecycle=excluded.lifecycle,provenance=excluded.provenance,metadata=excluded.metadata",
                        values)
                    writes += 1
                    if writes == 1 { try fault?(.afterFirstNodeWrite) }
                }
                for prepared in plan.nodes.values {
                    guard
                        try activeAncestry(
                            scope: page.scope, epoch: epoch, parent: prepared.node.id)
                    else { throw SyncProjectionFailure.invalidParent }
                }
                try fault?(.afterFinalNodeWrite)
                try execute(
                    "UPDATE node_projection_state SET applied_sequence=?,completeness=CASE WHEN completeness='COMPLETE' THEN 'COMPLETE' ELSE 'PARTIAL' END WHERE scope_id=? AND epoch=?",
                    [.text(page.through.rawValue), .integer(scopeId), .text(epoch)])
                try execute(
                    "UPDATE inbound_pages SET state='APPLIED_ACK_PENDING',updated_at=? WHERE scope_id=? AND epoch=? AND from_sequence=?",
                    [
                        .real(Date().timeIntervalSince1970), .integer(scopeId), .text(epoch),
                        .text(page.start.sequence.rawValue),
                    ])
                try requireProjectionCommit(page)
                try fault?(.beforeProjectionCommit)
                return false
            }
            committed = true
            try fault?(.afterProjectionCommit)
            return existing
        } catch {
            if committed { throw MutationQueueFailure.commitAcknowledgementLost }
            throw error
        }
    }
    private func requireProjectionCommit(_ page: SyncFeedPage) throws {
        let scopeId = try requireScopeId(page.scope)
        guard let state = try projectionRow(page.scope), state.epoch == page.start.epoch.rawValue,
            !SyncDecimalValidation.less(state.applied, page.through.rawValue),
            try scalar(
                "SELECT count(*) FROM projection_commits c WHERE scope_id=? AND epoch=? AND from_sequence=? AND through_sequence=? AND canonical_data=? AND event_count=? AND event_count=(SELECT count(*) FROM projection_events e WHERE e.scope_id=c.scope_id AND e.epoch=c.epoch AND e.from_sequence=c.from_sequence)",
                [
                    .integer(scopeId), .text(page.start.epoch.rawValue),
                    .text(page.start.sequence.rawValue), .text(page.through.rawValue),
                    .blob(page.canonicalData), .integer(Int64(page.events.count)),
                ]) == 1
        else { throw SyncFeedFailure.applicationCommitRequired }
    }
}

extension MutationQueueSQLiteStore {
    private struct AckAttempt: Equatable {
        let id: String, owner: String, previous: String, applied: String
        let dispatched: Bool, completed: Bool
    }
    private func projectionCanAcknowledge(_ row: ProjectionRow, scope: ClientMutationScope) throws
        -> Bool
    {
        if row.completeness == .complete {
            return try activeSnapshotId(scope, epoch: row.epoch) != nil
        }
        return row.completeness == .partial
    }
    private func ackAttempt(_ proof: AppliedSyncPageProof) throws -> AckAttempt? {
        let parts = proof.commitIdentity.split(separator: "/")
        guard parts.count == 2 else { throw SyncFeedFailure.applicationCommitRequired }
        return try query(
            "SELECT attempt_id,attempt_owner,previous_sequence,applied_sequence,dispatched,completed FROM sync_ack_attempts WHERE scope_id=? AND epoch=? AND from_sequence=? AND attempt_id=?",
            [
                .integer(try requireScopeId(proof.evidence.scope)),
                .text(proof.evidence.epoch.rawValue), .text(proof.evidence.from.rawValue),
                .text(String(parts[1])),
            ]
        ) { s in
            AckAttempt(
                id: try Self.text(s, 0), owner: try Self.text(s, 1), previous: try Self.text(s, 2),
                applied: try Self.text(s, 3), dispatched: sqlite3_column_int(s, 4) == 1,
                completed: sqlite3_column_int(s, 5) == 1)
        }.first
    }
    private func commitIdentity(_ page: SyncFeedPage) throws -> String {
        try requireProjectionCommit(page)
        guard
            let id = try query(
                "SELECT commit_id FROM projection_commits WHERE scope_id=? AND epoch=? AND from_sequence=?",
                [
                    .integer(try requireScopeId(page.scope)), .text(page.start.epoch.rawValue),
                    .text(page.start.sequence.rawValue),
                ], map: { try Self.text($0, 0) }
            ).first
        else { throw SyncFeedFailure.applicationCommitRequired }
        return id
    }
    func claimProjectionPage(
        scope: ClientMutationScope, position: SyncJournalPosition, credentialId: String,
        bridge: any RustBridgeProtocol, recovery: Bool
    ) async throws -> AppliedSyncPageProof {
        try requireNoRebaseline(scope)
        guard
            let record = try await inboundPage(
                scope: scope, position: position, credentialId: credentialId, bridge: bridge),
            let evidence = record.page.evidence
        else { throw SyncFeedFailure.applicationCommitRequired }
        try Task.checkCancellation()
        // OS ownership serializes live attempts across store instances/processes. The zero-byte
        // lock contains no metadata or token. Process termination releases it without networking.
        try connection.acquireAckLock(path: url.path + ".ack.lock")
        var issued = false
        defer { if !issued { connection.releaseAckLock() } }
        let proof = try transaction {
            try requireFeedScope(scope, credentialId: credentialId)
            guard let raw = try rawInbound(scope: scope, position: position),
                raw.canonical == record.page.canonicalData,
                raw.state == (recovery ? .ackInFlight : .appliedAckPending),
                let row = try projectionRow(scope), try projectionCanAcknowledge(row, scope: scope),
                let base = try syncBase(scope: scope), base.status == .verified,
                base.epoch == row.epoch,
                base.sequence == row.confirmed
            else { throw SyncFeedFailure.applicationCommitRequired }
            let scopeId = try requireScopeId(scope)
            let positions = try query(
                "SELECT from_sequence FROM inbound_pages WHERE scope_id=? AND epoch=? AND state IN ('APPLIED_ACK_PENDING','ACK_IN_FLIGHT')",
                [.integer(scopeId), .text(position.epoch.rawValue)]
            ) { try Self.text($0, 0) }
            guard
                !positions.contains(where: {
                    SyncDecimalValidation.less($0, position.sequence.rawValue)
                })
            else { throw SyncFeedFailure.checkpointConflict }
            let existing = try query(
                "SELECT attempt_id FROM sync_ack_attempts WHERE scope_id=? AND epoch=? AND from_sequence=?",
                [
                    .integer(scopeId), .text(position.epoch.rawValue),
                    .text(position.sequence.rawValue),
                ]
            ) { try Self.text($0, 0) }
            guard existing.count < MutationQueuePolicy.maximumRecoveryAttempts,
                existing.allSatisfy({ !activeAckAttempts.contains($0) })
            else { throw SyncFeedFailure.applicationCommitRequired }
            let commit = try commitIdentity(record.page)
            let attempt = UUID().uuidString
            try checkCapacity(additionalBytes: 256)
            connection.projectionAuthority = 2
            defer { connection.projectionAuthority = 0 }
            try execute(
                "INSERT INTO sync_ack_attempts(scope_id,epoch,from_sequence,attempt_id,attempt_owner,previous_sequence,applied_sequence,started_at) VALUES(?,?,?,?,?,?,?,?)",
                [
                    .integer(scopeId), .text(position.epoch.rawValue),
                    .text(position.sequence.rawValue), .text(attempt), .text(processOwner),
                    .text(row.confirmed), .text(row.applied), .real(Date().timeIntervalSince1970),
                ])
            if !recovery {
                try execute(
                    "UPDATE inbound_pages SET state='ACK_IN_FLIGHT',updated_at=? WHERE scope_id=? AND epoch=? AND from_sequence=?",
                    [
                        .real(Date().timeIntervalSince1970), .integer(scopeId),
                        .text(position.epoch.rawValue), .text(position.sequence.rawValue),
                    ])
            }
            try fault?(.beforeAckLeaseCommit)
            return AppliedSyncPageProof(
                evidence: evidence,
                locallyApplied: SyncJournalPosition(
                    epoch: position.epoch, sequence: try SyncDecimalValidation.validate(row.applied)
                ),
                previouslyConfirmed: SyncJournalPosition(
                    epoch: position.epoch,
                    sequence: try SyncDecimalValidation.validate(row.confirmed)),
                commitIdentity: commit + "/" + attempt)
        }
        // No dispatch capability exists before durable lease COMMIT. Post-commit failure leaves
        // recoverable ACK_IN_FLIGHT without retaining an inaccessible process-local reservation.
        try fault?(.afterAckLeaseCommit)
        activeAckAttempts.insert(String(proof.commitIdentity.split(separator: "/")[1]))
        issued = true
        return proof
    }
    func validateProjectionProof(_ proof: AppliedSyncPageProof, credentialId: String) throws {
        try requireNoRebaseline(proof.evidence.scope)
        try requireFeedScope(proof.evidence.scope, credentialId: credentialId)
        guard
            let raw = try rawInbound(
                scope: proof.evidence.scope,
                position: SyncJournalPosition(
                    epoch: proof.evidence.epoch, sequence: proof.evidence.from)),
            raw.state == .ackInFlight,
            let row = try projectionRow(proof.evidence.scope),
            row.epoch == proof.evidence.epoch.rawValue,
            try projectionCanAcknowledge(row, scope: proof.evidence.scope),
            row.applied == proof.locallyApplied.sequence.rawValue,
            row.confirmed == proof.previouslyConfirmed.sequence.rawValue,
            let attempt = try ackAttempt(proof), !attempt.completed, attempt.owner == processOwner,
            activeAckAttempts.contains(attempt.id), attempt.previous == row.confirmed,
            attempt.applied == row.applied,
            let commit = try query(
                "SELECT commit_id FROM projection_commits WHERE scope_id=? AND epoch=? AND from_sequence=? AND through_sequence=? AND canonical_data=?",
                [
                    .integer(try requireScopeId(proof.evidence.scope)),
                    .text(proof.evidence.epoch.rawValue), .text(proof.evidence.from.rawValue),
                    .text(proof.evidence.through.rawValue), .blob(raw.canonical),
                ], map: { try Self.text($0, 0) }
            ).first,
            proof.commitIdentity == commit + "/" + attempt.id,
            proof.locallyApplied.epoch == proof.evidence.epoch,
            proof.previouslyConfirmed.epoch == proof.evidence.epoch
        else { throw SyncFeedFailure.applicationCommitRequired }
        // Signed evidence must be exactly the token and page position retained in canonical wire data.
        guard
            let envelope = try JSONSerialization.jsonObject(with: raw.canonical) as? [String: Any],
            envelope["ack_token"] as? String == proof.evidence.token,
            envelope["epoch"] as? String == proof.evidence.epoch.rawValue,
            envelope["from_sequence"] as? String == proof.evidence.from.rawValue,
            envelope["through_sequence"] as? String == proof.evidence.through.rawValue,
            envelope["high_watermark"] as? String == proof.evidence.highWatermark.rawValue
        else { throw SyncFeedFailure.applicationCommitRequired }
        guard let base = try syncBase(scope: proof.evidence.scope), base.status == .verified,
            base.epoch == row.epoch, base.sequence == row.confirmed
        else { throw SyncFeedFailure.checkpointConflict }
    }
    func authorizeProjectionAckDispatch(_ proof: AppliedSyncPageProof, credentialId: String) throws
    {
        try transaction {
            try validateProjectionProof(proof, credentialId: credentialId)
            guard let attempt = try ackAttempt(proof), !attempt.dispatched else {
                throw SyncFeedFailure.applicationCommitRequired
            }
            connection.projectionAuthority = 2
            defer { connection.projectionAuthority = 0 }
            try execute(
                "UPDATE sync_ack_attempts SET dispatched=1 WHERE attempt_id=?", [.text(attempt.id)])
        }
    }
    func finishProjectionAttempt(_ proof: AppliedSyncPageProof) {
        guard let attempt = proof.commitIdentity.split(separator: "/").last else { return }
        activeAckAttempts.remove(String(attempt))
        if activeAckAttempts.isEmpty { connection.releaseAckLock() }
    }
    func blockProjectionPage(_ proof: AppliedSyncPageProof, credentialId: String) throws {
        try transaction {
            try validateProjectionProof(proof, credentialId: credentialId)
            let id = try requireScopeId(proof.evidence.scope)
            connection.projectionAuthority = 4
            defer { connection.projectionAuthority = 0 }
            try execute(
                "UPDATE node_projection_state SET completeness='REBASELINE_REQUIRED' WHERE scope_id=?",
                [.integer(id)])
            try execute(
                "UPDATE inbound_pages SET state='BLOCKED_REBASELINE',updated_at=? WHERE scope_id=? AND epoch=? AND from_sequence=?",
                [
                    .real(Date().timeIntervalSince1970), .integer(id),
                    .text(proof.evidence.epoch.rawValue), .text(proof.evidence.from.rawValue),
                ])
            try execute(
                "UPDATE sync_bases SET status='RECONCILIATION_REQUIRED' WHERE scope_id=?",
                [.integer(id)])
        }
    }
    func confirmProjectionPage(
        _ proof: AppliedSyncPageProof, checkpoint: SyncCheckpoint, credentialId: String,
        bridge: any RustBridgeProtocol
    ) async throws {
        let verified = try await SyncCheckpointResponseDecoder(bridge: bridge).decode(
            HTTPTransportResponse(
                statusCode: 200, headers: ["Content-Type": "application/json"],
                body: checkpoint.responseBody), scope: proof.evidence.scope)
        guard verified == checkpoint else { throw SyncFeedFailure.protocolFailure }
        try transaction {
            try validateProjectionProof(proof, credentialId: credentialId)
            guard checkpoint.base.scope == proof.evidence.scope,
                checkpoint.base.epoch == proof.evidence.epoch,
                !SyncDecimalValidation.less(
                    checkpoint.base.sequence.rawValue, proof.evidence.through.rawValue),
                !SyncDecimalValidation.less(
                    checkpoint.base.sequence.rawValue, proof.previouslyConfirmed.sequence.rawValue),
                !SyncDecimalValidation.less(
                    proof.locallyApplied.sequence.rawValue, checkpoint.base.sequence.rawValue),
                let attempt = try ackAttempt(proof), attempt.dispatched
            else { throw SyncFeedFailure.checkpointConflict }
            let scopeId = try requireScopeId(proof.evidence.scope)
            let old = try syncBase(scope: proof.evidence.scope)
            try checkCapacity(
                additionalBytes: checkpoint.responseBody.count * 2 - (old?.responseBody.count ?? 0))
            connection.projectionAuthority = 3
            defer { connection.projectionAuthority = 0 }
            let incompatible =
                try scalar(
                    "SELECT count(*) FROM mutations WHERE scope_id=? AND state NOT IN ('APPLIED','FAILED_PERMANENT') AND (epoch<>? OR sequence<>?)",
                    [
                        .integer(scopeId), .text(checkpoint.base.epoch.rawValue),
                        .text(checkpoint.base.sequence.rawValue),
                    ]) > 0
            try execute(
                "UPDATE sync_bases SET sequence=?,response=?,status=? WHERE scope_id=? AND epoch=?",
                [
                    .text(checkpoint.base.sequence.rawValue), .blob(checkpoint.responseBody),
                    .text(incompatible ? "RECONCILIATION_REQUIRED" : "VERIFIED"), .integer(scopeId),
                    .text(checkpoint.base.epoch.rawValue),
                ])
            try execute(
                "UPDATE node_projection_state SET confirmed_sequence=?,completeness=? WHERE scope_id=?",
                [
                    .text(checkpoint.base.sequence.rawValue),
                    .text(
                        incompatible
                            ? "REBASELINE_REQUIRED"
                            : (try projectionRow(proof.evidence.scope)?.completeness == .complete
                                ? "COMPLETE" : "PARTIAL")), .integer(scopeId),
                ])
            try execute(
                "UPDATE sync_ack_attempts SET completed=1,checkpoint=? WHERE attempt_id=?",
                [.blob(checkpoint.responseBody), .text(attempt.id)])
            try execute(
                "UPDATE inbound_pages SET state='ACK_CONFIRMED',updated_at=? WHERE scope_id=? AND epoch=? AND from_sequence=?",
                [
                    .real(Date().timeIntervalSince1970), .integer(scopeId),
                    .text(proof.evidence.epoch.rawValue), .text(proof.evidence.from.rawValue),
                ])
            try fault?(.beforeAckConfirmationCommit)
        }
        try fault?(.afterAckConfirmationCommit)
    }
}

// MARK: - Immutable full metadata snapshots, on the same actor-owned connection
extension MutationQueueSQLiteStore {
    private struct SnapshotRow: Equatable {
        let id: String, generation: String, epoch: String, cut: String
        let bootstrap: Data, library: Data
        let stage: RebaselineStageState
        let cursor: String?, token: String?, prepared: String?, completion: Data?
    }
    private func snapshotRow(_ scope: ClientMutationScope) throws -> SnapshotRow? {
        try query(
            "SELECT bootstrap_id,generation,epoch,cut,bootstrap,library,stage,cursor,terminal_token,prepared_id,completion FROM rebaseline_sessions WHERE scope_id=? ORDER BY rowid DESC LIMIT 1",
            [.integer(try requireScopeId(scope))]
        ) { s in
            guard let stage = RebaselineStageState(rawValue: try Self.text(s, 6)) else {
                throw MutationQueueFailure.malformedRecord
            }
            func optional(_ index: Int32) throws -> String? {
                sqlite3_column_type(s, index) == SQLITE_NULL ? nil : try Self.text(s, index)
            }
            return SnapshotRow(
                id: try Self.text(s, 0), generation: try Self.text(s, 1),
                epoch: try Self.text(s, 2), cut: try Self.text(s, 3),
                bootstrap: try Self.blob(s, 4, maximum: 16384),
                library: try Self.blob(s, 5, maximum: 16384), stage: stage,
                cursor: try optional(7), token: try optional(8), prepared: try optional(9),
                completion: sqlite3_column_type(s, 10) == SQLITE_NULL
                    ? nil : try Self.blob(s, 10, maximum: 16384))
        }.first
    }
    private func requireSnapshotOwner(_ owner: UUID) throws {
        guard connection.inboundLockOwner == owner, connection.ackLockHeld else {
            throw RebaselineFailure.alreadyRunning
        }
    }
    private func snapshotTransaction<T>(_ owner: UUID, _ operation: () throws -> T) throws -> T {
        try requireSnapshotOwner(owner)
        connection.projectionAuthority = 5
        defer { connection.projectionAuthority = 0 }
        return try transaction(operation)
    }
    private func requireNoRebaseline(_ scope: ClientMutationScope) throws {
        guard let id = try scopeId(scope) else { return }
        guard
            try scalar(
                "SELECT count(*) FROM rebaseline_sessions WHERE scope_id=? AND stage NOT IN ('ACTIVE_COMPLETE','EXPIRED','BLOCKED')",
                [.integer(id)]) == 0,
            try scalar(
                "SELECT count(*) FROM rebaseline_start_attempts WHERE scope_id=? AND resolved=0",
                [.integer(id)]) == 0
        else { throw MutationQueueFailure.reconciliationRequired }
    }
    private func requireSnapshotCompatibility(_ scope: ClientMutationScope) throws {
        let id = try requireScopeId(scope)
        guard
            try scalar(
                "SELECT count(*) FROM mutations WHERE scope_id=? AND state NOT IN ('APPLIED','FAILED_PERMANENT')",
                [.integer(id)]) == 0
        else { throw RebaselineFailure.incompatibleMutations }
        guard
            try scalar(
                "SELECT count(*) FROM inbound_pages WHERE scope_id=? AND state IN ('RECEIVED_UNAPPLIED','APPLIED_ACK_PENDING','ACK_IN_FLIGHT')",
                [.integer(id)]) == 0
        else { throw RebaselineFailure.incompatibleSync }
    }
    func claimRebaselineRun(scope: ClientMutationScope, credentialId: String, owner: UUID) throws {
        try requireFeedScope(scope, credentialId: credentialId)
        try connection.acquireInboundLock(path: url.path + ".inbound.lock", owner: owner)
        do { try connection.acquireAckLock(path: url.path + ".ack.lock") } catch {
            connection.releaseInboundLock(owner: owner)
            throw RebaselineFailure.alreadyRunning
        }
    }
    func finishRebaselineRun(owner: UUID) {
        guard connection.inboundLockOwner == owner else { return }
        connection.releaseAckLock()
        connection.releaseInboundLock(owner: owner)
    }
    func beginRebaselineStart(scope: ClientMutationScope, credentialId: String, owner: UUID) throws
        -> String
    {
        try snapshotTransaction(owner) {
            try requireFeedScope(scope, credentialId: credentialId)
            try requireSnapshotCompatibility(scope)
            if let saved = try snapshotRow(scope),
                ![.expired, .blocked, .activeComplete].contains(saved.stage)
            {
                throw RebaselineFailure.reconciliationRequired
            }
            guard try scalar("SELECT count(*) FROM rebaseline_start_attempts") < 1024 else {
                throw RebaselineFailure.capacity
            }
            try checkSnapshotCapacity(additional: 16384)
            let attempt = UUID().uuidString
            try execute(
                "INSERT INTO rebaseline_start_attempts(scope_id,attempt_id) VALUES(?,?)",
                [.integer(try requireScopeId(scope)), .text(attempt)])
            return attempt
        }
    }
    func saveRebaselineStart(
        _ bootstrap: RebaselineBootstrap, response: Data, library: Library, credentialId: String,
        owner: UUID, attempt: String
    ) throws {
        try snapshotTransaction(owner) {
            let scope = bootstrap.scope
            try requireFeedScope(scope, credentialId: credentialId)
            try requireSnapshotCompatibility(scope)
            guard bootstrap.state == .open, library.id == scope.libraryId,
                library.status != .quarantined, response.count <= 16384,
                try scalar(
                    "SELECT count(*) FROM rebaseline_start_attempts WHERE scope_id=? AND attempt_id=? AND resolved=0",
                    [.integer(try requireScopeId(scope)), .text(attempt)]) == 1
            else { throw RebaselineFailure.scopeMismatch }
            if let base = try syncBase(scope: scope),
                base.epoch == bootstrap.position.epoch.rawValue,
                SyncDecimalValidation.less(bootstrap.position.sequence.rawValue, base.sequence)
            {
                throw RebaselineFailure.reconciliationRequired
            }
            if let old = try snapshotRow(scope) {
                guard old.id != bootstrap.id.rawValue,
                    [.expired, .blocked, .activeComplete].contains(old.stage),
                    SyncDecimalValidation.less(old.generation, bootstrap.generation.value.rawValue)
                else { throw RebaselineFailure.generationMismatch }
            }
            let libraryBytes = try JSONEncoder().encode(LibraryProjectionMetadata(library))
            try checkSnapshotCapacity(additional: response.count + libraryBytes.count + 512)
            try execute(
                "INSERT INTO rebaseline_sessions(scope_id,bootstrap_id,generation,epoch,cut,bootstrap,library,stage) VALUES(?,?,?,?,?,?,?,'BOOTSTRAP_OPEN')",
                [
                    .integer(try requireScopeId(scope)), .text(bootstrap.id.rawValue),
                    .text(bootstrap.generation.value.rawValue),
                    .text(bootstrap.position.epoch.rawValue),
                    .text(bootstrap.position.sequence.rawValue), .blob(response),
                    .blob(libraryBytes),
                ])
            try execute(
                "UPDATE rebaseline_start_attempts SET resolved=1 WHERE scope_id=? AND resolved=0",
                [.integer(try requireScopeId(scope))])
        }
    }
    func rebaseline(
        scope: ClientMutationScope, credentialId: String, bridge: any RustBridgeProtocol
    ) async throws -> StoredRebaseline? {
        try requireFeedScope(scope, credentialId: credentialId)
        guard let row = try snapshotRow(scope) else { return nil }
        let bootstrap = try await RebaselineResponseDecoder(bridge: bridge).start(
            RebaselineCoordinator.response(row.bootstrap), scope: scope)
        let library = try await JSONDecoder().decode(
            LibraryProjectionMetadata.self, from: row.library
        ).validated(bridge: bridge)
        guard bootstrap.state == .open, row.id == bootstrap.id.rawValue,
            row.generation == bootstrap.generation.value.rawValue,
            row.epoch == bootstrap.position.epoch.rawValue,
            row.cut == bootstrap.position.sequence.rawValue,
            library.id == scope.libraryId,
            row.cursor.map({ RebaselinePolicy.validOpaque($0, maximum: 320) }) ?? true,
            row.token.map({ RebaselinePolicy.validOpaque($0, maximum: 336) }) ?? true
        else { throw RebaselineFailure.protocolFailure }
        try requireFeedScope(scope, credentialId: credentialId)
        guard try snapshotRow(scope) == row else { throw RebaselineFailure.generationMismatch }
        return StoredRebaseline(
            bootstrap: bootstrap, state: row.stage, cursor: row.cursor, terminalToken: row.token,
            preparedId: row.prepared, library: library, completion: row.completion)
    }
    func rebaselineProgress(
        scope: ClientMutationScope, credentialId: String, bridge: any RustBridgeProtocol
    ) async throws -> RebaselineProgress? {
        try requireFeedScope(scope, credentialId: credentialId)
        if try scalar(
            "SELECT count(*) FROM rebaseline_start_attempts WHERE scope_id=? AND resolved=0",
            [.integer(try requireScopeId(scope))]) > 0
        {
            return RebaselineProgress(
                state: .startUnknown, pages: 0, stagedNodes: 0, expectedNodes: 0, position: nil)
        }
        guard
            let saved = try await rebaseline(
                scope: scope, credentialId: credentialId, bridge: bridge)
        else {
            let uncertain =
                try scalar(
                    "SELECT count(*) FROM rebaseline_start_attempts WHERE scope_id=? AND resolved=0",
                    [.integer(try requireScopeId(scope))]) > 0
            return uncertain
                ? RebaselineProgress(
                    state: .startUnknown, pages: 0, stagedNodes: 0, expectedNodes: 0, position: nil)
                : nil
        }
        let counts = try snapshotCounts(saved.bootstrap.id.rawValue)
        guard counts.0 <= RebaselinePolicy.maximumPages, counts.1 <= saved.bootstrap.itemCount
        else { throw RebaselineFailure.countMismatch }
        return RebaselineProgress(
            state: saved.state == .completionInFlight ? .outcomeUnknown : saved.state,
            pages: counts.0, stagedNodes: counts.1, expectedNodes: saved.bootstrap.itemCount,
            position: saved.bootstrap.position)
    }
    private func snapshotCounts(_ id: String) throws -> (Int, Int) {
        (
            Int(
                try scalar(
                    "SELECT count(*) FROM rebaseline_pages WHERE bootstrap_id=?", [.text(id)])),
            Int(
                try scalar(
                    "SELECT count(*) FROM rebaseline_nodes WHERE bootstrap_id=?", [.text(id)]))
        )
    }
    private func checkSnapshotCapacity(additional: Int) throws {
        let bytes = try scalar(
            "SELECT coalesce((SELECT sum(length(response)+length(canonical)+512) FROM rebaseline_pages),0)+coalesce((SELECT sum(length(metadata)+256) FROM rebaseline_nodes),0)+coalesce((SELECT sum(length(bootstrap)+length(library)+coalesce(length(completion),0)+1024) FROM rebaseline_sessions),0)+coalesce((SELECT sum(coalesce(length(completion),0)+512) FROM rebaseline_completion_attempts),0)"
        )
        guard bytes + Int64(additional) <= Int64(RebaselinePolicy.maximumStoredBytes) else {
            throw RebaselineFailure.capacity
        }
        try checkCapacity(additionalBytes: additional)
    }
    private func manifestNodeBytes(_ page: RebaselineManifestPage) throws -> [Data] {
        let root = try JSONSerialization.jsonObject(with: page.responseBody) as! [String: Any]
        let data = root["data"] as! [String: Any]
        return try (data["nodes"] as! [[String: Any]]).map {
            try JSONSerialization.data(withJSONObject: $0, options: [.sortedKeys])
        }
    }
    func stageRebaseline(
        _ page: RebaselineManifestPage, cursor: String?, credentialId: String, owner: UUID,
        bridge: any RustBridgeProtocol
    ) async throws {
        let scope = page.bootstrap.scope
        guard
            let saved = try await rebaseline(
                scope: scope, credentialId: credentialId, bridge: bridge),
            saved.bootstrap.sameManifest(as: page.bootstrap)
        else { throw RebaselineFailure.generationMismatch }
        let verified = try await RebaselineResponseDecoder(bridge: bridge).page(
            RebaselineCoordinator.response(page.responseBody), expected: saved.bootstrap,
            limit: 1000)
        guard verified == page else { throw RebaselineFailure.protocolFailure }
        let bytes = try manifestNodeBytes(page)
        try Task.checkCancellation()
        try snapshotTransaction(owner) {
            try requireFeedScope(scope, credentialId: credentialId)
            guard let row = try snapshotRow(scope), row.id == page.bootstrap.id.rawValue else {
                throw RebaselineFailure.generationMismatch
            }
            let old = try query(
                "SELECT canonical FROM rebaseline_pages WHERE bootstrap_id=? AND request_cursor=?",
                [.text(row.id), .text(cursor ?? "")]
            ) { try Self.blob($0, 0, maximum: RebaselinePolicy.maximumResponseBytes) }.first
            if let old {
                guard old == page.canonicalData else { throw RebaselineFailure.protocolFailure }
                return
            }
            guard [.bootstrapOpen, .downloading].contains(row.stage), row.cursor == cursor,
                page.nextCursor != cursor || page.nextCursor == nil
            else { throw RebaselineFailure.protocolFailure }
            if let next = page.nextCursor {
                guard
                    try scalar(
                        "SELECT count(*) FROM rebaseline_pages WHERE bootstrap_id=? AND request_cursor=?",
                        [.text(row.id), .text(next)]) == 0
                else { throw RebaselineFailure.protocolFailure }
            }
            let counts = try snapshotCounts(row.id)
            guard counts.0 < RebaselinePolicy.maximumPages,
                counts.1 + page.nodes.count <= saved.bootstrap.itemCount
            else { throw RebaselineFailure.countMismatch }
            let previous = try query(
                "SELECT node_id FROM rebaseline_nodes WHERE bootstrap_id=? ORDER BY node_id DESC LIMIT 1",
                [.text(row.id)]
            ) { try Self.text($0, 0) }.first
            guard
                page.nodes.first.map({ node in previous.map { $0 < node.id.rawValue } ?? true })
                    ?? true
            else { throw RebaselineFailure.protocolFailure }
            try checkSnapshotCapacity(
                additional: page.responseBody.count + page.canonicalData.count
                    + bytes.reduce(0) { $0 + $1.count + 256 } + 512)
            for (node, metadata) in zip(page.nodes, bytes) {
                guard metadata.count <= RebaselinePolicy.maximumNodeBytes else {
                    throw RebaselineFailure.capacity
                }
                try execute(
                    "INSERT INTO rebaseline_nodes VALUES(?,?,?,?,?,?,?)",
                    [
                        .text(row.id), .text(node.id.rawValue),
                        node.parentId.map { .text($0.rawValue) } ?? .null,
                        .text(node.kind.rawValue), .text(node.state.rawValue),
                        .text(node.revision.rawValue), .blob(metadata),
                    ])
            }
            try execute(
                "INSERT INTO rebaseline_pages VALUES(?,?,?,?,?,?)",
                [
                    .text(row.id), .integer(Int64(counts.0)), .text(cursor ?? ""),
                    page.nextCursor.map(SQLValue.text) ?? .null, .blob(page.responseBody),
                    .blob(page.canonicalData),
                ])
            if page.evidence != nil, counts.1 + page.nodes.count != saved.bootstrap.itemCount {
                throw RebaselineFailure.countMismatch
            }
            try execute(
                "UPDATE rebaseline_sessions SET cursor=?,terminal_token=?,stage=? WHERE bootstrap_id=?",
                [
                    page.nextCursor.map(SQLValue.text) ?? .null,
                    page.evidence.map { .text($0.token) } ?? .null,
                    .text(page.evidence == nil ? "DOWNLOADING" : "TERMINAL_RECEIVED"),
                    .text(row.id),
                ])
            try fault?(.beforeSnapshotPageCommit)
        }
    }
    private func verifySnapshot(
        _ saved: StoredRebaseline, credentialId: String, bridge: any RustBridgeProtocol
    ) async throws {
        let id = saved.bootstrap.id.rawValue
        let counts = try snapshotCounts(id)
        guard counts.0 > 0, counts.0 <= RebaselinePolicy.maximumPages,
            counts.1 == saved.bootstrap.itemCount,
            let token = saved.terminalToken, saved.cursor == nil
        else { throw RebaselineFailure.unverifiedManifest }
        var cursor: String?, last: String?
        var total = 0
        for index in 0..<counts.0 {
            try Task.checkCancellation()
            let raw = try query(
                "SELECT request_cursor,next_cursor,response,canonical FROM rebaseline_pages WHERE bootstrap_id=? AND page_index=?",
                [.text(id), .integer(Int64(index))]
            ) { s in
                (
                    try Self.text(s, 0),
                    sqlite3_column_type(s, 1) == SQLITE_NULL ? nil : try Self.text(s, 1),
                    try Self.blob(s, 2, maximum: RebaselinePolicy.maximumResponseBytes),
                    try Self.blob(s, 3, maximum: RebaselinePolicy.maximumResponseBytes)
                )
            }.first
            guard let raw, raw.0 == (cursor ?? "") else { throw RebaselineFailure.protocolFailure }
            let page = try await RebaselineResponseDecoder(bridge: bridge).page(
                RebaselineCoordinator.response(raw.2), expected: saved.bootstrap, limit: 1000)
            guard page.canonicalData == raw.3, page.nextCursor == raw.1,
                index == counts.0 - 1 ? page.evidence?.token == token : page.evidence == nil
            else { throw RebaselineFailure.unverifiedManifest }
            let bytes = try manifestNodeBytes(page)
            for (node, metadata) in zip(page.nodes, bytes) {
                guard last.map({ $0 < node.id.rawValue }) ?? true,
                    try scalar(
                        "SELECT count(*) FROM rebaseline_nodes WHERE bootstrap_id=? AND node_id=? AND metadata=? AND kind=? AND lifecycle=? AND revision=? AND parent_id IS ?",
                        [
                            .text(id), .text(node.id.rawValue), .blob(metadata),
                            .text(node.kind.rawValue), .text(node.state.rawValue),
                            .text(node.revision.rawValue),
                            node.parentId.map { .text($0.rawValue) } ?? .null,
                        ]) == 1
                else { throw RebaselineFailure.protocolFailure }
                last = node.id.rawValue
                total += 1
            }
            cursor = page.nextCursor
        }
        guard total == saved.bootstrap.itemCount else { throw RebaselineFailure.countMismatch }
        let topology = try query(
            "SELECT node_id,parent_id,kind,lifecycle FROM rebaseline_nodes WHERE bootstrap_id=?",
            [.text(id)]
        ) { s in
            (
                try Self.text(s, 0),
                sqlite3_column_type(s, 1) == SQLITE_NULL ? nil : try Self.text(s, 1),
                try Self.text(s, 2), try Self.text(s, 3)
            )
        }
        let graph = Dictionary(uniqueKeysWithValues: topology.map { ($0.0, ($0.1, $0.2, $0.3)) })
        let root = saved.library.rootNodeId.rawValue
        guard graph[root]?.0 == nil, graph[root]?.1 == "DIRECTORY", graph[root]?.2 == "ACTIVE",
            topology.filter({ $0.1 == nil }).map({ $0.0 }) == [root]
        else { throw RebaselineFailure.invalidGraph }
        var resolved: Set<String> = [root]
        for row in topology {
            var path: Set<String> = [], next = row.0
            while !resolved.contains(next) {
                guard path.insert(next).inserted, let parent = graph[next]?.0,
                    graph[parent]?.1 == "DIRECTORY"
                else { throw RebaselineFailure.invalidGraph }
                next = parent
            }
            resolved.formUnion(path)
        }
        try requireFeedScope(saved.bootstrap.scope, credentialId: credentialId)
        guard let row = try snapshotRow(saved.bootstrap.scope), row.id == id, row.token == token
        else { throw RebaselineFailure.generationMismatch }
    }
    func prepareRebaseline(
        scope: ClientMutationScope, credentialId: String, owner: UUID,
        bridge: any RustBridgeProtocol
    ) async throws {
        guard
            let saved = try await rebaseline(
                scope: scope, credentialId: credentialId, bridge: bridge),
            saved.state == .terminalReceived
        else { throw RebaselineFailure.unverifiedManifest }
        try await verifySnapshot(saved, credentialId: credentialId, bridge: bridge)
        try snapshotTransaction(owner) {
            try requireFeedScope(scope, credentialId: credentialId)
            try requireSnapshotCompatibility(scope)
            guard try snapshotRow(scope)?.stage == .terminalReceived else {
                throw RebaselineFailure.unverifiedManifest
            }
            try execute(
                "UPDATE rebaseline_sessions SET prepared_id=?,stage='PREPARED_FOR_HANDOFF' WHERE bootstrap_id=?",
                [.text(UUID().uuidString), .text(saved.bootstrap.id.rawValue)])
            try fault?(.beforeSnapshotPreparationCommit)
        }
    }
    func claimRebaselineCompletion(
        scope: ClientMutationScope, credentialId: String, owner: UUID, recovering: Bool,
        bridge: any RustBridgeProtocol
    ) async throws -> RebaselineHandoff {
        guard
            let saved = try await rebaseline(
                scope: scope, credentialId: credentialId, bridge: bridge),
            recovering
                ? [.outcomeUnknown, .completionInFlight].contains(saved.state)
                : saved.state == .prepared,
            let prepared = saved.preparedId, let token = saved.terminalToken
        else { throw RebaselineFailure.unverifiedManifest }
        try await verifySnapshot(saved, credentialId: credentialId, bridge: bridge)
        return try snapshotTransaction(owner) {
            try requireFeedScope(scope, credentialId: credentialId)
            try requireSnapshotCompatibility(scope)
            guard let row = try snapshotRow(scope), row.prepared == prepared,
                row.stage == saved.state,
                try scalar(
                    "SELECT count(*) FROM rebaseline_completion_attempts WHERE bootstrap_id=?",
                    [.text(row.id)]) < 8
            else { throw RebaselineFailure.reconciliationRequired }
            let attempt = UUID().uuidString
            try checkSnapshotCapacity(additional: 32768)
            try execute(
                "INSERT INTO rebaseline_completion_attempts(bootstrap_id,attempt_id,owner,prepared_id) VALUES(?,?,?,?)",
                [.text(row.id), .text(attempt), .text(owner.uuidString), .text(prepared)])
            try execute(
                "UPDATE rebaseline_sessions SET stage='SERVER_COMPLETION_IN_FLIGHT' WHERE bootstrap_id=?",
                [.text(row.id)])
            return RebaselineHandoff(
                bootstrap: saved.bootstrap, credentialId: credentialId, preparedId: prepared,
                attemptId: attempt, token: token)
        }
    }
    func confirmRebaseline(
        _ result: RebaselineCompletionResult, handoff: RebaselineHandoff, owner: UUID
    ) throws {
        try snapshotTransaction(owner) {
            let scope = handoff.bootstrap.scope
            try requireFeedScope(scope, credentialId: handoff.credentialId)
            guard result.bootstrap.sameManifest(as: handoff.bootstrap),
                result.bootstrap.state == .completed,
                result.position == handoff.bootstrap.position,
                let row = try snapshotRow(scope), row.stage == .completionInFlight,
                row.prepared == handoff.preparedId, row.token == handoff.token,
                try scalar(
                    "SELECT count(*) FROM rebaseline_completion_attempts WHERE attempt_id=? AND owner=? AND prepared_id=? AND bootstrap_id=? AND completion IS NULL",
                    [
                        .text(handoff.attemptId), .text(owner.uuidString),
                        .text(handoff.preparedId), .text(row.id),
                    ]) == 1
            else { throw RebaselineFailure.unverifiedManifest }
            try execute(
                "UPDATE rebaseline_completion_attempts SET completion=? WHERE attempt_id=?",
                [.blob(result.responseBody), .text(handoff.attemptId)])
            try execute(
                "UPDATE rebaseline_sessions SET stage='SERVER_COMPLETION_CONFIRMED',completion=? WHERE bootstrap_id=?",
                [.blob(result.responseBody), .text(row.id)])
            try fault?(.beforeSnapshotConfirmationCommit)
        }
    }
    func unknownRebaseline(
        handoff: RebaselineHandoff, owner: UUID, reconciliationRequired: Bool = false
    ) throws {
        try snapshotTransaction(owner) {
            try execute(
                "UPDATE rebaseline_sessions SET stage=? WHERE bootstrap_id=? AND stage='SERVER_COMPLETION_IN_FLIGHT'",
                [
                    .text(reconciliationRequired ? "RECONCILIATION_REQUIRED" : "OUTCOME_UNKNOWN"),
                    .text(handoff.bootstrap.id.rawValue),
                ])
        }
    }
    func expireRebaseline(scope: ClientMutationScope, credentialId: String, owner: UUID) throws {
        try snapshotTransaction(owner) {
            try requireFeedScope(scope, credentialId: credentialId)
            guard let row = try snapshotRow(scope),
                [.bootstrapOpen, .downloading].contains(row.stage)
            else { throw RebaselineFailure.reconciliationRequired }
            try execute(
                "UPDATE rebaseline_sessions SET stage='EXPIRED' WHERE bootstrap_id=?",
                [.text(row.id)])
        }
    }
    func activateRebaseline(
        _ result: RebaselineCompletionResult, scope: ClientMutationScope, credentialId: String,
        owner: UUID, bridge: any RustBridgeProtocol
    ) async throws {
        guard
            let saved = try await rebaseline(
                scope: scope, credentialId: credentialId, bridge: bridge),
            saved.state == .completionConfirmed,
            saved.completion == result.responseBody, saved.preparedId != nil
        else { throw RebaselineFailure.unverifiedManifest }
        let verified = try await RebaselineResponseDecoder(bridge: bridge).completion(
            RebaselineCoordinator.response(result.responseBody), expected: saved.bootstrap)
        guard verified == result else { throw RebaselineFailure.protocolFailure }
        try await verifySnapshot(saved, credentialId: credentialId, bridge: bridge)
        try Task.checkCancellation()
        try snapshotTransaction(owner) {
            try requireFeedScope(scope, credentialId: credentialId)
            try requireSnapshotCompatibility(scope)
            guard let row = try snapshotRow(scope), row.stage == .completionConfirmed,
                row.completion == result.responseBody
            else { throw RebaselineFailure.unverifiedManifest }
            let scopeId = try requireScopeId(scope)
            // All old active membership is removed in this transaction; historical inbox/queue evidence is retained.
            try execute("DELETE FROM cached_nodes WHERE scope_id=?", [.integer(scopeId)])
            try execute(
                "INSERT INTO node_projection_state VALUES(?,?,?,?,?,'COMPLETE',1) ON CONFLICT(scope_id) DO UPDATE SET epoch=excluded.epoch,anchor_sequence=excluded.anchor_sequence,applied_sequence=excluded.applied_sequence,confirmed_sequence=excluded.confirmed_sequence,completeness='COMPLETE'",
                [
                    .integer(scopeId), .text(row.epoch), .text(row.cut), .text(row.cut),
                    .text(row.cut),
                ])
            try execute(
                "INSERT INTO cached_libraries VALUES(?,?,?,1) ON CONFLICT(scope_id,epoch) DO UPDATE SET metadata=excluded.metadata",
                [.integer(scopeId), .text(row.epoch), .blob(row.library)])
            try execute(
                "INSERT INTO rebaseline_active VALUES(?,?) ON CONFLICT(scope_id) DO UPDATE SET bootstrap_id=excluded.bootstrap_id",
                [.integer(scopeId), .text(row.id)])
            try execute(
                "INSERT INTO sync_bases VALUES(?,?,?,?,'VERIFIED') ON CONFLICT(scope_id) DO UPDATE SET epoch=excluded.epoch,sequence=excluded.sequence,response=excluded.response,status='VERIFIED'",
                [.integer(scopeId), .text(row.epoch), .text(row.cut), .blob(result.responseBody)])
            try execute(
                "UPDATE rebaseline_sessions SET stage='ACTIVE_COMPLETE' WHERE bootstrap_id=?",
                [.text(row.id)])
            try fault?(.beforeSnapshotActivationCommit)
        }
    }
}

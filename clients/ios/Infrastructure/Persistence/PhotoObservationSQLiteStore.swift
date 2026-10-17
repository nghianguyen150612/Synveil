import Foundation
import SQLite3

private let sqliteTransient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

enum PhotoObservationStoreFaultPoint: Sendable {
    case beforeObservationCommit
    case beforeExtractionTransitionCommit
}

public final class PhotoObservationSQLiteStore: PhotoObservationStoreProtocol, @unchecked Sendable {
    private let dbQueue: DispatchQueue
    private let authorizationBarrierLock = NSLock()
    private var db: OpaquePointer?
    private var authorizationWritesBlocked = false
    private var revokedSessionRevision: UInt64?
    private let fault: (@Sendable (PhotoObservationStoreFaultPoint) throws -> Void)?

    static func productionURL() throws -> URL {
        guard
            let root = FileManager.default.urls(
                for: .applicationSupportDirectory, in: .userDomainMask
            ).first
        else {
            throw NSError(
                domain: "PhotoObservationSQLiteStore", code: 99,
                userInfo: [NSLocalizedDescriptionKey: "Application Support is unavailable"])
        }
        return root.appendingPathComponent("Synveil/PhotoKit", isDirectory: true)
            .appendingPathComponent("photo-observations.sqlite")
    }

    init(
        databaseURL: URL,
        fault: (@Sendable (PhotoObservationStoreFaultPoint) throws -> Void)? = nil
    ) throws {
        self.dbQueue = DispatchQueue(label: "com.synveil.ios.photo.sqlite", qos: .utility)
        self.fault = fault
        try MutationQueueFilePolicy.prepare(databaseURL)
        var dbPointer: OpaquePointer?
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX
        if sqlite3_open_v2(databaseURL.path, &dbPointer, flags, nil) != SQLITE_OK {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 100,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to open SQLite database at \(databaseURL.path)"
                ]
            )
        }
        self.db = dbPointer
        try MutationQueueFilePolicy.protect(databaseURL)
        try dbQueue.sync {
            try PhotoObservationSQLiteSchema.createTables(db: self.db)
        }
    }

    deinit {
        if let db = db {
            sqlite3_close(db)
        }
    }

    public func activateAuthorization(_ authorization: PhotoAuthorizationSession) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performActivateAuthorization(authorization)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func revokeAuthorization(_ authorization: PhotoAuthorizationSession) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performRevokeAuthorization(authorization)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Called synchronously by the application logout/auth-rejection barrier before session state
    /// changes. Queue ordering drains any in-flight transaction before deactivating the token.
    @discardableResult
    func revokeCurrentAuthorizationSynchronously(
        throughSessionRevision: UInt64 = 0
    ) -> Bool {
        // Block newly submitted writes before waiting for the queue. If durable deactivation
        // itself fails, a write queued behind this barrier must not race the caller that marks
        // the failed revocation as blocked.
        authorizationBarrierLock.lock()
        authorizationWritesBlocked = true
        revokedSessionRevision = max(revokedSessionRevision ?? 0, throughSessionRevision)
        authorizationBarrierLock.unlock()
        do {
            try dbQueue.sync { try self.performDeactivateCurrentAuthorization() }
            return true
        } catch {
            return false
        }
    }

    public func commitObservations(
        _ observations: [PhotoAssetObservation], authorization: PhotoAuthorizationSession
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performSaveObservations(
                        observations,
                        authorization: authorization)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func createExtraction(
        _ intent: PhotoResourceExtractionIntent, stagingId: String
    ) async throws -> PhotoResourceExtractionRecord {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let record = try self.performCreateExtraction(intent, stagingId: stagingId)
                    continuation.resume(returning: record)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func transitionExtraction(
        _ record: PhotoResourceExtractionRecord,
        expectedState: PhotoResourceExtractionState,
        authorization: PhotoAuthorizationSession
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performTransitionExtraction(
                        record, expectedState: expectedState, authorization: authorization)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func extraction(
        operationId: UUID, authorization: PhotoAuthorizationSession
    ) async throws -> PhotoResourceExtractionRecord? {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    guard try self.performAuthorizationIsCurrent(authorization) else {
                        throw PhotoResourceTransferFailure.staleAuthorization
                    }
                    let record = try self.performFetchExtraction(
                        operationId: operationId, authorization: authorization)
                    continuation.resume(returning: record)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func extractions(
        authorization: PhotoAuthorizationSession
    ) async throws -> [PhotoResourceExtractionRecord] {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    guard try self.performAuthorizationIsCurrent(authorization) else {
                        throw PhotoResourceTransferFailure.staleAuthorization
                    }
                    let records = try self.performFetchExtractions(authorization: authorization)
                    continuation.resume(returning: records)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func extractionStagingIds() async throws -> Set<String> {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let ids = try self.performFetchExtractionStagingIds()
                    continuation.resume(returning: ids)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func fetchObservations(
        profileId: UUID,
        ownerId: String
    ) async throws -> [PhotoAssetObservation] {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let results = try self.performFetchObservations(
                        profileId: profileId,
                        ownerId: ownerId
                    )
                    continuation.resume(returning: results)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func markSourceUnavailable(
        localIdentifier: String,
        reason: PhotoSourceAvailability,
        profileId: UUID,
        ownerId: String
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performMarkSourceUnavailable(
                        localIdentifier: localIdentifier,
                        reason: reason,
                        profileId: profileId,
                        ownerId: ownerId
                    )
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func saveConfiguration(
        _ config: PhotoBackupConfiguration,
        profileId: UUID
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performSaveConfiguration(config, profileId: profileId)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func fetchConfiguration(profileId: UUID) async throws -> PhotoBackupConfiguration {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let config = try self.performFetchConfiguration(profileId: profileId)
                    continuation.resume(returning: config)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func clearObservations(profileId: UUID) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performClearObservations(profileId: profileId)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    // MARK: - Internal Database Actions

    private func performSaveObservations(
        _ observations: [PhotoAssetObservation],
        authorization: PhotoAuthorizationSession
    ) throws {
        let sql = """
            INSERT INTO photo_asset_observations (
                local_identifier, profile_id, owner_id, asset_kind,
                creation_date, modification_date, pixel_width, pixel_height,
                is_favorite, is_hidden, discovery_generation, observed_at,
                permission_scope, source_availability, is_excluded
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(local_identifier, profile_id) DO UPDATE SET
                owner_id=excluded.owner_id,
                asset_kind=excluded.asset_kind,
                creation_date=excluded.creation_date,
                modification_date=excluded.modification_date,
                pixel_width=excluded.pixel_width,
                pixel_height=excluded.pixel_height,
                is_favorite=excluded.is_favorite,
                is_hidden=excluded.is_hidden,
                discovery_generation=excluded.discovery_generation,
                observed_at=excluded.observed_at,
                permission_scope=excluded.permission_scope,
                source_availability=excluded.source_availability,
                is_excluded=excluded.is_excluded;
            """

        var statement: OpaquePointer?
        if sqlite3_prepare_v2(db, sql, -1, &statement, nil) != SQLITE_OK {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 101,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to prepare insert observation query"
                ]
            )
        }
        defer { sqlite3_finalize(statement) }

        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            guard try performAuthorizationIsCurrent(authorization) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }

            for obs in observations {
                sqlite3_reset(statement)

                let pIdStr = authorization.profileId.uuidString
                bindText(statement, index: 1, text: obs.localIdentifier)
                bindText(statement, index: 2, text: pIdStr)
                bindText(statement, index: 3, text: authorization.ownerId)
                bindText(statement, index: 4, text: obs.assetKind.rawValue)

                if let creation = obs.creationDate {
                    sqlite3_bind_double(statement, 5, creation.timeIntervalSince1970)
                } else {
                    sqlite3_bind_null(statement, 5)
                }

                if let modification = obs.modificationDate {
                    sqlite3_bind_double(statement, 6, modification.timeIntervalSince1970)
                } else {
                    sqlite3_bind_null(statement, 6)
                }

                sqlite3_bind_int(statement, 7, Int32(obs.pixelWidth))
                sqlite3_bind_int(statement, 8, Int32(obs.pixelHeight))
                sqlite3_bind_int(statement, 9, obs.isFavorite ? 1 : 0)
                sqlite3_bind_int(statement, 10, obs.isHidden ? 1 : 0)
                sqlite3_bind_int64(statement, 11, obs.discoveryGeneration)
                sqlite3_bind_double(statement, 12, obs.observedAt.timeIntervalSince1970)
                bindText(statement, index: 13, text: obs.permissionScope.rawValue)
                bindText(statement, index: 14, text: obs.sourceAvailability.rawValue)
                sqlite3_bind_int(statement, 15, obs.isExcluded ? 1 : 0)

                if sqlite3_step(statement) != SQLITE_DONE {
                    throw NSError(
                        domain: "PhotoObservationSQLiteStore",
                        code: 102,
                        userInfo: [
                            NSLocalizedDescriptionKey:
                                "Failed to execute observation insert"
                        ]
                    )
                }
            }
            try fault?(.beforeObservationCommit)
            try performSQL("COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performActivateAuthorization(_ authorization: PhotoAuthorizationSession) throws {
        authorizationBarrierLock.lock()
        let minimumAllowedRevision = revokedSessionRevision
        authorizationBarrierLock.unlock()
        guard minimumAllowedRevision.map({ authorization.sessionRevision > $0 }) ?? true else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }

        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            let sql = """
                INSERT INTO photo_authorization_session (
                    singleton, profile_id, owner_id, session_generation, is_active, updated_at
                ) VALUES (1, ?, ?, ?, 1, ?)
                ON CONFLICT(singleton) DO UPDATE SET
                    profile_id=excluded.profile_id,
                    owner_id=excluded.owner_id,
                    session_generation=excluded.session_generation,
                    is_active=1,
                    updated_at=excluded.updated_at;
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(120, "Failed to prepare authorization activation")
            }
            defer { sqlite3_finalize(statement) }
            bindText(statement, index: 1, text: authorization.profileId.uuidString)
            bindText(statement, index: 2, text: authorization.ownerId)
            bindText(statement, index: 3, text: authorization.generation.uuidString)
            sqlite3_bind_double(statement, 4, Date().timeIntervalSince1970)
            guard sqlite3_step(statement) == SQLITE_DONE else {
                throw photoStoreError(121, "Failed to activate authorization session")
            }
            try performSQL("COMMIT;")
            clearAuthorizationWriteBlock()
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performRevokeAuthorization(_ authorization: PhotoAuthorizationSession) throws {
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            let sql = """
                UPDATE photo_authorization_session SET is_active=0, updated_at=?
                WHERE singleton=1 AND profile_id=? AND owner_id=? AND session_generation=?;
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(122, "Failed to prepare authorization revocation")
            }
            defer { sqlite3_finalize(statement) }
            sqlite3_bind_double(statement, 1, Date().timeIntervalSince1970)
            bindText(statement, index: 2, text: authorization.profileId.uuidString)
            bindText(statement, index: 3, text: authorization.ownerId)
            bindText(statement, index: 4, text: authorization.generation.uuidString)
            guard sqlite3_step(statement) == SQLITE_DONE else {
                throw photoStoreError(123, "Failed to revoke authorization session")
            }
            try performSQL("COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performDeactivateCurrentAuthorization() throws {
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            try performSQL(
                "UPDATE photo_authorization_session SET is_active=0, updated_at="
                    + "\(Date().timeIntervalSince1970) WHERE singleton=1;")
            try performSQL("COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performAuthorizationIsCurrent(_ authorization: PhotoAuthorizationSession) throws
        -> Bool
    {
        authorizationBarrierLock.lock()
        let writesBlocked = authorizationWritesBlocked
        authorizationBarrierLock.unlock()
        guard !writesBlocked else { return false }
        let sql = """
            SELECT is_active FROM photo_authorization_session
            WHERE singleton=1 AND profile_id=? AND owner_id=? AND session_generation=?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(124, "Failed to prepare authorization check")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: authorization.generation.uuidString)
        guard sqlite3_step(statement) == SQLITE_ROW else { return false }
        return sqlite3_column_int(statement, 0) == 1
    }

    private func performCreateExtraction(
        _ intent: PhotoResourceExtractionIntent, stagingId: String
    ) throws -> PhotoResourceExtractionRecord {
        guard intent.authorization.profileId == intent.profileId,
            intent.authorization.ownerId == intent.ownerId,
            intent.userInitiated,
            intent.policyVersion > 0,
            intent.maximumResourceBytes > 0,
            intent.maximumResourceBytes <= UInt64(Int64.max),
            intent.selection.resourceIndex >= 0,
            UUID(uuidString: stagingId)?.uuidString.lowercased() == stagingId
        else { throw PhotoResourceTransferFailure.staleAuthorization }
        let now = Date()
        let record = PhotoResourceExtractionRecord(
            operationId: intent.operationId,
            profileId: intent.profileId,
            ownerId: intent.ownerId,
            deviceId: intent.deviceId,
            selection: intent.selection,
            observationGeneration: intent.observationGeneration,
            authorizationGeneration: intent.authorization.generation,
            policyVersion: intent.policyVersion,
            maximumResourceBytes: intent.maximumResourceBytes,
            userInitiated: intent.userInitiated,
            stagingId: stagingId,
            state: .discovered,
            createdAt: intent.createdAt,
            updatedAt: now)
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            guard try performAuthorizationIsCurrent(intent.authorization) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }
            try ensureNoUnresolvedExtraction(for: intent)
            let sql = """
                INSERT INTO photo_resource_extractions (
                    operation_id, profile_id, owner_id, device_id, local_identifier,
                    resource_index, resource_type, resource_role, rendition,
                    uniform_type_identifier, source_modification_date, observation_generation,
                    policy_version, maximum_resource_bytes, user_initiated,
                    authorization_generation, staging_id, byte_count, sha256, state,
                    related_upload_operation_id, failure_code, created_at, updated_at
                ) VALUES (
                    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    NULL, NULL, ?, NULL, NULL, ?, ?
                );
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(125, "Failed to prepare extraction creation")
            }
            defer { sqlite3_finalize(statement) }
            bindExtraction(record, to: statement)
            guard sqlite3_step(statement) == SQLITE_DONE else {
                throw photoStoreError(126, "Failed to persist extraction intent")
            }
            try performSQL("COMMIT;")
            return record
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performTransitionExtraction(
        _ record: PhotoResourceExtractionRecord,
        expectedState: PhotoResourceExtractionState,
        authorization: PhotoAuthorizationSession
    ) throws {
        let requiresContent = [
            PhotoResourceExtractionState.verifiedReady,
            .handoffPending,
            .handedOff,
            .uploadedConfirmed,
        ].contains(record.state)
        guard record.profileId == authorization.profileId,
            record.ownerId == authorization.ownerId,
            record.userInitiated, record.policyVersion > 0,
            record.maximumResourceBytes > 0,
            allowedExtractionTransition(from: expectedState, to: record.state),
            !requiresContent
                || (record.byteCount ?? 0) > 0
                    && (record.byteCount ?? UInt64.max) <= record.maximumResourceBytes
                    && isValidSHA256(record.sha256),
            ![.handoffPending, .handedOff, .uploadedConfirmed].contains(record.state)
                || validUploadOperationId(record.relatedUploadOperationId)
        else { throw PhotoResourceTransferFailure.storageFailure }

        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            guard try performAuthorizationIsCurrent(authorization) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }
            guard
                let existing = try performFetchExtraction(
                    operationId: record.operationId, authorization: authorization),
                existing.state == expectedState, sameExtractionIdentity(existing, record)
            else { throw PhotoResourceTransferFailure.storageFailure }
            if [.verifiedReady, .handoffPending].contains(record.state) {
                guard try performExtractionConsentIsCurrent(record),
                    try performExtractionObservationIsCurrent(record)
                else { throw PhotoResourceTransferFailure.configurationDisabled }
            }
            let sql = """
                UPDATE photo_resource_extractions SET
                    byte_count=?, sha256=?, state=?, related_upload_operation_id=?,
                    failure_code=?, updated_at=?
                WHERE operation_id=? AND profile_id=? AND owner_id=?
                    AND authorization_generation=? AND state=?;
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(127, "Failed to prepare extraction transition")
            }
            defer { sqlite3_finalize(statement) }
            if let byteCount = record.byteCount {
                guard byteCount <= UInt64(Int64.max) else {
                    throw PhotoResourceTransferFailure.resourceTooLarge
                }
                sqlite3_bind_int64(statement, 1, Int64(byteCount))
            } else {
                sqlite3_bind_null(statement, 1)
            }
            bindOptionalText(statement, index: 2, text: record.sha256)
            bindText(statement, index: 3, text: record.state.rawValue)
            bindOptionalText(statement, index: 4, text: record.relatedUploadOperationId)
            bindOptionalText(statement, index: 5, text: record.failureCode)
            sqlite3_bind_double(statement, 6, record.updatedAt.timeIntervalSince1970)
            bindText(statement, index: 7, text: record.operationId.uuidString)
            bindText(statement, index: 8, text: record.profileId.uuidString)
            bindText(statement, index: 9, text: record.ownerId)
            bindText(statement, index: 10, text: record.authorizationGeneration.uuidString)
            bindText(statement, index: 11, text: expectedState.rawValue)
            guard sqlite3_step(statement) == SQLITE_DONE, sqlite3_changes(db) == 1 else {
                throw photoStoreError(128, "Extraction state changed before durable commit")
            }
            try fault?(.beforeExtractionTransitionCommit)
            try performSQL("COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performFetchExtraction(
        operationId: UUID, authorization: PhotoAuthorizationSession
    ) throws -> PhotoResourceExtractionRecord? {
        let sql = """
            SELECT operation_id, profile_id, owner_id, device_id, local_identifier,
                resource_index, resource_type, resource_role, rendition,
                uniform_type_identifier, source_modification_date, observation_generation,
                policy_version, maximum_resource_bytes, user_initiated,
                authorization_generation, staging_id, byte_count, sha256, state,
                related_upload_operation_id, failure_code, created_at, updated_at
            FROM photo_resource_extractions
            WHERE operation_id=? AND profile_id=? AND owner_id=?
                AND authorization_generation=?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(129, "Failed to prepare extraction read")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: operationId.uuidString)
        bindText(statement, index: 2, text: authorization.profileId.uuidString)
        bindText(statement, index: 3, text: authorization.ownerId)
        bindText(statement, index: 4, text: authorization.generation.uuidString)
        guard sqlite3_step(statement) == SQLITE_ROW else { return nil }
        return try decodeExtraction(statement)
    }

    private func performFetchExtractionStagingIds() throws -> Set<String> {
        let sql = """
            SELECT staging_id FROM photo_resource_extractions
            WHERE state IN ('verifiedReady', 'handoffPending')
                AND authorization_generation=(
                    SELECT session_generation FROM photo_authorization_session
                    WHERE singleton=1 AND is_active=1
                );
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(130, "Failed to prepare extraction staging inventory")
        }
        defer { sqlite3_finalize(statement) }
        var result = Set<String>()
        while sqlite3_step(statement) == SQLITE_ROW {
            guard let value = sqlite3_column_text(statement, 0) else { continue }
            result.insert(String(cString: value))
        }
        return result
    }

    private func performFetchExtractions(
        authorization: PhotoAuthorizationSession
    ) throws -> [PhotoResourceExtractionRecord] {
        let sql = """
            SELECT operation_id, profile_id, owner_id, device_id, local_identifier,
                resource_index, resource_type, resource_role, rendition,
                uniform_type_identifier, source_modification_date, observation_generation,
                policy_version, maximum_resource_bytes, user_initiated,
                authorization_generation, staging_id, byte_count, sha256, state,
                related_upload_operation_id, failure_code, created_at, updated_at
            FROM photo_resource_extractions
            WHERE profile_id=? AND owner_id=? AND authorization_generation=?
            ORDER BY created_at DESC;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(136, "Failed to prepare extraction inventory")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: authorization.generation.uuidString)
        var records: [PhotoResourceExtractionRecord] = []
        while sqlite3_step(statement) == SQLITE_ROW {
            records.append(try decodeExtraction(statement))
        }
        return records
    }

    private func bindExtraction(
        _ record: PhotoResourceExtractionRecord, to statement: OpaquePointer?
    ) {
        bindText(statement, index: 1, text: record.operationId.uuidString)
        bindText(statement, index: 2, text: record.profileId.uuidString)
        bindText(statement, index: 3, text: record.ownerId)
        bindText(statement, index: 4, text: record.deviceId)
        bindText(statement, index: 5, text: record.selection.localIdentifier)
        sqlite3_bind_int64(statement, 6, Int64(record.selection.resourceIndex))
        bindText(statement, index: 7, text: record.selection.resourceType)
        bindText(statement, index: 8, text: record.selection.role.rawValue)
        bindText(statement, index: 9, text: record.selection.rendition.rawValue)
        bindOptionalText(statement, index: 10, text: record.selection.uniformTypeIdentifier)
        if let date = record.selection.sourceModificationDate {
            sqlite3_bind_double(statement, 11, date.timeIntervalSince1970)
        } else {
            sqlite3_bind_null(statement, 11)
        }
        sqlite3_bind_int64(statement, 12, record.observationGeneration)
        sqlite3_bind_int64(statement, 13, Int64(record.policyVersion))
        sqlite3_bind_int64(statement, 14, Int64(record.maximumResourceBytes))
        sqlite3_bind_int(statement, 15, record.userInitiated ? 1 : 0)
        bindText(statement, index: 16, text: record.authorizationGeneration.uuidString)
        bindText(statement, index: 17, text: record.stagingId)
        bindText(statement, index: 18, text: record.state.rawValue)
        sqlite3_bind_double(statement, 19, record.createdAt.timeIntervalSince1970)
        sqlite3_bind_double(statement, 20, record.updatedAt.timeIntervalSince1970)
    }

    private func decodeExtraction(_ statement: OpaquePointer?) throws
        -> PhotoResourceExtractionRecord
    {
        func string(_ index: Int32) throws -> String {
            guard let value = sqlite3_column_text(statement, index) else {
                throw photoStoreError(131, "Corrupt PhotoKit extraction row")
            }
            return String(cString: value)
        }
        guard let operationId = UUID(uuidString: try string(0)),
            let profileId = UUID(uuidString: try string(1)),
            let authorizationGeneration = UUID(uuidString: try string(15)),
            let role = PhotoResourceRole(rawValue: try string(7)),
            let rendition = PhotoResourceRendition(rawValue: try string(8)),
            let state = PhotoResourceExtractionState(rawValue: try string(19))
        else { throw photoStoreError(132, "Invalid PhotoKit extraction identity") }
        let byteCount: UInt64?
        if sqlite3_column_type(statement, 17) == SQLITE_NULL {
            byteCount = nil
        } else {
            let value = sqlite3_column_int64(statement, 17)
            guard value >= 0 else { throw photoStoreError(133, "Negative extraction length") }
            byteCount = UInt64(value)
        }
        let sourceModificationDate =
            sqlite3_column_type(statement, 10) == SQLITE_NULL
            ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 10))
        let maximumResourceBytes = sqlite3_column_int64(statement, 13)
        let policyVersion = sqlite3_column_int64(statement, 12)
        let resourceIndex = sqlite3_column_int64(statement, 5)
        let userInitiated = sqlite3_column_int(statement, 14) == 1
        guard maximumResourceBytes > 0, policyVersion > 0, resourceIndex >= 0,
            userInitiated
        else {
            throw photoStoreError(137, "Invalid stored PhotoKit extraction policy")
        }
        let selection = PhotoResourceSelection(
            localIdentifier: try string(4),
            resourceIndex: Int(resourceIndex),
            resourceType: try string(6),
            role: role,
            rendition: rendition,
            uniformTypeIdentifier: nullableString(statement, 9),
            sourceModificationDate: sourceModificationDate)
        return PhotoResourceExtractionRecord(
            operationId: operationId,
            profileId: profileId,
            ownerId: try string(2),
            deviceId: try string(3),
            selection: selection,
            observationGeneration: sqlite3_column_int64(statement, 11),
            authorizationGeneration: authorizationGeneration,
            policyVersion: Int(policyVersion),
            maximumResourceBytes: UInt64(maximumResourceBytes),
            userInitiated: userInitiated,
            stagingId: try string(16),
            byteCount: byteCount,
            sha256: nullableString(statement, 18),
            state: state,
            relatedUploadOperationId: nullableString(statement, 20),
            failureCode: nullableString(statement, 21),
            createdAt: Date(timeIntervalSince1970: sqlite3_column_double(statement, 22)),
            updatedAt: Date(timeIntervalSince1970: sqlite3_column_double(statement, 23)))
    }

    private func allowedExtractionTransition(
        from previous: PhotoResourceExtractionState, to next: PhotoResourceExtractionState
    ) -> Bool {
        let allowed: [PhotoResourceExtractionState: Set<PhotoResourceExtractionState>] = [
            .discovered: [
                .eligible, .cancelled, .sourceUnavailable, .permissionRevoked,
                .failedRecoverable, .failedTerminal,
            ],
            .eligible: [
                .extracting, .cancelled, .sourceUnavailable, .permissionRevoked,
                .failedRecoverable, .failedTerminal,
            ],
            .extracting: [
                .verifying, .cancelled, .sourceUnavailable, .permissionRevoked,
                .failedRecoverable, .failedTerminal,
            ],
            .verifying: [
                .verifiedReady, .cancelled, .sourceUnavailable, .permissionRevoked,
                .failedRecoverable, .failedTerminal,
            ],
            .verifiedReady: [.handoffPending, .cancelled, .permissionRevoked, .failedTerminal],
            .handoffPending: [
                .handedOff, .uploadedConfirmed, .permissionRevoked,
                .failedRecoverable, .failedTerminal,
            ],
            .handedOff: [
                .uploadedConfirmed, .permissionRevoked, .failedRecoverable,
                .failedTerminal,
            ],
            .failedRecoverable: [.eligible, .cancelled, .permissionRevoked, .failedTerminal],
        ]
        return allowed[previous]?.contains(next) == true
    }

    private func sameExtractionIdentity(
        _ lhs: PhotoResourceExtractionRecord, _ rhs: PhotoResourceExtractionRecord
    ) -> Bool {
        lhs.operationId == rhs.operationId
            && lhs.profileId == rhs.profileId
            && lhs.ownerId == rhs.ownerId
            && lhs.deviceId == rhs.deviceId
            && lhs.selection == rhs.selection
            && lhs.observationGeneration == rhs.observationGeneration
            && lhs.authorizationGeneration == rhs.authorizationGeneration
            && lhs.policyVersion == rhs.policyVersion
            && lhs.maximumResourceBytes == rhs.maximumResourceBytes
            && lhs.userInitiated == rhs.userInitiated
            && lhs.stagingId == rhs.stagingId
            && lhs.createdAt == rhs.createdAt
    }

    private func performExtractionConsentIsCurrent(
        _ record: PhotoResourceExtractionRecord
    ) throws -> Bool {
        let sql = """
            SELECT is_opted_in, include_videos, pause_discovery
            FROM photo_backup_configurations WHERE profile_id=?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(140, "Failed to prepare PhotoKit consent check")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: record.profileId.uuidString)
        guard sqlite3_step(statement) == SQLITE_ROW,
            sqlite3_column_int(statement, 0) == 1,
            sqlite3_column_int(statement, 2) == 0
        else { return false }
        let includesVideos = sqlite3_column_int(statement, 1) == 1
        return includesVideos || ![.primaryVideo, .motionVideo].contains(record.selection.role)
    }

    private func performExtractionObservationIsCurrent(
        _ record: PhotoResourceExtractionRecord
    ) throws -> Bool {
        let sql = """
            SELECT 1 FROM photo_asset_observations
            WHERE local_identifier=? AND profile_id=? AND owner_id=?
                AND discovery_generation=? AND source_availability='available'
                AND is_excluded=0 LIMIT 1;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(141, "Failed to prepare PhotoKit observation fence")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: record.selection.localIdentifier)
        bindText(statement, index: 2, text: record.profileId.uuidString)
        bindText(statement, index: 3, text: record.ownerId)
        sqlite3_bind_int64(statement, 4, record.observationGeneration)
        return sqlite3_step(statement) == SQLITE_ROW
    }

    private func ensureNoUnresolvedExtraction(
        for intent: PhotoResourceExtractionIntent
    ) throws {
        let sql = """
            SELECT 1 FROM photo_resource_extractions
            WHERE profile_id=? AND owner_id=? AND device_id=? AND local_identifier=?
                AND resource_index=? AND resource_type=? AND resource_role=? AND rendition=?
                AND uniform_type_identifier IS ? AND source_modification_date IS ?
                AND authorization_generation=?
                AND state IN (
                    'discovered', 'eligible', 'extracting', 'verifying',
                    'verifiedReady', 'handoffPending', 'handedOff'
                )
            LIMIT 1;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(138, "Failed to prepare duplicate PhotoKit operation check")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: intent.profileId.uuidString)
        bindText(statement, index: 2, text: intent.ownerId)
        bindText(statement, index: 3, text: intent.deviceId)
        bindText(statement, index: 4, text: intent.selection.localIdentifier)
        sqlite3_bind_int64(statement, 5, Int64(intent.selection.resourceIndex))
        bindText(statement, index: 6, text: intent.selection.resourceType)
        bindText(statement, index: 7, text: intent.selection.role.rawValue)
        bindText(statement, index: 8, text: intent.selection.rendition.rawValue)
        bindOptionalText(statement, index: 9, text: intent.selection.uniformTypeIdentifier)
        if let date = intent.selection.sourceModificationDate {
            sqlite3_bind_double(statement, 10, date.timeIntervalSince1970)
        } else {
            sqlite3_bind_null(statement, 10)
        }
        bindText(statement, index: 11, text: intent.authorization.generation.uuidString)
        let step = sqlite3_step(statement)
        guard step == SQLITE_DONE || step == SQLITE_ROW else {
            throw photoStoreError(139, "Failed to inspect prior PhotoKit operation")
        }
        guard step != SQLITE_ROW else {
            throw PhotoResourceTransferFailure.existingUnresolvedOperation
        }
    }

    private func isValidSHA256(_ value: String?) -> Bool {
        guard let value else { return false }
        return value.range(of: "^sha256:[0-9a-f]{64}$", options: .regularExpression) != nil
    }

    private func validUploadOperationId(_ value: String?) -> Bool {
        guard let value else { return false }
        return UUID(uuidString: value)?.uuidString.lowercased() == value
    }

    private func nullableString(_ statement: OpaquePointer?, _ index: Int32) -> String? {
        guard sqlite3_column_type(statement, index) != SQLITE_NULL,
            let value = sqlite3_column_text(statement, index)
        else { return nil }
        return String(cString: value)
    }

    private func bindOptionalText(_ statement: OpaquePointer?, index: Int32, text: String?) {
        if let text {
            bindText(statement, index: index, text: text)
        } else {
            sqlite3_bind_null(statement, index)
        }
    }

    private func performSQL(_ sql: String) throws {
        guard let db else { throw photoStoreError(134, "PhotoKit database is closed") }
        var message: UnsafeMutablePointer<CChar>?
        guard sqlite3_exec(db, sql, nil, nil, &message) == SQLITE_OK else {
            let detail = message.map { String(cString: $0) } ?? "SQLite operation failed"
            sqlite3_free(message)
            throw photoStoreError(135, detail)
        }
    }

    private func photoStoreError(_ code: Int, _ message: String) -> NSError {
        NSError(
            domain: "PhotoObservationSQLiteStore", code: code,
            userInfo: [NSLocalizedDescriptionKey: message])
    }

    private func clearAuthorizationWriteBlock() {
        authorizationBarrierLock.lock()
        authorizationWritesBlocked = false
        authorizationBarrierLock.unlock()
    }

    private func performFetchObservations(
        profileId: UUID,
        ownerId: String
    ) throws -> [PhotoAssetObservation] {
        let sql = """
            SELECT local_identifier, asset_kind, creation_date, modification_date, \
            pixel_width, pixel_height, is_favorite, is_hidden, discovery_generation, \
            observed_at, permission_scope, source_availability, is_excluded \
            FROM photo_asset_observations WHERE profile_id=? AND owner_id=?;
            """
        var statement: OpaquePointer?
        if sqlite3_prepare_v2(db, sql, -1, &statement, nil) != SQLITE_OK {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 103,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to prepare fetch observations query"
                ]
            )
        }
        defer { sqlite3_finalize(statement) }

        let pIdStr = profileId.uuidString
        bindText(statement, index: 1, text: pIdStr)
        bindText(statement, index: 2, text: ownerId)

        var observations: [PhotoAssetObservation] = []
        while sqlite3_step(statement) == SQLITE_ROW {
            guard let locIdC = sqlite3_column_text(statement, 0),
                let kindC = sqlite3_column_text(statement, 1),
                let scopeC = sqlite3_column_text(statement, 10),
                let sourceAvailC = sqlite3_column_text(statement, 11)
            else {
                continue
            }

            let localIdentifier = String(cString: locIdC)
            let kind = PhotoAssetKind(rawValue: String(cString: kindC)) ?? .unknown
            let scope =
                PhotoPermissionState(rawValue: String(cString: scopeC)) ?? .unknownFutureStatus
            let sourceAvail =
                PhotoSourceAvailability(rawValue: String(cString: sourceAvailC))
                ?? .sourceUnavailable

            let creationDate: Date? =
                sqlite3_column_type(statement, 2) != SQLITE_NULL
                ? Date(timeIntervalSince1970: sqlite3_column_double(statement, 2)) : nil
            let modificationDate: Date? =
                sqlite3_column_type(statement, 3) != SQLITE_NULL
                ? Date(timeIntervalSince1970: sqlite3_column_double(statement, 3)) : nil

            let pixelWidth = Int(sqlite3_column_int(statement, 4))
            let pixelHeight = Int(sqlite3_column_int(statement, 5))
            let isFavorite = sqlite3_column_int(statement, 6) != 0
            let isHidden = sqlite3_column_int(statement, 7) != 0
            let generation = sqlite3_column_int64(statement, 8)
            let observedAt = Date(timeIntervalSince1970: sqlite3_column_double(statement, 9))
            let isExcluded = sqlite3_column_int(statement, 12) != 0

            let obs = PhotoAssetObservation(
                localIdentifier: localIdentifier,
                assetKind: kind,
                creationDate: creationDate,
                modificationDate: modificationDate,
                pixelWidth: pixelWidth,
                pixelHeight: pixelHeight,
                isFavorite: isFavorite,
                isHidden: isHidden,
                discoveryGeneration: generation,
                observedAt: observedAt,
                permissionScope: scope,
                sourceAvailability: sourceAvail,
                isExcluded: isExcluded
            )
            observations.append(obs)
        }

        return observations
    }

    private func performMarkSourceUnavailable(
        localIdentifier: String,
        reason: PhotoSourceAvailability,
        profileId: UUID,
        ownerId: String
    ) throws {
        let sql = """
            UPDATE photo_asset_observations SET source_availability=? \
            WHERE local_identifier=? AND profile_id=? AND owner_id=?;
            """
        var statement: OpaquePointer?
        if sqlite3_prepare_v2(db, sql, -1, &statement, nil) != SQLITE_OK {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 104,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to prepare mark unavailable query"
                ]
            )
        }
        defer { sqlite3_finalize(statement) }

        bindText(statement, index: 1, text: reason.rawValue)
        bindText(statement, index: 2, text: localIdentifier)
        bindText(statement, index: 3, text: profileId.uuidString)
        bindText(statement, index: 4, text: ownerId)

        if sqlite3_step(statement) != SQLITE_DONE {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 105,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to update source availability"
                ]
            )
        }
    }

    private func performSaveConfiguration(
        _ config: PhotoBackupConfiguration,
        profileId: UUID
    ) throws {
        let sql = """
            INSERT INTO photo_backup_configurations (
                profile_id, is_opted_in, scope, include_videos, pause_discovery, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(profile_id) DO UPDATE SET
                is_opted_in=excluded.is_opted_in,
                scope=excluded.scope,
                include_videos=excluded.include_videos,
                pause_discovery=excluded.pause_discovery,
                updated_at=excluded.updated_at;
            """
        var statement: OpaquePointer?
        if sqlite3_prepare_v2(db, sql, -1, &statement, nil) != SQLITE_OK {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 106,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to prepare save config query"
                ]
            )
        }
        defer { sqlite3_finalize(statement) }

        bindText(statement, index: 1, text: profileId.uuidString)
        sqlite3_bind_int(statement, 2, config.isOptedIn ? 1 : 0)
        bindText(statement, index: 3, text: config.scope.rawValue)
        sqlite3_bind_int(statement, 4, config.includeVideos ? 1 : 0)
        sqlite3_bind_int(statement, 5, config.pauseDiscovery ? 1 : 0)
        sqlite3_bind_double(statement, 6, Date().timeIntervalSince1970)

        if sqlite3_step(statement) != SQLITE_DONE {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 107,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to save configuration"
                ]
            )
        }
    }

    private func performFetchConfiguration(profileId: UUID) throws -> PhotoBackupConfiguration {
        let sql = """
            SELECT is_opted_in, scope, include_videos, pause_discovery \
            FROM photo_backup_configurations WHERE profile_id=?;
            """
        var statement: OpaquePointer?
        if sqlite3_prepare_v2(db, sql, -1, &statement, nil) != SQLITE_OK {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 108,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to prepare fetch config query"
                ]
            )
        }
        defer { sqlite3_finalize(statement) }

        bindText(statement, index: 1, text: profileId.uuidString)

        if sqlite3_step(statement) == SQLITE_ROW {
            let isOptedIn = sqlite3_column_int(statement, 0) != 0
            let scopeRaw = String(cString: sqlite3_column_text(statement, 1))
            let scope = PhotoBackupScope(rawValue: scopeRaw) ?? .allAuthorized
            let includeVideos = sqlite3_column_int(statement, 2) != 0
            let pauseDiscovery = sqlite3_column_int(statement, 3) != 0

            return PhotoBackupConfiguration(
                isOptedIn: isOptedIn,
                scope: scope,
                includeVideos: includeVideos,
                pauseDiscovery: pauseDiscovery
            )
        } else {
            return PhotoBackupConfiguration()
        }
    }

    private func performClearObservations(profileId: UUID) throws {
        let sql = "DELETE FROM photo_asset_observations WHERE profile_id=?;"
        var statement: OpaquePointer?
        if sqlite3_prepare_v2(db, sql, -1, &statement, nil) != SQLITE_OK {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 109,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to prepare clear observations query"
                ]
            )
        }
        defer { sqlite3_finalize(statement) }

        bindText(statement, index: 1, text: profileId.uuidString)

        if sqlite3_step(statement) != SQLITE_DONE {
            throw NSError(
                domain: "PhotoObservationSQLiteStore",
                code: 110,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "Failed to clear observations"
                ]
            )
        }
    }

    private func bindText(_ statement: OpaquePointer?, index: Int32, text: String) {
        _ = text.withCString { cString in
            sqlite3_bind_text(statement, index, cString, -1, sqliteTransient)
        }
    }
}

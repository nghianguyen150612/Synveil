import Foundation
import SQLite3

private let sqliteTransient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

enum PhotoObservationStoreFaultPoint: Sendable {
    case beforeObservationCommit
    case beforeExtractionTransitionCommit
}

public final class PhotoObservationSQLiteStore: PhotoObservationStoreProtocol,
    PhotoBackupQueueStoreProtocol, @unchecked Sendable
{
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

    public func enqueueBackupCandidates(
        _ candidates: [PhotoBackupCandidate], authorization: PhotoAuthorizationSession,
        maximumPending: Int
    ) async throws -> Int {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let count = try self.performEnqueueBackupCandidates(
                        candidates, authorization: authorization,
                        maximumPending: min(maximumPending, 1_000))
                    continuation.resume(returning: count)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func backupDiscoveryCursor(
        authorization: PhotoAuthorizationSession, deviceId: String, policyRevision: Int64,
        scanGeneration: Int64
    ) async throws -> String? {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let cursor = try self.performBackupDiscoveryCursor(
                        authorization: authorization, deviceId: deviceId,
                        policyRevision: policyRevision, scanGeneration: scanGeneration)
                    continuation.resume(returning: cursor)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func saveBackupDiscoveryCursor(
        _ localIdentifier: String?, authorization: PhotoAuthorizationSession,
        deviceId: String, policyRevision: Int64, scanGeneration: Int64
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performSaveBackupDiscoveryCursor(
                        localIdentifier, authorization: authorization, deviceId: deviceId,
                        policyRevision: policyRevision, scanGeneration: scanGeneration)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func claimNextBackupCandidate(
        authorization: PhotoAuthorizationSession, deviceId: String, claimId: UUID,
        leaseUntil: Date, policyRevision: Int64
    ) async throws -> PhotoBackupCandidate? {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let candidate = try self.performClaimNextBackupCandidate(
                        authorization: authorization, deviceId: deviceId, claimId: claimId,
                        leaseUntil: leaseUntil, policyRevision: policyRevision)
                    continuation.resume(returning: candidate)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func transitionBackupCandidate(
        _ candidate: PhotoBackupCandidate, expectedState: PhotoBackupQueueState,
        authorization: PhotoAuthorizationSession
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performTransitionBackupCandidate(
                        candidate, expectedState: expectedState, authorization: authorization)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func backupCandidates(
        authorization: PhotoAuthorizationSession, limit: Int
    ) async throws -> [PhotoBackupCandidate] {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let candidates = try self.performFetchBackupCandidates(
                        authorization: authorization, limit: min(max(limit, 1), 1_000))
                    continuation.resume(returning: candidates)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func adoptRecoverableBackupCandidates(
        authorization: PhotoAuthorizationSession, deviceId: String
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performAdoptRecoverableBackupCandidates(
                        authorization: authorization, deviceId: deviceId)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func backupQueueSummary(
        authorization: PhotoAuthorizationSession
    ) async throws -> PhotoBackupQueueSummary {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let summary = try self.performBackupQueueSummary(authorization: authorization)
                    continuation.resume(returning: summary)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func recoverExpiredBackupClaims(
        authorization: PhotoAuthorizationSession, before: Date
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performRecoverExpiredBackupClaims(
                        authorization: authorization, before: before)
                    continuation.resume()
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

    public func fetchObservationPage(
        profileId: UUID, ownerId: String, scanGeneration: Int64,
        after localIdentifier: String?, limit: Int
    ) async throws -> [PhotoAssetObservation] {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    let results = try self.performFetchObservationPage(
                        profileId: profileId, ownerId: ownerId,
                        scanGeneration: scanGeneration, after: localIdentifier,
                        limit: min(max(limit, 1), 1_000))
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

    private func performEnqueueBackupCandidates(
        _ candidates: [PhotoBackupCandidate], authorization: PhotoAuthorizationSession,
        maximumPending: Int
    ) throws -> Int {
        guard maximumPending > 0, candidates.count <= 10_000 else {
            throw photoStoreError(150, "Photo Backup queue batch is outside its bound")
        }
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            guard try performAuthorizationIsCurrent(authorization) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }
            let config = try performFetchConfiguration(profileId: authorization.profileId)
            guard config.canAutomaticallyBackUpInForeground else {
                throw PhotoResourceTransferFailure.configurationDisabled
            }
            guard let first = candidates.first else {
                try performSQL("COMMIT;")
                return 0
            }
            guard candidates.allSatisfy({ $0.deviceId == first.deviceId }) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }
            try performPruneTerminalBackupCandidates(
                authorization: authorization, deviceId: first.deviceId,
                olderThan: Date().addingTimeInterval(-90 * 24 * 60 * 60))
            var totalRows = try performBackupQueueTotalCount(
                authorization: authorization, deviceId: first.deviceId)
            var pending = try performBackupQueuePendingCount(
                authorization: authorization, deviceId: first.deviceId)
            let sql = """
                INSERT INTO photo_backup_candidates (
                    candidate_id, profile_id, owner_id, device_id, client_library_scope,
                    local_identifier, source_revision, resource_index, resource_type,
                    resource_role, rendition, uniform_type_identifier,
                    source_modification_date, observation_generation, policy_revision,
                    authorization_generation, destination_library_identifier,
                    destination_parent_identifier, group_identity, state, attempt_count,
                    retry_after, claim_id,
                    claim_expires_at, claim_previous_state, extraction_operation_id,
                    upload_operation_id, file_version_identifier, verified_byte_count,
                    verified_sha256, photo_asset_identifier, backup_snapshot_identifier,
                    backup_retention_confirmed_at, safe_failure, created_at, updated_at
                ) VALUES (
                    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
                )
                ON CONFLICT DO NOTHING;
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(151, "Failed to prepare Photo Backup queue insertion")
            }
            defer { sqlite3_finalize(statement) }

            var inserted = 0
            for candidate in candidates {
                guard candidate.profileId == authorization.profileId,
                    candidate.ownerId == authorization.ownerId,
                    candidate.authorizationGeneration == authorization.generation,
                    candidate.localIdentifier == candidate.selection.localIdentifier,
                    candidate.selection.resourceIndex >= 0,
                    candidate.deviceId.isEmpty == false,
                    candidate.clientLibraryScope.isEmpty == false,
                    candidate.destinationLibraryIdentifier.isEmpty == false,
                    candidate.destinationParentIdentifier.isEmpty == false,
                    candidate.policyRevision == config.policyRevision
                else { throw PhotoResourceTransferFailure.staleAuthorization }
                guard config.targetLibraryIdentifier == candidate.destinationLibraryIdentifier,
                    config.targetParentIdentifier == candidate.destinationParentIdentifier,
                    try performCandidateObservationIsCurrent(candidate)
                else { throw PhotoResourceTransferFailure.ineligibleObservation }

                if try performRefreshExistingBackupCandidate(candidate) {
                    pending = try performBackupQueuePendingCount(
                        authorization: authorization, deviceId: candidate.deviceId)
                    if pending > maximumPending {
                        throw PhotoResourceTransferFailure.queueCapacityReached
                    }
                    continue
                }
                if try performHasUnresolvedCandidate(for: candidate) { continue }
                sqlite3_reset(statement)
                sqlite3_clear_bindings(statement)
                bindBackupCandidate(candidate, to: statement)
                guard sqlite3_step(statement) == SQLITE_DONE else {
                    throw photoStoreError(152, "Failed to enqueue Photo Backup resource")
                }
                if sqlite3_changes(db) == 1 {
                    inserted += 1
                    pending += 1
                    totalRows += 1
                    if pending > maximumPending || totalRows > 50_000 {
                        throw PhotoResourceTransferFailure.queueCapacityReached
                    }
                }
            }
            try performSQL("COMMIT;")
            return inserted
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performClaimNextBackupCandidate(
        authorization: PhotoAuthorizationSession, deviceId: String, claimId: UUID,
        leaseUntil: Date, policyRevision: Int64
    ) throws -> PhotoBackupCandidate? {
        guard !deviceId.isEmpty, policyRevision > 0, leaseUntil > Date() else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            guard try performAuthorizationIsCurrent(authorization) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }
            let config = try performFetchConfiguration(profileId: authorization.profileId)
            guard config.canAutomaticallyBackUpInForeground,
                config.policyRevision == policyRevision
            else { throw PhotoResourceTransferFailure.configurationDisabled }
            try performRecoverExpiredBackupClaims(
                authorization: authorization, before: Date())
            let selectSQL = """
                SELECT \(backupCandidateColumnList)
                FROM photo_backup_candidates
                WHERE profile_id=? AND owner_id=? AND device_id=?
                    AND authorization_generation=?
                    AND ((policy_revision=? AND state IN ('QUEUED', 'VERIFIED_LOCALLY',
                        'UPLOADING', 'RECONCILIATION_PENDING', 'FAILED_RECOVERABLE',
                        'SOURCE_UNAVAILABLE'))
                        OR (upload_operation_id IS NOT NULL
                            AND state='RECONCILIATION_PENDING'))
                    AND (retry_after IS NULL OR retry_after<=?)
                ORDER BY CASE state
                    WHEN 'RECONCILIATION_PENDING' THEN 0
                    WHEN 'UPLOADING' THEN 0
                    WHEN 'VERIFIED_LOCALLY' THEN 1
                    ELSE 2 END,
                    created_at ASC, candidate_id ASC
                LIMIT 1;
                """
            var query: OpaquePointer?
            guard sqlite3_prepare_v2(db, selectSQL, -1, &query, nil) == SQLITE_OK else {
                throw photoStoreError(153, "Failed to prepare Photo Backup candidate claim")
            }
            defer { sqlite3_finalize(query) }
            bindText(query, index: 1, text: authorization.profileId.uuidString)
            bindText(query, index: 2, text: authorization.ownerId)
            bindText(query, index: 3, text: deviceId)
            bindText(query, index: 4, text: authorization.generation.uuidString)
            sqlite3_bind_int64(query, 5, policyRevision)
            sqlite3_bind_double(query, 6, Date().timeIntervalSince1970)
            guard sqlite3_step(query) == SQLITE_ROW else {
                try performSQL("COMMIT;")
                return nil
            }
            let original = try decodeBackupCandidate(query)
            let updated = original.replacing(
                state: .claimed, claimId: claimId, claimExpiresAt: leaseUntil,
                claimPreviousState: original.state,
                attemptCount: original.attemptCount + 1)
            let updateSQL = """
                UPDATE photo_backup_candidates SET state='CLAIMED', attempt_count=attempt_count+1,
                    claim_id=?,
                    claim_expires_at=?, claim_previous_state=?, updated_at=?
                WHERE candidate_id=? AND state=? AND authorization_generation=?;
                """
            var update: OpaquePointer?
            guard sqlite3_prepare_v2(db, updateSQL, -1, &update, nil) == SQLITE_OK else {
                throw photoStoreError(154, "Failed to prepare Photo Backup candidate lease")
            }
            defer { sqlite3_finalize(update) }
            bindText(update, index: 1, text: claimId.uuidString)
            sqlite3_bind_double(update, 2, leaseUntil.timeIntervalSince1970)
            bindText(update, index: 3, text: original.state.rawValue)
            sqlite3_bind_double(update, 4, Date().timeIntervalSince1970)
            bindText(update, index: 5, text: original.candidateId.uuidString)
            bindText(update, index: 6, text: original.state.rawValue)
            bindText(update, index: 7, text: authorization.generation.uuidString)
            guard sqlite3_step(update) == SQLITE_DONE, sqlite3_changes(db) == 1 else {
                throw photoStoreError(155, "Photo Backup candidate lease was lost")
            }
            try performSQL("COMMIT;")
            return updated
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performTransitionBackupCandidate(
        _ candidate: PhotoBackupCandidate, expectedState: PhotoBackupQueueState,
        authorization: PhotoAuthorizationSession
    ) throws {
        guard candidate.profileId == authorization.profileId,
            candidate.ownerId == authorization.ownerId,
            candidate.authorizationGeneration == authorization.generation,
            allowedBackupCandidateTransition(from: expectedState, to: candidate.state),
            candidate.claimId != nil || candidate.state == .queued
        else { throw PhotoResourceTransferFailure.staleAuthorization }
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            guard try performAuthorizationIsCurrent(authorization) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }
            guard
                let current = try performFetchBackupCandidate(
                    candidateId: candidate.candidateId, authorization: authorization),
                current.state == expectedState,
                sameBackupCandidateIdentity(current, candidate),
                current.claimId == candidate.claimId
            else { throw PhotoResourceTransferFailure.storageFailure }
            if [.extracting, .verifiedLocally, .uploading].contains(candidate.state) {
                let config = try performFetchConfiguration(profileId: authorization.profileId)
                guard config.canAutomaticallyBackUpInForeground,
                    config.policyRevision == candidate.policyRevision
                else { throw PhotoResourceTransferFailure.configurationDisabled }
            }
            if candidate.state == .fileUploadedAwaitingPhotoBinding {
                guard let uploadId = candidate.uploadOperationId,
                    validUploadOperationId(uploadId),
                    let byteCount = candidate.verifiedByteCount, byteCount > 0,
                    isValidSHA256(candidate.verifiedSHA256),
                    candidate.fileVersionIdentifier?.isEmpty == false
                else { throw PhotoResourceTransferFailure.storageFailure }
            }
            if candidate.state == .photoBindingConfirmed {
                guard candidate.photoAssetIdentifier?.isEmpty == false else {
                    throw PhotoResourceTransferFailure.storageFailure
                }
            }
            if candidate.state == .partial {
                guard let uploadId = candidate.uploadOperationId,
                    validUploadOperationId(uploadId),
                    candidate.fileVersionIdentifier?.isEmpty == false,
                    let byteCount = candidate.verifiedByteCount, byteCount > 0,
                    isValidSHA256(candidate.verifiedSHA256)
                else { throw PhotoResourceTransferFailure.storageFailure }
            }
            if candidate.state == .authoritativeBackupConfirmed {
                guard candidate.photoAssetIdentifier?.isEmpty == false,
                    candidate.backupSnapshotIdentifier?.isEmpty == false,
                    candidate.backupRetentionConfirmedAt != nil
                else { throw PhotoResourceTransferFailure.storageFailure }
            }
            let sql = """
                UPDATE photo_backup_candidates SET
                    state=?, claim_id=?, claim_expires_at=?, claim_previous_state=?,
                    extraction_operation_id=?, upload_operation_id=?, file_version_identifier=?,
                    verified_byte_count=?, verified_sha256=?, photo_asset_identifier=?,
                    backup_snapshot_identifier=?, backup_retention_confirmed_at=?,
                    attempt_count=?, retry_after=?, safe_failure=?, updated_at=?
                WHERE candidate_id=? AND profile_id=? AND owner_id=?
                    AND authorization_generation=? AND state=?
                    AND COALESCE(claim_id, '')=COALESCE(?, '');
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(156, "Failed to prepare Photo Backup state update")
            }
            defer { sqlite3_finalize(statement) }
            bindText(statement, index: 1, text: candidate.state.rawValue)
            bindOptionalText(statement, index: 2, text: candidate.claimId?.uuidString)
            if let expiry = candidate.claimExpiresAt {
                sqlite3_bind_double(statement, 3, expiry.timeIntervalSince1970)
            } else {
                sqlite3_bind_null(statement, 3)
            }
            bindOptionalText(statement, index: 4, text: candidate.claimPreviousState?.rawValue)
            bindOptionalText(statement, index: 5, text: candidate.extractionOperationId?.uuidString)
            bindOptionalText(statement, index: 6, text: candidate.uploadOperationId)
            bindOptionalText(statement, index: 7, text: candidate.fileVersionIdentifier)
            if let byteCount = candidate.verifiedByteCount {
                guard byteCount <= UInt64(Int64.max) else {
                    throw PhotoResourceTransferFailure.resourceTooLarge
                }
                sqlite3_bind_int64(statement, 8, Int64(byteCount))
            } else {
                sqlite3_bind_null(statement, 8)
            }
            bindOptionalText(statement, index: 9, text: candidate.verifiedSHA256)
            bindOptionalText(statement, index: 10, text: candidate.photoAssetIdentifier)
            bindOptionalText(statement, index: 11, text: candidate.backupSnapshotIdentifier)
            if let confirmedAt = candidate.backupRetentionConfirmedAt {
                sqlite3_bind_double(statement, 12, confirmedAt.timeIntervalSince1970)
            } else {
                sqlite3_bind_null(statement, 12)
            }
            sqlite3_bind_int(statement, 13, Int32(candidate.attemptCount))
            if let retryAfter = candidate.retryAfter {
                sqlite3_bind_double(statement, 14, retryAfter.timeIntervalSince1970)
            } else {
                sqlite3_bind_null(statement, 14)
            }
            bindOptionalText(statement, index: 15, text: candidate.safeFailure?.rawValue)
            sqlite3_bind_double(statement, 16, candidate.updatedAt.timeIntervalSince1970)
            bindText(statement, index: 17, text: candidate.candidateId.uuidString)
            bindText(statement, index: 18, text: authorization.profileId.uuidString)
            bindText(statement, index: 19, text: authorization.ownerId)
            bindText(statement, index: 20, text: authorization.generation.uuidString)
            bindText(statement, index: 21, text: expectedState.rawValue)
            bindOptionalText(statement, index: 22, text: candidate.claimId?.uuidString)
            guard sqlite3_step(statement) == SQLITE_DONE, sqlite3_changes(db) == 1 else {
                throw photoStoreError(157, "Photo Backup candidate changed before commit")
            }
            try performSQL("COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performFetchBackupCandidates(
        authorization: PhotoAuthorizationSession, limit: Int
    ) throws -> [PhotoBackupCandidate] {
        guard try performAuthorizationIsCurrent(authorization) else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        let sql = """
            SELECT \(backupCandidateColumnList) FROM photo_backup_candidates
            WHERE profile_id=? AND owner_id=? AND authorization_generation=?
            ORDER BY created_at DESC, candidate_id DESC LIMIT ?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(158, "Failed to prepare Photo Backup candidate inventory")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: authorization.generation.uuidString)
        sqlite3_bind_int(statement, 4, Int32(limit))
        var result: [PhotoBackupCandidate] = []
        while sqlite3_step(statement) == SQLITE_ROW {
            result.append(try decodeBackupCandidate(statement))
        }
        return result
    }

    private func performBackupQueueSummary(
        authorization: PhotoAuthorizationSession
    ) throws -> PhotoBackupQueueSummary {
        guard try performAuthorizationIsCurrent(authorization) else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        let sql = """
            SELECT state, COUNT(*) FROM photo_backup_candidates
            WHERE profile_id=? AND owner_id=? AND authorization_generation=?
            GROUP BY state;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(159, "Failed to prepare Photo Backup queue summary")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: authorization.generation.uuidString)
        var counts: [PhotoBackupQueueState: Int] = [:]
        while sqlite3_step(statement) == SQLITE_ROW {
            guard let raw = sqlite3_column_text(statement, 0),
                let state = PhotoBackupQueueState(rawValue: String(cString: raw))
            else { continue }
            counts[state] = Int(sqlite3_column_int64(statement, 1))
        }
        return PhotoBackupQueueSummary(counts: counts)
    }

    private func performAdoptRecoverableBackupCandidates(
        authorization: PhotoAuthorizationSession, deviceId: String
    ) throws {
        guard !deviceId.isEmpty, try performAuthorizationIsCurrent(authorization) else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            guard try performAuthorizationIsCurrent(authorization) else {
                throw PhotoResourceTransferFailure.staleAuthorization
            }
            let config = try performFetchConfiguration(profileId: authorization.profileId)
            let sql = """
                UPDATE photo_backup_candidates SET
                    authorization_generation=?,
                    state=CASE
                        WHEN upload_operation_id IS NOT NULL
                            AND state IN ('CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY', 'UPLOADING')
                            THEN 'RECONCILIATION_PENDING'
                        WHEN upload_operation_id IS NULL
                            AND state IN ('CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY')
                            THEN 'QUEUED'
                        ELSE state END,
                    extraction_operation_id=CASE WHEN upload_operation_id IS NULL
                        THEN NULL ELSE extraction_operation_id END,
                    verified_byte_count=CASE WHEN upload_operation_id IS NULL
                        THEN NULL ELSE verified_byte_count END,
                    verified_sha256=CASE WHEN upload_operation_id IS NULL
                        THEN NULL ELSE verified_sha256 END,
                    claim_id=NULL, claim_expires_at=NULL, claim_previous_state=NULL,
                    retry_after=CASE WHEN upload_operation_id IS NOT NULL
                            AND state IN ('CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY', 'UPLOADING')
                        THEN ? ELSE retry_after END,
                    safe_failure=CASE WHEN upload_operation_id IS NOT NULL
                            AND state IN ('CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY', 'UPLOADING')
                        THEN 'uploadOutcomeUnknown'
                        WHEN upload_operation_id IS NULL
                            AND state IN ('CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY')
                        THEN 'policyChanged' ELSE safe_failure END,
                    updated_at=?
                WHERE profile_id=? AND owner_id=? AND device_id=?
                    AND authorization_generation<>?
                    AND ((upload_operation_id IS NOT NULL AND state IN (
                            'CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY', 'UPLOADING',
                            'RECONCILIATION_PENDING', 'FILE_UPLOADED_AWAITING_PHOTO_BINDING',
                            'PHOTO_BINDING_CONFIRMED', 'AWAITING_BACKUP_RETENTION_PROOF',
                            'AUTHORITATIVE_BACKUP_CONFIRMED', 'PARTIAL', 'FAILED_RECOVERABLE'))
                        OR (upload_operation_id IS NULL AND policy_revision=?
                            AND destination_library_identifier IS ?
                            AND destination_parent_identifier IS ?
                            AND state IN ('QUEUED', 'ELIGIBLE', 'CLAIMED', 'EXTRACTING',
                                'VERIFIED_LOCALLY', 'FAILED_RECOVERABLE', 'SOURCE_UNAVAILABLE')));
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(187, "Failed to prepare Photo Backup queue adoption")
            }
            defer { sqlite3_finalize(statement) }
            bindText(statement, index: 1, text: authorization.generation.uuidString)
            sqlite3_bind_double(statement, 2, Date().addingTimeInterval(30).timeIntervalSince1970)
            sqlite3_bind_double(statement, 3, Date().timeIntervalSince1970)
            bindText(statement, index: 4, text: authorization.profileId.uuidString)
            bindText(statement, index: 5, text: authorization.ownerId)
            bindText(statement, index: 6, text: deviceId)
            bindText(statement, index: 7, text: authorization.generation.uuidString)
            sqlite3_bind_int64(statement, 8, config.policyRevision)
            bindOptionalText(statement, index: 9, text: config.targetLibraryIdentifier)
            bindOptionalText(statement, index: 10, text: config.targetParentIdentifier)
            guard sqlite3_step(statement) == SQLITE_DONE else {
                throw photoStoreError(188, "Failed to adopt recoverable Photo Backup candidates")
            }
            try performSQL("COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performRecoverExpiredBackupClaims(
        authorization: PhotoAuthorizationSession, before: Date
    ) throws {
        guard try performAuthorizationIsCurrent(authorization) else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        let sql = """
            UPDATE photo_backup_candidates SET
                state=CASE
                    WHEN state='CLAIMED' AND claim_previous_state='EXTRACTING' THEN 'QUEUED'
                    WHEN state='CLAIMED' THEN COALESCE(claim_previous_state, 'QUEUED')
                    WHEN state='EXTRACTING' THEN 'QUEUED'
                    WHEN state='UPLOADING' THEN 'RECONCILIATION_PENDING'
                    ELSE state END,
                claim_id=NULL, claim_expires_at=NULL, claim_previous_state=NULL,
                retry_after=CASE WHEN state IN ('UPLOADING', 'RECONCILIATION_PENDING')
                    THEN ? ELSE retry_after END,
                safe_failure=CASE WHEN state='UPLOADING' THEN 'uploadOutcomeUnknown'
                    WHEN state='EXTRACTING' THEN 'stagingCorrupt' ELSE safe_failure END,
                updated_at=?
            WHERE profile_id=? AND owner_id=? AND authorization_generation=?
                AND state IN ('CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY', 'UPLOADING',
                    'RECONCILIATION_PENDING') AND claim_expires_at<=?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(160, "Failed to prepare expired Photo Backup claim recovery")
        }
        defer { sqlite3_finalize(statement) }
        sqlite3_bind_double(statement, 1, Date().timeIntervalSince1970)
        sqlite3_bind_double(statement, 2, Date().addingTimeInterval(30).timeIntervalSince1970)
        bindText(statement, index: 3, text: authorization.profileId.uuidString)
        bindText(statement, index: 4, text: authorization.ownerId)
        bindText(statement, index: 5, text: authorization.generation.uuidString)
        sqlite3_bind_double(statement, 6, before.timeIntervalSince1970)
        guard sqlite3_step(statement) == SQLITE_DONE else {
            throw photoStoreError(161, "Failed to recover expired Photo Backup claims")
        }
    }

    private func performBackupDiscoveryCursor(
        authorization: PhotoAuthorizationSession, deviceId: String, policyRevision: Int64,
        scanGeneration: Int64
    ) throws -> String? {
        guard try performAuthorizationIsCurrent(authorization) else {
            throw PhotoResourceTransferFailure.staleAuthorization
        }
        let sql = """
            SELECT last_local_identifier FROM photo_backup_scan_cursors
            WHERE profile_id=? AND owner_id=? AND device_id=?
                AND authorization_generation=? AND policy_revision=? AND scan_generation=?
            LIMIT 1;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(178, "Failed to prepare Photo Backup scan cursor read")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: deviceId)
        bindText(statement, index: 4, text: authorization.generation.uuidString)
        sqlite3_bind_int64(statement, 5, policyRevision)
        sqlite3_bind_int64(statement, 6, scanGeneration)
        guard sqlite3_step(statement) == SQLITE_ROW else { return nil }
        return nullableString(statement, 0)
    }

    private func performSaveBackupDiscoveryCursor(
        _ localIdentifier: String?, authorization: PhotoAuthorizationSession,
        deviceId: String, policyRevision: Int64, scanGeneration: Int64
    ) throws {
        guard try performAuthorizationIsCurrent(authorization), !deviceId.isEmpty,
            policyRevision > 0, scanGeneration > 0
        else { throw PhotoResourceTransferFailure.staleAuthorization }
        if let localIdentifier {
            let sql = """
                INSERT INTO photo_backup_scan_cursors (
                    profile_id, owner_id, device_id, authorization_generation,
                    policy_revision, scan_generation, last_local_identifier, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(profile_id, owner_id, device_id, authorization_generation,
                    policy_revision) DO UPDATE SET
                    scan_generation=excluded.scan_generation,
                    last_local_identifier=excluded.last_local_identifier,
                    updated_at=excluded.updated_at;
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(179, "Failed to prepare Photo Backup scan cursor write")
            }
            defer { sqlite3_finalize(statement) }
            bindText(statement, index: 1, text: authorization.profileId.uuidString)
            bindText(statement, index: 2, text: authorization.ownerId)
            bindText(statement, index: 3, text: deviceId)
            bindText(statement, index: 4, text: authorization.generation.uuidString)
            sqlite3_bind_int64(statement, 5, policyRevision)
            sqlite3_bind_int64(statement, 6, scanGeneration)
            bindText(statement, index: 7, text: localIdentifier)
            sqlite3_bind_double(statement, 8, Date().timeIntervalSince1970)
            guard sqlite3_step(statement) == SQLITE_DONE else {
                throw photoStoreError(180, "Failed to persist Photo Backup scan cursor")
            }
        } else {
            let sql = """
                DELETE FROM photo_backup_scan_cursors
                WHERE profile_id=? AND owner_id=? AND device_id=?
                    AND authorization_generation=? AND policy_revision=?;
                """
            var statement: OpaquePointer?
            guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
                throw photoStoreError(181, "Failed to prepare Photo Backup cursor reset")
            }
            defer { sqlite3_finalize(statement) }
            bindText(statement, index: 1, text: authorization.profileId.uuidString)
            bindText(statement, index: 2, text: authorization.ownerId)
            bindText(statement, index: 3, text: deviceId)
            bindText(statement, index: 4, text: authorization.generation.uuidString)
            sqlite3_bind_int64(statement, 5, policyRevision)
            guard sqlite3_step(statement) == SQLITE_DONE else {
                throw photoStoreError(182, "Failed to reset Photo Backup scan cursor")
            }
        }
    }

    private func performBackupQueuePendingCount(
        authorization: PhotoAuthorizationSession, deviceId: String
    ) throws -> Int {
        let sql = """
            SELECT COUNT(*) FROM photo_backup_candidates
            WHERE profile_id=? AND owner_id=? AND device_id=?
                AND authorization_generation=?
                AND state IN ('QUEUED', 'CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY',
                    'UPLOADING', 'RECONCILIATION_PENDING', 'FAILED_RECOVERABLE',
                    'SOURCE_UNAVAILABLE');
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(162, "Failed to count Photo Backup queue candidates")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: deviceId)
        bindText(statement, index: 4, text: authorization.generation.uuidString)
        guard sqlite3_step(statement) == SQLITE_ROW else { return 0 }
        return Int(sqlite3_column_int64(statement, 0))
    }

    private func performBackupQueueTotalCount(
        authorization: PhotoAuthorizationSession, deviceId: String
    ) throws -> Int {
        let sql = """
            SELECT COUNT(*) FROM photo_backup_candidates
            WHERE profile_id=? AND owner_id=? AND device_id=?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(185, "Failed to count retained Photo Backup queue rows")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: deviceId)
        guard sqlite3_step(statement) == SQLITE_ROW else { return 0 }
        return Int(sqlite3_column_int64(statement, 0))
    }

    private func performPruneTerminalBackupCandidates(
        authorization: PhotoAuthorizationSession, deviceId: String, olderThan cutoff: Date
    ) throws {
        let sql = """
            DELETE FROM photo_backup_candidates
            WHERE profile_id=? AND owner_id=? AND device_id=?
                AND upload_operation_id IS NULL AND file_version_identifier IS NULL
                AND state IN ('FAILED_TERMINAL', 'CANCELLED') AND updated_at<?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(186, "Failed to prepare bounded Photo Backup queue cleanup")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: authorization.profileId.uuidString)
        bindText(statement, index: 2, text: authorization.ownerId)
        bindText(statement, index: 3, text: deviceId)
        sqlite3_bind_double(statement, 4, cutoff.timeIntervalSince1970)
        guard sqlite3_step(statement) == SQLITE_DONE else {
            throw photoStoreError(187, "Failed to prune terminal Photo Backup queue rows")
        }
    }

    private func performCandidateObservationIsCurrent(_ candidate: PhotoBackupCandidate) throws
        -> Bool
    {
        let sql = """
            SELECT local_identifier, asset_kind, creation_date, modification_date,
                pixel_width, pixel_height, is_favorite, is_hidden, discovery_generation,
                observed_at, permission_scope, source_availability, is_excluded
            FROM photo_asset_observations
            WHERE local_identifier=? AND profile_id=? AND owner_id=?
                AND discovery_generation=? AND source_availability='available'
                AND is_excluded=0 LIMIT 1;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(163, "Failed to prepare Photo Backup source eligibility check")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: candidate.localIdentifier)
        bindText(statement, index: 2, text: candidate.profileId.uuidString)
        bindText(statement, index: 3, text: candidate.ownerId)
        sqlite3_bind_int64(statement, 4, candidate.observationGeneration)
        guard sqlite3_step(statement) == SQLITE_ROW else { return false }
        let current = try decodePhotoObservation(statement)
        return current.permissionScope.rawValue == candidate.clientLibraryScope
            && PhotoBackupCandidate.sourceRevision(for: current) == candidate.sourceRevision
    }

    private func performRefreshExistingBackupCandidate(
        _ candidate: PhotoBackupCandidate
    ) throws -> Bool {
        let sql = """
            SELECT candidate_id, state, upload_operation_id, retry_after
            FROM photo_backup_candidates
            WHERE profile_id=? AND owner_id=? AND device_id=? AND client_library_scope=?
                AND local_identifier=? AND source_revision=? AND resource_index=?
                AND resource_type=? AND resource_role=? AND rendition=?
                AND authorization_generation=? AND policy_revision=? LIMIT 1;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(170, "Failed to prepare Photo Backup duplicate lookup")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: candidate.profileId.uuidString)
        bindText(statement, index: 2, text: candidate.ownerId)
        bindText(statement, index: 3, text: candidate.deviceId)
        bindText(statement, index: 4, text: candidate.clientLibraryScope)
        bindText(statement, index: 5, text: candidate.localIdentifier)
        bindText(statement, index: 6, text: candidate.sourceRevision)
        sqlite3_bind_int64(statement, 7, Int64(candidate.selection.resourceIndex))
        bindText(statement, index: 8, text: candidate.selection.resourceType)
        bindText(statement, index: 9, text: candidate.selection.role.rawValue)
        bindText(statement, index: 10, text: candidate.selection.rendition.rawValue)
        bindText(statement, index: 11, text: candidate.authorizationGeneration.uuidString)
        sqlite3_bind_int64(statement, 12, candidate.policyRevision)
        guard sqlite3_step(statement) == SQLITE_ROW else { return false }
        guard let stateRaw = sqlite3_column_text(statement, 1),
            let state = PhotoBackupQueueState(rawValue: String(cString: stateRaw))
        else { throw photoStoreError(171, "Invalid duplicate Photo Backup candidate") }
        let uploadOperationId = nullableString(statement, 2)
        let retryAfter =
            sqlite3_column_type(statement, 3) == SQLITE_NULL
            ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 3))
        guard [.queued, .failedRecoverable, .sourceUnavailable].contains(state),
            uploadOperationId == nil,
            let idRaw = sqlite3_column_text(statement, 0),
            let id = UUID(uuidString: String(cString: idRaw))
        else { return true }
        let sourceRetryIsPending =
            state == .sourceUnavailable
            && retryAfter.map { $0 > Date() } == true

        let updateSQL = """
            UPDATE photo_backup_candidates SET
                state=CASE WHEN state='SOURCE_UNAVAILABLE' AND ?=0
                    THEN 'QUEUED' ELSE state END,
                observation_generation=?,
                retry_after=CASE WHEN state='SOURCE_UNAVAILABLE' AND ?=0
                    THEN NULL ELSE retry_after END,
                safe_failure=CASE WHEN state='SOURCE_UNAVAILABLE' AND ?=0
                    THEN NULL ELSE safe_failure END,
                updated_at=?
            WHERE candidate_id=? AND state=? AND upload_operation_id IS NULL;
            """
        var update: OpaquePointer?
        guard sqlite3_prepare_v2(db, updateSQL, -1, &update, nil) == SQLITE_OK else {
            throw photoStoreError(172, "Failed to prepare Photo Backup duplicate refresh")
        }
        defer { sqlite3_finalize(update) }
        sqlite3_bind_int(update, 1, sourceRetryIsPending ? 1 : 0)
        sqlite3_bind_int64(update, 2, candidate.observationGeneration)
        sqlite3_bind_int(update, 3, sourceRetryIsPending ? 1 : 0)
        sqlite3_bind_int(update, 4, sourceRetryIsPending ? 1 : 0)
        sqlite3_bind_double(update, 5, Date().timeIntervalSince1970)
        bindText(update, index: 6, text: id.uuidString)
        bindText(update, index: 7, text: state.rawValue)
        guard sqlite3_step(update) == SQLITE_DONE else {
            throw photoStoreError(173, "Failed to refresh Photo Backup observation generation")
        }
        return true
    }

    private func performHasUnresolvedCandidate(for candidate: PhotoBackupCandidate) throws -> Bool {
        let sql = """
            SELECT 1 FROM photo_backup_candidates
            WHERE profile_id=? AND owner_id=? AND device_id=?
                AND client_library_scope=? AND local_identifier=? AND source_revision=?
                AND resource_index=? AND resource_type=? AND resource_role=? AND rendition=?
                AND authorization_generation=?
                AND state IN ('CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY', 'UPLOADING',
                    'RECONCILIATION_PENDING', 'FILE_UPLOADED_AWAITING_PHOTO_BINDING',
                    'PHOTO_BINDING_CONFIRMED', 'AWAITING_BACKUP_RETENTION_PROOF',
                    'AUTHORITATIVE_BACKUP_CONFIRMED');
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(164, "Failed to prepare unresolved Photo Backup identity check")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: candidate.profileId.uuidString)
        bindText(statement, index: 2, text: candidate.ownerId)
        bindText(statement, index: 3, text: candidate.deviceId)
        bindText(statement, index: 4, text: candidate.clientLibraryScope)
        bindText(statement, index: 5, text: candidate.localIdentifier)
        bindText(statement, index: 6, text: candidate.sourceRevision)
        sqlite3_bind_int64(statement, 7, Int64(candidate.selection.resourceIndex))
        bindText(statement, index: 8, text: candidate.selection.resourceType)
        bindText(statement, index: 9, text: candidate.selection.role.rawValue)
        bindText(statement, index: 10, text: candidate.selection.rendition.rawValue)
        bindText(statement, index: 11, text: candidate.authorizationGeneration.uuidString)
        return sqlite3_step(statement) == SQLITE_ROW
    }

    private func performFetchBackupCandidate(
        candidateId: UUID, authorization: PhotoAuthorizationSession
    ) throws -> PhotoBackupCandidate? {
        let sql = """
            SELECT \(backupCandidateColumnList) FROM photo_backup_candidates
            WHERE candidate_id=? AND profile_id=? AND owner_id=? AND authorization_generation=?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(165, "Failed to prepare Photo Backup candidate read")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: candidateId.uuidString)
        bindText(statement, index: 2, text: authorization.profileId.uuidString)
        bindText(statement, index: 3, text: authorization.ownerId)
        bindText(statement, index: 4, text: authorization.generation.uuidString)
        guard sqlite3_step(statement) == SQLITE_ROW else { return nil }
        return try decodeBackupCandidate(statement)
    }

    private func bindBackupCandidate(
        _ candidate: PhotoBackupCandidate, to statement: OpaquePointer?
    ) {
        bindText(statement, index: 1, text: candidate.candidateId.uuidString)
        bindText(statement, index: 2, text: candidate.profileId.uuidString)
        bindText(statement, index: 3, text: candidate.ownerId)
        bindText(statement, index: 4, text: candidate.deviceId)
        bindText(statement, index: 5, text: candidate.clientLibraryScope)
        bindText(statement, index: 6, text: candidate.localIdentifier)
        bindText(statement, index: 7, text: candidate.sourceRevision)
        sqlite3_bind_int64(statement, 8, Int64(candidate.selection.resourceIndex))
        bindText(statement, index: 9, text: candidate.selection.resourceType)
        bindText(statement, index: 10, text: candidate.selection.role.rawValue)
        bindText(statement, index: 11, text: candidate.selection.rendition.rawValue)
        bindOptionalText(statement, index: 12, text: candidate.selection.uniformTypeIdentifier)
        if let date = candidate.selection.sourceModificationDate {
            sqlite3_bind_double(statement, 13, date.timeIntervalSince1970)
        } else {
            sqlite3_bind_null(statement, 13)
        }
        sqlite3_bind_int64(statement, 14, candidate.observationGeneration)
        sqlite3_bind_int64(statement, 15, candidate.policyRevision)
        bindText(statement, index: 16, text: candidate.authorizationGeneration.uuidString)
        bindText(statement, index: 17, text: candidate.destinationLibraryIdentifier)
        bindText(statement, index: 18, text: candidate.destinationParentIdentifier)
        bindOptionalText(statement, index: 19, text: candidate.groupIdentity)
        bindText(statement, index: 20, text: candidate.state.rawValue)
        sqlite3_bind_int(statement, 21, Int32(candidate.attemptCount))
        if let retryAfter = candidate.retryAfter {
            sqlite3_bind_double(statement, 22, retryAfter.timeIntervalSince1970)
        } else {
            sqlite3_bind_null(statement, 22)
        }
        bindOptionalText(statement, index: 23, text: candidate.claimId?.uuidString)
        if let date = candidate.claimExpiresAt {
            sqlite3_bind_double(statement, 24, date.timeIntervalSince1970)
        } else {
            sqlite3_bind_null(statement, 24)
        }
        bindOptionalText(statement, index: 25, text: candidate.claimPreviousState?.rawValue)
        bindOptionalText(statement, index: 26, text: candidate.extractionOperationId?.uuidString)
        bindOptionalText(statement, index: 27, text: candidate.uploadOperationId)
        bindOptionalText(statement, index: 28, text: candidate.fileVersionIdentifier)
        if let bytes = candidate.verifiedByteCount {
            sqlite3_bind_int64(statement, 29, Int64(bytes))
        } else {
            sqlite3_bind_null(statement, 29)
        }
        bindOptionalText(statement, index: 30, text: candidate.verifiedSHA256)
        bindOptionalText(statement, index: 31, text: candidate.photoAssetIdentifier)
        bindOptionalText(statement, index: 32, text: candidate.backupSnapshotIdentifier)
        if let confirmedAt = candidate.backupRetentionConfirmedAt {
            sqlite3_bind_double(statement, 33, confirmedAt.timeIntervalSince1970)
        } else {
            sqlite3_bind_null(statement, 33)
        }
        bindOptionalText(statement, index: 34, text: candidate.safeFailure?.rawValue)
        sqlite3_bind_double(statement, 35, candidate.createdAt.timeIntervalSince1970)
        sqlite3_bind_double(statement, 36, candidate.updatedAt.timeIntervalSince1970)
    }

    private func decodeBackupCandidate(_ statement: OpaquePointer?) throws
        -> PhotoBackupCandidate
    {
        func string(_ index: Int32) throws -> String {
            guard let value = sqlite3_column_text(statement, index) else {
                throw photoStoreError(166, "Corrupt Photo Backup candidate")
            }
            return String(cString: value)
        }
        func uuid(_ index: Int32) throws -> UUID {
            guard let value = UUID(uuidString: try string(index)) else {
                throw photoStoreError(167, "Invalid Photo Backup candidate identity")
            }
            return value
        }
        guard let role = PhotoResourceRole(rawValue: try string(9)),
            let rendition = PhotoResourceRendition(rawValue: try string(10)),
            let state = PhotoBackupQueueState(rawValue: try string(19))
        else { throw photoStoreError(168, "Invalid Photo Backup candidate state") }
        let resourceIndex = sqlite3_column_int64(statement, 7)
        let byteCountValue = sqlite3_column_int64(statement, 28)
        guard resourceIndex >= 0, byteCountValue >= 0 else {
            throw photoStoreError(169, "Invalid Photo Backup candidate size")
        }
        let selection = PhotoResourceSelection(
            localIdentifier: try string(5), resourceIndex: Int(resourceIndex),
            resourceType: try string(8), role: role, rendition: rendition,
            uniformTypeIdentifier: nullableString(statement, 11),
            sourceModificationDate: sqlite3_column_type(statement, 12) == SQLITE_NULL
                ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 12)))
        return PhotoBackupCandidate(
            candidateId: try uuid(0), profileId: try uuid(1), ownerId: try string(2),
            deviceId: try string(3), clientLibraryScope: try string(4),
            localIdentifier: try string(5), sourceRevision: try string(6),
            selection: selection, observationGeneration: sqlite3_column_int64(statement, 13),
            policyRevision: sqlite3_column_int64(statement, 14),
            authorizationGeneration: try uuid(15),
            destinationLibraryIdentifier: try string(16),
            destinationParentIdentifier: try string(17),
            groupIdentity: nullableString(statement, 18),
            state: state, attemptCount: Int(sqlite3_column_int(statement, 20)),
            retryAfter: sqlite3_column_type(statement, 21) == SQLITE_NULL
                ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 21)),
            claimId: nullableString(statement, 22).flatMap(UUID.init(uuidString:)),
            claimExpiresAt: sqlite3_column_type(statement, 23) == SQLITE_NULL
                ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 23)),
            claimPreviousState: nullableString(statement, 24).flatMap(
                PhotoBackupQueueState.init(rawValue:)),
            extractionOperationId: nullableString(statement, 25).flatMap(UUID.init(uuidString:)),
            uploadOperationId: nullableString(statement, 26),
            fileVersionIdentifier: nullableString(statement, 27),
            verifiedByteCount: sqlite3_column_type(statement, 28) == SQLITE_NULL
                ? nil : UInt64(byteCountValue),
            verifiedSHA256: nullableString(statement, 29),
            photoAssetIdentifier: nullableString(statement, 30),
            backupSnapshotIdentifier: nullableString(statement, 31),
            backupRetentionConfirmedAt: sqlite3_column_type(statement, 32) == SQLITE_NULL
                ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 32)),
            safeFailure: nullableString(statement, 33).flatMap(
                PhotoBackupSafeFailure.init(rawValue:)),
            createdAt: Date(timeIntervalSince1970: sqlite3_column_double(statement, 34)),
            updatedAt: Date(timeIntervalSince1970: sqlite3_column_double(statement, 35)))
    }

    private func sameBackupCandidateIdentity(
        _ lhs: PhotoBackupCandidate, _ rhs: PhotoBackupCandidate
    ) -> Bool {
        lhs.candidateId == rhs.candidateId && lhs.profileId == rhs.profileId
            && lhs.ownerId == rhs.ownerId && lhs.deviceId == rhs.deviceId
            && lhs.clientLibraryScope == rhs.clientLibraryScope
            && lhs.localIdentifier == rhs.localIdentifier
            && lhs.sourceRevision == rhs.sourceRevision && lhs.selection == rhs.selection
            && lhs.policyRevision == rhs.policyRevision
            && lhs.authorizationGeneration == rhs.authorizationGeneration
            && lhs.destinationLibraryIdentifier == rhs.destinationLibraryIdentifier
            && lhs.destinationParentIdentifier == rhs.destinationParentIdentifier
            && lhs.groupIdentity == rhs.groupIdentity
    }

    private func allowedBackupCandidateTransition(
        from previous: PhotoBackupQueueState, to next: PhotoBackupQueueState
    ) -> Bool {
        let allowed: [PhotoBackupQueueState: Set<PhotoBackupQueueState>] = [
            .queued: [
                .claimed, .cancelled, .sourceUnavailable, .failedRecoverable, .failedTerminal,
            ],
            .eligible: [.claimed, .cancelled, .failedRecoverable, .failedTerminal],
            .claimed: [
                .queued, .extracting, .verifiedLocally, .uploading, .reconciliationPending,
                .sourceUnavailable, .failedRecoverable, .failedTerminal, .cancelled,
            ],
            .extracting: [
                .verifiedLocally, .queued, .sourceUnavailable, .failedRecoverable, .failedTerminal,
            ],
            .verifiedLocally: [.claimed, .uploading, .failedRecoverable, .failedTerminal],
            .uploading: [
                .reconciliationPending, .fileUploadedAwaitingPhotoBinding,
                .failedRecoverable, .failedTerminal,
            ],
            .reconciliationPending: [
                .claimed, .uploading, .fileUploadedAwaitingPhotoBinding,
                .failedRecoverable, .failedTerminal, .reconciliationPending,
            ],
            .fileUploadedAwaitingPhotoBinding: [
                .photoBindingConfirmed, .awaitingBackupRetentionProof,
                .failedRecoverable, .partial,
            ],
            .photoBindingConfirmed: [.awaitingBackupRetentionProof, .authoritativeBackupConfirmed],
            .awaitingBackupRetentionProof: [.authoritativeBackupConfirmed, .failedRecoverable],
            .failedRecoverable: [.claimed, .queued, .failedTerminal, .sourceUnavailable],
        ]
        return allowed[previous]?.contains(next) == true
    }

    private var backupCandidateColumnList: String {
        "candidate_id, profile_id, owner_id, device_id, client_library_scope, local_identifier, "
            + "source_revision, resource_index, resource_type, resource_role, rendition, "
            + "uniform_type_identifier, source_modification_date, observation_generation, "
            + "policy_revision, authorization_generation, destination_library_identifier, "
            + "destination_parent_identifier, group_identity, state, attempt_count, retry_after, "
            + "claim_id, claim_expires_at, claim_previous_state, extraction_operation_id, "
            + "upload_operation_id, "
            + "file_version_identifier, verified_byte_count, verified_sha256, "
            + "photo_asset_identifier, backup_snapshot_identifier, "
            + "backup_retention_confirmed_at, safe_failure, "
            + "created_at, updated_at"
    }

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
            if !intent.userInitiated {
                let policy = try performFetchConfiguration(profileId: intent.profileId)
                guard policy.canAutomaticallyBackUpInForeground,
                    policy.policyRevision == Int64(intent.policyVersion)
                else { throw PhotoResourceTransferFailure.configurationDisabled }
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
            record.policyVersion > 0,
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
        guard maximumResourceBytes > 0, policyVersion > 0, resourceIndex >= 0
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
            && photoTimestampValuesMatch(lhs.createdAt, rhs.createdAt)
    }

    private func performExtractionConsentIsCurrent(
        _ record: PhotoResourceExtractionRecord
    ) throws -> Bool {
        let sql = """
            SELECT is_opted_in, include_videos, pause_discovery,
                automatic_foreground_backup_enabled, automatic_backup_consent_confirmed,
                pause_automatic_backup, policy_revision, target_library_identifier,
                target_parent_identifier
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
        guard includesVideos || ![.primaryVideo, .motionVideo].contains(record.selection.role)
        else { return false }
        guard !record.userInitiated else { return true }
        return sqlite3_column_int(statement, 3) == 1
            && sqlite3_column_int(statement, 4) == 1
            && sqlite3_column_int(statement, 5) == 0
            && sqlite3_column_int64(statement, 6) == Int64(record.policyVersion)
            && nullableString(statement, 7)?.isEmpty == false
            && nullableString(statement, 8)?.isEmpty == false
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
            FROM photo_asset_observations WHERE profile_id=? AND owner_id=?
            ORDER BY local_identifier ASC;
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
            observations.append(try decodePhotoObservation(statement))
        }

        return observations
    }

    private func performFetchObservationPage(
        profileId: UUID, ownerId: String, scanGeneration: Int64,
        after localIdentifier: String?, limit: Int
    ) throws -> [PhotoAssetObservation] {
        let hasCursor = localIdentifier != nil
        let predicate = hasCursor ? "AND local_identifier > ?" : ""
        let sql = """
            SELECT local_identifier, asset_kind, creation_date, modification_date,
                pixel_width, pixel_height, is_favorite, is_hidden, discovery_generation,
                observed_at, permission_scope, source_availability, is_excluded
            FROM photo_asset_observations
            WHERE profile_id=? AND owner_id=? AND discovery_generation=? \(predicate)
            ORDER BY local_identifier ASC LIMIT ?;
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(183, "Failed to prepare paged PhotoKit observation read")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: profileId.uuidString)
        bindText(statement, index: 2, text: ownerId)
        sqlite3_bind_int64(statement, 3, scanGeneration)
        if let localIdentifier {
            bindText(statement, index: 4, text: localIdentifier)
            sqlite3_bind_int(statement, 5, Int32(limit))
        } else {
            sqlite3_bind_int(statement, 4, Int32(limit))
        }
        var observations: [PhotoAssetObservation] = []
        while sqlite3_step(statement) == SQLITE_ROW {
            observations.append(try decodePhotoObservation(statement))
        }
        return observations
    }

    private func decodePhotoObservation(_ statement: OpaquePointer?) throws
        -> PhotoAssetObservation
    {
        guard let localIdentifier = nullableString(statement, 0),
            let kindRaw = nullableString(statement, 1),
            let scopeRaw = nullableString(statement, 10),
            let sourceRaw = nullableString(statement, 11)
        else { throw photoStoreError(184, "Corrupt PhotoKit observation row") }
        let kind = PhotoAssetKind(rawValue: kindRaw) ?? .unknown
        let scope = PhotoPermissionState(rawValue: scopeRaw) ?? .unknownFutureStatus
        let source = PhotoSourceAvailability(rawValue: sourceRaw) ?? .sourceUnavailable
        return PhotoAssetObservation(
            localIdentifier: localIdentifier, assetKind: kind,
            creationDate: sqlite3_column_type(statement, 2) == SQLITE_NULL
                ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 2)),
            modificationDate: sqlite3_column_type(statement, 3) == SQLITE_NULL
                ? nil : Date(timeIntervalSince1970: sqlite3_column_double(statement, 3)),
            pixelWidth: Int(sqlite3_column_int(statement, 4)),
            pixelHeight: Int(sqlite3_column_int(statement, 5)),
            isFavorite: sqlite3_column_int(statement, 6) != 0,
            isHidden: sqlite3_column_int(statement, 7) != 0,
            discoveryGeneration: sqlite3_column_int64(statement, 8),
            observedAt: Date(timeIntervalSince1970: sqlite3_column_double(statement, 9)),
            permissionScope: scope, sourceAvailability: source,
            isExcluded: sqlite3_column_int(statement, 12) != 0)
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
        guard config.maximumResourceSizeBytes > 0,
            config.maximumResourceSizeBytes <= UInt64(Int64.max),
            config.maximumConcurrentOperations == 1,
            !config.automaticForegroundBackupEnabled
                || config.automaticBackupConsentConfirmed
                    && config.discoveryEnabled
                    && config.targetLibraryIdentifier?.isEmpty == false
                    && config.targetParentIdentifier?.isEmpty == false
        else {
            throw photoStoreError(
                174, "Photo Backup policy is incomplete or not explicitly confirmed")
        }
        try performSQL("BEGIN IMMEDIATE TRANSACTION;")
        do {
            let old = try performFetchConfiguration(profileId: profileId)
            let hasExistingRow = try performConfigurationExists(profileId: profileId)
            let routingPolicyChanged =
                hasExistingRow
                && (old.discoveryEnabled != config.discoveryEnabled
                    || old.scope != config.scope
                    || old.includeStillImages != config.includeStillImages
                    || old.includeVideos != config.includeVideos
                    || old.includeLivePhotos != config.includeLivePhotos
                    || old.renditionPolicy != config.renditionPolicy
                    || old.targetLibraryIdentifier != config.targetLibraryIdentifier
                    || old.targetParentIdentifier != config.targetParentIdentifier
                    || old.networkRestriction != config.networkRestriction
                    || old.allowCellularTransfer != config.allowCellularTransfer
                    || old.respectLowDataMode != config.respectLowDataMode
                    || old.maximumResourceSizeBytes != config.maximumResourceSizeBytes)
            let nextRevision =
                hasExistingRow
                ? (routingPolicyChanged ? old.policyRevision &+ 1 : old.policyRevision)
                : max(config.policyRevision, 1)
            let persisted = config.replacingPolicyRevision(max(nextRevision, 1))
            if routingPolicyChanged {
                let fenceSQL = """
                    UPDATE photo_backup_candidates SET state='FAILED_TERMINAL', claim_id=NULL,
                        claim_expires_at=NULL, claim_previous_state=NULL,
                        safe_failure='policyChanged', updated_at=?
                    WHERE profile_id=? AND policy_revision<? AND upload_operation_id IS NULL
                        AND state IN ('QUEUED', 'CLAIMED', 'EXTRACTING', 'VERIFIED_LOCALLY',
                            'FAILED_RECOVERABLE');
                    """
                var fence: OpaquePointer?
                guard sqlite3_prepare_v2(db, fenceSQL, -1, &fence, nil) == SQLITE_OK else {
                    throw photoStoreError(175, "Failed to fence stale Photo Backup candidates")
                }
                defer { sqlite3_finalize(fence) }
                sqlite3_bind_double(fence, 1, Date().timeIntervalSince1970)
                bindText(fence, index: 2, text: profileId.uuidString)
                sqlite3_bind_int64(fence, 3, persisted.policyRevision)
                guard sqlite3_step(fence) == SQLITE_DONE else {
                    throw photoStoreError(176, "Failed to fence stale Photo Backup work")
                }
            }
            let sql = """
                INSERT INTO photo_backup_configurations (
                    profile_id, is_opted_in, scope, include_videos, pause_discovery,
                    include_still_images, include_live_photos, rendition_policy,
                    target_library_identifier, target_parent_identifier, network_restriction,
                    allow_cellular_transfer, respect_low_data_mode, maximum_resource_size_bytes,
                    maximum_concurrent_operations, automatic_foreground_backup_enabled,
                    automatic_backup_consent_confirmed, pause_automatic_backup, policy_revision,
                    updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(profile_id) DO UPDATE SET
                    is_opted_in=excluded.is_opted_in,
                    scope=excluded.scope,
                    include_videos=excluded.include_videos,
                    pause_discovery=excluded.pause_discovery,
                    include_still_images=excluded.include_still_images,
                    include_live_photos=excluded.include_live_photos,
                    rendition_policy=excluded.rendition_policy,
                    target_library_identifier=excluded.target_library_identifier,
                    target_parent_identifier=excluded.target_parent_identifier,
                    network_restriction=excluded.network_restriction,
                    allow_cellular_transfer=excluded.allow_cellular_transfer,
                    respect_low_data_mode=excluded.respect_low_data_mode,
                    maximum_resource_size_bytes=excluded.maximum_resource_size_bytes,
                    maximum_concurrent_operations=excluded.maximum_concurrent_operations,
                    automatic_foreground_backup_enabled=excluded.automatic_foreground_backup_enabled,
                    automatic_backup_consent_confirmed=excluded.automatic_backup_consent_confirmed,
                    pause_automatic_backup=excluded.pause_automatic_backup,
                    policy_revision=excluded.policy_revision,
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
            sqlite3_bind_int(statement, 2, persisted.discoveryEnabled ? 1 : 0)
            bindText(statement, index: 3, text: persisted.scope.rawValue)
            sqlite3_bind_int(statement, 4, persisted.includeVideos ? 1 : 0)
            sqlite3_bind_int(statement, 5, persisted.pauseDiscovery ? 1 : 0)
            sqlite3_bind_int(statement, 6, persisted.includeStillImages ? 1 : 0)
            sqlite3_bind_int(statement, 7, persisted.includeLivePhotos ? 1 : 0)
            bindText(statement, index: 8, text: persisted.renditionPolicy.rawValue)
            bindOptionalText(statement, index: 9, text: persisted.targetLibraryIdentifier)
            bindOptionalText(statement, index: 10, text: persisted.targetParentIdentifier)
            bindText(statement, index: 11, text: persisted.networkRestriction.rawValue)
            sqlite3_bind_int(statement, 12, persisted.allowCellularTransfer ? 1 : 0)
            sqlite3_bind_int(statement, 13, persisted.respectLowDataMode ? 1 : 0)
            sqlite3_bind_int64(statement, 14, Int64(persisted.maximumResourceSizeBytes))
            sqlite3_bind_int(statement, 15, Int32(persisted.maximumConcurrentOperations))
            sqlite3_bind_int(statement, 16, persisted.automaticForegroundBackupEnabled ? 1 : 0)
            sqlite3_bind_int(statement, 17, persisted.automaticBackupConsentConfirmed ? 1 : 0)
            sqlite3_bind_int(statement, 18, persisted.pauseAutomaticBackup ? 1 : 0)
            sqlite3_bind_int64(statement, 19, persisted.policyRevision)
            sqlite3_bind_double(statement, 20, Date().timeIntervalSince1970)

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
            try performSQL("COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private func performFetchConfiguration(profileId: UUID) throws -> PhotoBackupConfiguration {
        let sql = """
            SELECT is_opted_in, scope, include_videos, pause_discovery,
                include_still_images, include_live_photos, rendition_policy,
                target_library_identifier, target_parent_identifier, network_restriction,
                allow_cellular_transfer, respect_low_data_mode, maximum_resource_size_bytes,
                maximum_concurrent_operations, automatic_foreground_backup_enabled,
                automatic_backup_consent_confirmed, pause_automatic_backup, policy_revision
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
            let includeStillImages = sqlite3_column_int(statement, 4) != 0
            let includeLivePhotos = sqlite3_column_int(statement, 5) != 0
            let renditionPolicy =
                PhotoBackupRenditionPolicy(
                    rawValue: String(cString: sqlite3_column_text(statement, 6))) ?? .originalOnly
            let networkRestriction =
                PhotoBackupNetworkRestriction(
                    rawValue: String(cString: sqlite3_column_text(statement, 9))) ?? .wifiOnly
            let maximumResourceSize = sqlite3_column_int64(statement, 12)
            let maximumConcurrent = sqlite3_column_int(statement, 13)
            let policyRevision = sqlite3_column_int64(statement, 17)

            return PhotoBackupConfiguration(
                isOptedIn: isOptedIn,
                scope: scope,
                includeStillImages: includeStillImages,
                includeVideos: includeVideos,
                includeLivePhotos: includeLivePhotos,
                renditionPolicy: renditionPolicy,
                targetLibraryIdentifier: nullableString(statement, 7),
                targetParentIdentifier: nullableString(statement, 8),
                networkRestriction: networkRestriction,
                allowCellularTransfer: sqlite3_column_int(statement, 10) != 0,
                respectLowDataMode: sqlite3_column_int(statement, 11) != 0,
                maximumResourceSizeBytes: maximumResourceSize > 0
                    ? UInt64(maximumResourceSize) : 536_870_912,
                maximumConcurrentOperations: Int(maximumConcurrent),
                automaticForegroundBackupEnabled: sqlite3_column_int(statement, 14) != 0,
                automaticBackupConsentConfirmed: sqlite3_column_int(statement, 15) != 0,
                pauseAutomaticBackup: sqlite3_column_int(statement, 16) != 0,
                policyRevision: max(policyRevision, 1),
                pauseDiscovery: pauseDiscovery)
        } else {
            return PhotoBackupConfiguration()
        }
    }

    private func performConfigurationExists(profileId: UUID) throws -> Bool {
        let sql = "SELECT 1 FROM photo_backup_configurations WHERE profile_id=? LIMIT 1;"
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK else {
            throw photoStoreError(177, "Failed to prepare Photo Backup configuration lookup")
        }
        defer { sqlite3_finalize(statement) }
        bindText(statement, index: 1, text: profileId.uuidString)
        return sqlite3_step(statement) == SQLITE_ROW
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

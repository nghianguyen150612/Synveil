import Foundation
import SQLite3

private let sqliteTransient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

public final class PhotoObservationSQLiteStore: PhotoObservationStoreProtocol, @unchecked Sendable {
    private let dbQueue: DispatchQueue
    private var db: OpaquePointer?

    public init(databaseURL: URL) throws {
        self.dbQueue = DispatchQueue(label: "com.synveil.ios.photo.sqlite", qos: .utility)
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
        try dbQueue.sync {
            try PhotoObservationSQLiteSchema.createTables(db: self.db)
        }
    }

    deinit {
        if let db = db {
            sqlite3_close(db)
        }
    }

    public func saveObservations(
        _ observations: [PhotoAssetObservation],
        profileId: UUID,
        ownerId: String
    ) async throws {
        try await withCheckedThrowingContinuation { continuation in
            dbQueue.async {
                do {
                    try self.performSaveObservations(
                        observations,
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
                        profileId: profileId
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
        profileId: UUID,
        ownerId: String
    ) throws {
        let sql = """
            INSERT INTO photo_asset_observations (
                local_identifier, profile_id, owner_id, asset_kind,
                creation_date, modification_date, pixel_width, pixel_height,
                is_favorite, is_hidden, discovery_generation, observed_at,
                permission_scope, source_availability, is_excluded
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(local_identifier, profile_id) DO UPDATE SET
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

        sqlite3_exec(db, "BEGIN TRANSACTION;", nil, nil, nil)

        for obs in observations {
            sqlite3_reset(statement)

            let pIdStr = profileId.uuidString
            bindText(statement, index: 1, text: obs.localIdentifier)
            bindText(statement, index: 2, text: pIdStr)
            bindText(statement, index: 3, text: ownerId)
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
                sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
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

        sqlite3_exec(db, "COMMIT;", nil, nil, nil)
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
        profileId: UUID
    ) throws {
        let sql = """
            UPDATE photo_asset_observations SET source_availability=? \
            WHERE local_identifier=? AND profile_id=?;
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
        text.withCString { cString in
            sqlite3_bind_text(statement, index, cString, -1, sqliteTransient)
        }
    }
}

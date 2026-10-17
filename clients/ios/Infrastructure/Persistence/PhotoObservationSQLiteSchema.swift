import Foundation
import SQLite3

public struct PhotoObservationSQLiteSchema {
    public static let schemaVersion: Int32 = 2

    public static let createTableObservations = """
        CREATE TABLE IF NOT EXISTS photo_asset_observations (
            local_identifier TEXT NOT NULL,
            profile_id TEXT NOT NULL,
            owner_id TEXT NOT NULL,
            asset_kind TEXT NOT NULL,
            creation_date DOUBLE,
            modification_date DOUBLE,
            pixel_width INTEGER NOT NULL,
            pixel_height INTEGER NOT NULL,
            is_favorite INTEGER NOT NULL,
            is_hidden INTEGER NOT NULL,
            discovery_generation INTEGER NOT NULL,
            observed_at DOUBLE NOT NULL,
            permission_scope TEXT NOT NULL,
            source_availability TEXT NOT NULL,
            is_excluded INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (local_identifier, profile_id)
        );
        """

    public static let createTableConfigurations = """
        CREATE TABLE IF NOT EXISTS photo_backup_configurations (
            profile_id TEXT PRIMARY KEY,
            is_opted_in INTEGER NOT NULL,
            scope TEXT NOT NULL,
            include_videos INTEGER NOT NULL,
            pause_discovery INTEGER NOT NULL,
            updated_at DOUBLE NOT NULL
        );
        """

    public static let createTableAuthorizationSession = """
        CREATE TABLE IF NOT EXISTS photo_authorization_session (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            profile_id TEXT NOT NULL,
            owner_id TEXT NOT NULL,
            session_generation TEXT NOT NULL,
            is_active INTEGER NOT NULL,
            updated_at DOUBLE NOT NULL
        );
        """

    public static let createTableExtractions = """
        CREATE TABLE IF NOT EXISTS photo_resource_extractions (
            operation_id TEXT PRIMARY KEY,
            profile_id TEXT NOT NULL,
            owner_id TEXT NOT NULL,
            device_id TEXT NOT NULL,
            local_identifier TEXT NOT NULL,
            resource_index INTEGER NOT NULL,
            resource_type TEXT NOT NULL,
            resource_role TEXT NOT NULL,
            rendition TEXT NOT NULL,
            uniform_type_identifier TEXT,
            source_modification_date DOUBLE,
            observation_generation INTEGER NOT NULL,
            policy_version INTEGER NOT NULL,
            maximum_resource_bytes INTEGER NOT NULL,
            user_initiated INTEGER NOT NULL,
            authorization_generation TEXT NOT NULL,
            staging_id TEXT NOT NULL UNIQUE,
            byte_count INTEGER,
            sha256 TEXT,
            state TEXT NOT NULL,
            related_upload_operation_id TEXT,
            failure_code TEXT,
            created_at DOUBLE NOT NULL,
            updated_at DOUBLE NOT NULL
        );
        CREATE INDEX IF NOT EXISTS photo_resource_extractions_scope
            ON photo_resource_extractions(profile_id, owner_id, state);
        """

    public static func createTables(db: OpaquePointer?) throws {
        guard let db else { throw schemaError(1, "Missing SQLite database") }
        var versionStatement: OpaquePointer?
        guard
            sqlite3_prepare_v2(db, "PRAGMA user_version;", -1, &versionStatement, nil)
                == SQLITE_OK,
            sqlite3_step(versionStatement) == SQLITE_ROW
        else { throw schemaError(2, "Failed to read PhotoKit schema version") }
        let version = sqlite3_column_int(versionStatement, 0)
        sqlite3_finalize(versionStatement)
        guard version <= schemaVersion else {
            throw schemaError(3, "PhotoKit database schema is newer than this application")
        }

        try execute(db, "BEGIN IMMEDIATE TRANSACTION;")
        do {
            try execute(db, createTableObservations)
            try execute(db, createTableConfigurations)
            try execute(db, createTableAuthorizationSession)
            try execute(db, createTableExtractions)
            try execute(db, "PRAGMA user_version = \(schemaVersion);")
            try execute(db, "COMMIT;")
        } catch {
            _ = sqlite3_exec(db, "ROLLBACK;", nil, nil, nil)
            throw error
        }
    }

    private static func execute(_ db: OpaquePointer, _ sql: String) throws {
        var errorMessage: UnsafeMutablePointer<CChar>?
        guard sqlite3_exec(db, sql, nil, nil, &errorMessage) == SQLITE_OK else {
            let message =
                errorMessage.map { String(cString: $0) }
                ?? "SQLite schema operation failed"
            sqlite3_free(errorMessage)
            throw schemaError(4, message)
        }
    }

    private static func schemaError(_ code: Int, _ message: String) -> NSError {
        NSError(
            domain: "PhotoObservationSQLiteSchema", code: code,
            userInfo: [NSLocalizedDescriptionKey: message])
    }
}

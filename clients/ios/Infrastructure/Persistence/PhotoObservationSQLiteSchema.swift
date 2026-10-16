import Foundation
import SQLite3

public struct PhotoObservationSQLiteSchema {
    public static let schemaVersion: Int32 = 1

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

    public static func createTables(db: OpaquePointer?) throws {
        var errorMessage: UnsafeMutablePointer<CChar>?
        if sqlite3_exec(db, createTableObservations, nil, nil, &errorMessage) != SQLITE_OK {
            let message = String(cString: errorMessage!)
            sqlite3_free(errorMessage)
            throw NSError(domain: "PhotoObservationSQLiteSchema", code: 1, userInfo: [NSLocalizedDescriptionKey: "Failed to create observations table: \(message)"])
        }

        if sqlite3_exec(db, createTableConfigurations, nil, nil, &errorMessage) != SQLITE_OK {
            let message = String(cString: errorMessage!)
            sqlite3_free(errorMessage)
            throw NSError(domain: "PhotoObservationSQLiteSchema", code: 2, userInfo: [NSLocalizedDescriptionKey: "Failed to create configurations table: \(message)"])
        }
    }
}

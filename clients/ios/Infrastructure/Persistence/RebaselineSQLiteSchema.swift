import Foundation

enum RebaselineSQLiteSchema {
    static let tables = [
        """
        CREATE TABLE rebaseline_start_attempts (
          scope_id INTEGER NOT NULL REFERENCES scopes(scope_id), attempt_id TEXT PRIMARY KEY,
          resolved INTEGER NOT NULL DEFAULT 0 CHECK(resolved IN (0,1))
        )
        """,
        """
        CREATE TABLE rebaseline_sessions (
          scope_id INTEGER NOT NULL REFERENCES scopes(scope_id), bootstrap_id TEXT PRIMARY KEY,
          generation TEXT NOT NULL, epoch TEXT NOT NULL, cut TEXT NOT NULL,
          bootstrap BLOB NOT NULL CHECK(length(bootstrap) BETWEEN 1 AND 16384),
          library BLOB NOT NULL CHECK(length(library) BETWEEN 1 AND 16384),
          stage TEXT NOT NULL CHECK(stage IN ('BOOTSTRAP_OPEN','DOWNLOADING','TERMINAL_RECEIVED','PREPARED_FOR_HANDOFF','SERVER_COMPLETION_IN_FLIGHT','SERVER_COMPLETION_CONFIRMED','ACTIVE_COMPLETE','EXPIRED','BLOCKED','OUTCOME_UNKNOWN','RECONCILIATION_REQUIRED')),
          cursor TEXT, terminal_token TEXT CHECK(terminal_token IS NULL OR length(terminal_token) BETWEEN 1 AND 336),
          prepared_id TEXT, completion BLOB CHECK(completion IS NULL OR length(completion) BETWEEN 1 AND 16384),
          UNIQUE(scope_id,generation)
        )
        """,
        """
        CREATE TABLE rebaseline_pages (
          bootstrap_id TEXT NOT NULL REFERENCES rebaseline_sessions(bootstrap_id), page_index INTEGER NOT NULL,
          request_cursor TEXT NOT NULL, next_cursor TEXT, response BLOB NOT NULL CHECK(length(response) BETWEEN 1 AND 2097152),
          canonical BLOB NOT NULL CHECK(length(canonical) BETWEEN 1 AND 2097152),
          PRIMARY KEY(bootstrap_id,page_index), UNIQUE(bootstrap_id,request_cursor)
        )
        """,
        """
        CREATE TABLE rebaseline_nodes (
          bootstrap_id TEXT NOT NULL REFERENCES rebaseline_sessions(bootstrap_id), node_id TEXT NOT NULL,
          parent_id TEXT, kind TEXT NOT NULL CHECK(kind IN ('FILE','DIRECTORY')),
          lifecycle TEXT NOT NULL CHECK(lifecycle IN ('ACTIVE','TRASHED')), revision TEXT NOT NULL,
          metadata BLOB NOT NULL CHECK(length(metadata) BETWEEN 1 AND 16384),
          PRIMARY KEY(bootstrap_id,node_id)
        )
        """,
        "CREATE INDEX rebaseline_children ON rebaseline_nodes(bootstrap_id,parent_id,lifecycle,node_id)",
        """
        CREATE TABLE rebaseline_completion_attempts (
          bootstrap_id TEXT NOT NULL REFERENCES rebaseline_sessions(bootstrap_id), attempt_id TEXT PRIMARY KEY,
          owner TEXT NOT NULL, prepared_id TEXT NOT NULL,
          completion BLOB CHECK(completion IS NULL OR length(completion) BETWEEN 1 AND 16384)
        )
        """,
        """
        CREATE TABLE rebaseline_active (
          scope_id INTEGER PRIMARY KEY REFERENCES scopes(scope_id),
          bootstrap_id TEXT NOT NULL UNIQUE REFERENCES rebaseline_sessions(bootstrap_id)
        )
        """,
        """
        CREATE TRIGGER rebaseline_session_identity BEFORE UPDATE OF scope_id,bootstrap_id,generation,epoch,cut,bootstrap,library ON rebaseline_sessions
        BEGIN SELECT RAISE(ABORT,'immutable snapshot identity'); END
        """,
    ]
    static var statements: [String] {
        tables
            + [
                "rebaseline_start_attempts", "rebaseline_sessions", "rebaseline_pages",
                "rebaseline_nodes", "rebaseline_completion_attempts", "rebaseline_active",
            ].flatMap { table in
                ["INSERT", "UPDATE", "DELETE"].map { operation in
                    let immutable =
                        operation != "INSERT"
                        && ["rebaseline_pages", "rebaseline_nodes"].contains(table)
                    return
                        "CREATE TRIGGER \(table)_\(operation.lowercased())_gate BEFORE \(operation) ON \(table)\nWHEN \(immutable || operation == "DELETE" ? "1" : "synveil_projection_authorized()<>5")\nBEGIN SELECT RAISE(ABORT,'snapshot transaction required'); END"
                }
            }
    }
    /// Preserve the original P039 definition for exact v4 migration validation.
    static var projection: [String] {
        NodeProjectionSQLiteSchema.statements.map { sql in
            if sql.contains("CREATE TRIGGER cached_nodes_delete_gate") {
                return sql.replacingOccurrences(
                    of: "WHEN 1", with: "WHEN synveil_projection_authorized()<>5")
            }
            if sql.contains("CREATE TRIGGER cached_nodes_")
                || sql.contains("CREATE TRIGGER cached_libraries_")
                || sql.contains("CREATE TRIGGER node_projection_state_")
            {
                return sql.replacingOccurrences(of: "NOT IN (1)", with: "NOT IN (1,5)")
                    .replacingOccurrences(of: "NOT IN (1,3,4)", with: "NOT IN (1,3,4,5)")
            }
            return sql
        }
    }
}

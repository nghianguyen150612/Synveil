import Foundation

enum FileUploadSQLiteSchema {
    static let statements = [
        """
        CREATE TABLE file_uploads (
          operation_id TEXT PRIMARY KEY,
          scope_id INTEGER NOT NULL REFERENCES scopes(scope_id),
          idempotency_key TEXT NOT NULL,
          operation TEXT NOT NULL CHECK(operation IN ('CREATE_FILE','REPLACE_CONTENT')),
          library_id TEXT NOT NULL,
          parent_id TEXT,
          node_id TEXT,
          target_name TEXT,
          expected_revision TEXT,
          expected_bytes TEXT NOT NULL,
          expected_sha256 TEXT NOT NULL,
          request_bytes BLOB NOT NULL CHECK(length(request_bytes) BETWEEN 1 AND 16384),
          staging_id TEXT NOT NULL UNIQUE,
          credential_id TEXT NOT NULL,
          originating_session_id TEXT NOT NULL,
          server_session_id TEXT UNIQUE,
          server_target_node_id TEXT,
          authoritative_offset TEXT NOT NULL,
          server_state TEXT CHECK(server_state IS NULL OR server_state IN ('OPEN','VERIFYING','COMMITTING','COMMITTED','FAILED','EXPIRED','ABORTED')),
          state TEXT NOT NULL CHECK(state IN ('PREPARING_SOURCE','SOURCE_STAGED','SESSION_CREATING','SESSION_OPEN','UPLOADING','READY_TO_COMPLETE','COMPLETING','COMMITTED','OUTCOME_UNKNOWN','RECOVERY_REQUIRED','CANCELLED_LOCAL','ABORT_PENDING','ABORTED','EXPIRED','FAILED_RECOVERABLE','FAILED_PERMANENT','STORAGE_UNAVAILABLE','SESSION_INVALIDATED')),
          uncertain INTEGER NOT NULL CHECK(uncertain IN (0,1)),
          attempt_count INTEGER NOT NULL CHECK(attempt_count BETWEEN 0 AND 1000),
          completion BLOB CHECK(completion IS NULL OR length(completion) BETWEEN 1 AND 65536),
          failure TEXT,
          staging_released INTEGER NOT NULL DEFAULT 0 CHECK(staging_released IN (0,1)),
          created_at REAL NOT NULL,
          updated_at REAL NOT NULL,
          UNIQUE(scope_id,idempotency_key),
          CHECK((operation='CREATE_FILE' AND parent_id IS NOT NULL AND target_name IS NOT NULL AND node_id IS NULL AND expected_revision IS NULL)
            OR (operation='REPLACE_CONTENT' AND parent_id IS NULL AND target_name IS NULL AND node_id IS NOT NULL AND expected_revision IS NOT NULL)),
          CHECK(staging_released=0 OR state='COMMITTED'),
          CHECK(state!='COMMITTED' OR (completion IS NOT NULL AND server_state='COMMITTED'
            AND authoritative_offset=expected_bytes AND server_session_id IS NOT NULL))
        )
        """,
        """
        CREATE TRIGGER file_upload_identity BEFORE UPDATE ON file_uploads
        WHEN NEW.operation_id!=OLD.operation_id OR NEW.scope_id!=OLD.scope_id
          OR NEW.idempotency_key!=OLD.idempotency_key OR NEW.operation!=OLD.operation
          OR NEW.library_id!=OLD.library_id OR NEW.parent_id IS NOT OLD.parent_id
          OR NEW.node_id IS NOT OLD.node_id OR NEW.target_name IS NOT OLD.target_name
          OR NEW.expected_revision IS NOT OLD.expected_revision
          OR NEW.expected_bytes!=OLD.expected_bytes OR NEW.expected_sha256!=OLD.expected_sha256
          OR NEW.request_bytes!=OLD.request_bytes OR NEW.staging_id!=OLD.staging_id
          OR NEW.credential_id!=OLD.credential_id
          OR NEW.originating_session_id!=OLD.originating_session_id OR NEW.created_at!=OLD.created_at
          OR (OLD.server_session_id IS NOT NULL AND NEW.server_session_id IS NOT OLD.server_session_id)
          OR (OLD.server_target_node_id IS NOT NULL
            AND NEW.server_target_node_id IS NOT OLD.server_target_node_id)
        BEGIN SELECT RAISE(ABORT,'immutable upload identity'); END
        """,
        """
        CREATE TRIGGER file_upload_offset BEFORE UPDATE ON file_uploads
        WHEN CAST(NEW.authoritative_offset AS INTEGER)<CAST(OLD.authoritative_offset AS INTEGER)
          OR NEW.attempt_count<OLD.attempt_count
        BEGIN SELECT RAISE(ABORT,'upload progress cannot rewind'); END
        """,
    ]
}

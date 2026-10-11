import Foundation

enum FileDownloadSQLiteSchema {
    static let statements = [
        """
        CREATE TABLE file_downloads (
          operation_id TEXT PRIMARY KEY,
          scope_id INTEGER NOT NULL REFERENCES scopes(scope_id),
          node_id TEXT NOT NULL, version_id TEXT NOT NULL,
          expected_length TEXT NOT NULL, expected_sha256 TEXT NOT NULL,
          credential_id TEXT NOT NULL, session_id TEXT NOT NULL,
          durable_offset TEXT NOT NULL, prefix_sha256 TEXT NOT NULL,
          state TEXT NOT NULL CHECK(state IN ('PREPARED','DOWNLOADING','PARTIAL','VERIFYING','VERIFIED','CANCELLED','FAILED_RECOVERABLE','FAILED_PERMANENT','INTEGRITY_FAILURE','STORAGE_UNAVAILABLE','SESSION_INVALIDATED','SOURCE_UNAVAILABLE')),
          attempt_count INTEGER NOT NULL CHECK(attempt_count BETWEEN 0 AND 1000),
          failure TEXT, created_at REAL NOT NULL, updated_at REAL NOT NULL,
          UNIQUE(scope_id,version_id)
        )
        """,
        """
        CREATE TRIGGER file_download_identity BEFORE UPDATE ON file_downloads
        WHEN NEW.operation_id!=OLD.operation_id OR NEW.scope_id!=OLD.scope_id
          OR NEW.node_id!=OLD.node_id OR NEW.version_id!=OLD.version_id
          OR NEW.expected_length!=OLD.expected_length OR NEW.expected_sha256!=OLD.expected_sha256
          OR NEW.credential_id!=OLD.credential_id OR NEW.session_id!=OLD.session_id
          OR NEW.created_at!=OLD.created_at
        BEGIN SELECT RAISE(ABORT,'immutable download identity'); END
        """,
        """
        CREATE TRIGGER file_download_verified BEFORE UPDATE ON file_downloads
        WHEN NEW.state='VERIFIED' AND (OLD.state!='VERIFYING'
          OR NEW.durable_offset!=NEW.expected_length OR NEW.prefix_sha256!=NEW.expected_sha256)
        BEGIN SELECT RAISE(ABORT,'unverified download'); END
        """,
    ]
}

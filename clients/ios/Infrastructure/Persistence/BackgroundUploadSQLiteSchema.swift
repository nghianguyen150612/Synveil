import Foundation

enum BackgroundUploadSQLiteSchema {
    static let statements = [
        """
        CREATE TABLE background_upload_jobs (
          operation_id TEXT PRIMARY KEY REFERENCES file_uploads(operation_id),
          generation INTEGER NOT NULL CHECK(generation BETWEEN 0 AND 1000),
          blocked INTEGER NOT NULL CHECK(blocked IN (0,1)),
          metadata BLOB NOT NULL CHECK(length(metadata) BETWEEN 1 AND 4096)
        )
        """,
        """
        CREATE TABLE background_upload_orphans (
          task_identifier INTEGER PRIMARY KEY CHECK(task_identifier>0)
        )
        """,
    ]
}

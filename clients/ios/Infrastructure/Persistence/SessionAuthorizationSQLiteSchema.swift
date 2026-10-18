import Foundation

/// v9 adds only authorization-check evidence; every P046 v8 intent/table/guard remains unchanged.
enum SessionAuthorizationSQLiteSchema {
    static let statements = [
        """
        CREATE TABLE session_authorization_checks (
          endpoint TEXT NOT NULL CHECK(length(endpoint) BETWEEN 1 AND 4096),
          owner_id TEXT NOT NULL CHECK(length(owner_id)=36),
          device_id TEXT NOT NULL CHECK(length(device_id)=36),
          credential_id TEXT NOT NULL CHECK(length(credential_id)=36),
          attempt_id TEXT NOT NULL CHECK(length(attempt_id)=36),
          state TEXT NOT NULL CHECK(state IN ('CHECKING','AUTHORIZED','TRANSIENT','REJECTED','VERIFY_REQUIRED')),
          requires_verification INTEGER NOT NULL CHECK(requires_verification IN (0,1)),
          PRIMARY KEY(endpoint,owner_id,device_id,credential_id)
        )
        """,
        """
        CREATE TRIGGER session_authorization_insert BEFORE INSERT ON session_authorization_checks
        WHEN synveil_projection_authorized()!=9
        BEGIN SELECT RAISE(ABORT,'authorization evidence authority required'); END
        """,
        """
        CREATE TRIGGER session_authorization_update BEFORE UPDATE ON session_authorization_checks
        WHEN synveil_projection_authorized()!=9 OR NEW.endpoint!=OLD.endpoint OR NEW.owner_id!=OLD.owner_id OR NEW.device_id!=OLD.device_id OR NEW.credential_id!=OLD.credential_id
        BEGIN SELECT RAISE(ABORT,'authorization evidence authority required'); END
        """,
        """
        CREATE TRIGGER session_authorization_delete BEFORE DELETE ON session_authorization_checks
        BEGIN SELECT RAISE(ABORT,'authorization evidence retained'); END
        """,
    ]
}

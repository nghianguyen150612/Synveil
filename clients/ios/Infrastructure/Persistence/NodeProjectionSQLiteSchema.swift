import Foundation

/// Exact SQL is verified on open. All protected writes require connection-local transaction authority.
enum NodeProjectionSQLiteSchema {
    private static let base: [String] = [
        """
        CREATE TABLE node_projection_state (
          scope_id INTEGER PRIMARY KEY REFERENCES scopes(scope_id), epoch TEXT NOT NULL,
          anchor_sequence TEXT NOT NULL, applied_sequence TEXT NOT NULL, confirmed_sequence TEXT NOT NULL,
          completeness TEXT NOT NULL CHECK(completeness IN ('UNINITIALIZED','PARTIAL','COMPLETE','REBASELINE_REQUIRED')),
          encoding_version INTEGER NOT NULL CHECK(encoding_version=1),
          UNIQUE(scope_id,epoch)
        )
        """,
        """
        CREATE TABLE cached_libraries (
          scope_id INTEGER NOT NULL REFERENCES scopes(scope_id), epoch TEXT NOT NULL,
          metadata BLOB NOT NULL CHECK(length(metadata) BETWEEN 1 AND 16384),
          encoding_version INTEGER NOT NULL CHECK(encoding_version=1),
          PRIMARY KEY(scope_id,epoch)
        )
        """,
        """
        CREATE TABLE cached_nodes (
          scope_id INTEGER NOT NULL, epoch TEXT NOT NULL, node_id TEXT NOT NULL,
          revision TEXT NOT NULL, sequence TEXT NOT NULL, parent_id TEXT,
          lifecycle TEXT NOT NULL CHECK(lifecycle IN ('ACTIVE','TRASHED','PURGED')),
          provenance TEXT NOT NULL CHECK(provenance IN ('CANONICAL','EVENT','LAST_KNOWN')),
          metadata BLOB CHECK(metadata IS NULL OR length(metadata) BETWEEN 1 AND 16384),
          encoding_version INTEGER NOT NULL CHECK(encoding_version=1),
          PRIMARY KEY(scope_id,epoch,node_id),
          FOREIGN KEY(scope_id,epoch) REFERENCES node_projection_state(scope_id,epoch),
          CHECK(lifecycle<>'ACTIVE' OR (provenance='CANONICAL' AND metadata IS NOT NULL)),
          CHECK(lifecycle<>'PURGED' OR (metadata IS NULL AND parent_id IS NULL AND provenance='EVENT'))
        )
        """,
        "CREATE INDEX cached_active_children ON cached_nodes(scope_id,epoch,parent_id,lifecycle,node_id)",
        """
        CREATE TABLE projection_commits (
          scope_id INTEGER NOT NULL, epoch TEXT NOT NULL, from_sequence TEXT NOT NULL,
          through_sequence TEXT NOT NULL, commit_id TEXT NOT NULL UNIQUE,
          event_count INTEGER NOT NULL CHECK(event_count BETWEEN 1 AND 500),
          canonical_data BLOB NOT NULL CHECK(length(canonical_data) BETWEEN 1 AND 2097152),
          committed_at REAL NOT NULL,
          PRIMARY KEY(scope_id,epoch,from_sequence),
          FOREIGN KEY(scope_id,epoch,from_sequence) REFERENCES inbound_pages(scope_id,epoch,from_sequence)
        )
        """,
        """
        CREATE TABLE projection_events (
          scope_id INTEGER NOT NULL, epoch TEXT NOT NULL, sequence TEXT NOT NULL,
          event_id TEXT NOT NULL, from_sequence TEXT NOT NULL, node_id TEXT NOT NULL,
          revision TEXT NOT NULL, kind TEXT NOT NULL,
          PRIMARY KEY(scope_id,epoch,sequence), UNIQUE(scope_id,epoch,event_id),
          FOREIGN KEY(scope_id,epoch,from_sequence) REFERENCES projection_commits(scope_id,epoch,from_sequence)
        )
        """,
        """
        CREATE TABLE sync_ack_attempts (
          scope_id INTEGER NOT NULL, epoch TEXT NOT NULL, from_sequence TEXT NOT NULL,
          attempt_id TEXT NOT NULL, attempt_owner TEXT NOT NULL, previous_sequence TEXT NOT NULL,
          applied_sequence TEXT NOT NULL, dispatched INTEGER NOT NULL DEFAULT 0 CHECK(dispatched IN (0,1)),
          completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0,1)),
          started_at REAL NOT NULL, checkpoint BLOB CHECK(checkpoint IS NULL OR length(checkpoint) BETWEEN 1 AND 16384),
          PRIMARY KEY(scope_id,epoch,from_sequence,attempt_id),
          FOREIGN KEY(scope_id,epoch,from_sequence) REFERENCES projection_commits(scope_id,epoch,from_sequence)
        )
        """,
        """
        CREATE TRIGGER inbound_insert_gate BEFORE INSERT ON inbound_pages
        WHEN NEW.state<>'RECEIVED_UNAPPLIED'
        BEGIN SELECT RAISE(ABORT,'received page required'); END
        """,
        """
        CREATE TRIGGER inbound_application_gate BEFORE UPDATE OF state ON inbound_pages
        WHEN NEW.state<>OLD.state AND NEW.state<>'BLOCKED_REBASELINE' AND NOT (
          EXISTS(SELECT 1 FROM projection_commits c JOIN node_projection_state p ON p.scope_id=c.scope_id AND p.epoch=c.epoch
            WHERE c.scope_id=NEW.scope_id AND c.epoch=NEW.epoch AND c.from_sequence=NEW.from_sequence
              AND c.through_sequence=NEW.through_sequence AND c.canonical_data=NEW.canonical_data
              AND c.event_count=(SELECT count(*) FROM projection_events e WHERE e.scope_id=c.scope_id AND e.epoch=c.epoch AND e.from_sequence=c.from_sequence))
          AND ((OLD.state='RECEIVED_UNAPPLIED' AND NEW.state='APPLIED_ACK_PENDING' AND synveil_projection_authorized()=1
                AND EXISTS(SELECT 1 FROM node_projection_state p WHERE p.scope_id=NEW.scope_id AND p.epoch=NEW.epoch AND p.applied_sequence=NEW.through_sequence))
            OR (OLD.state='APPLIED_ACK_PENDING' AND NEW.state='ACK_IN_FLIGHT' AND synveil_projection_authorized()=2
                AND EXISTS(SELECT 1 FROM sync_ack_attempts a WHERE a.scope_id=NEW.scope_id AND a.epoch=NEW.epoch AND a.from_sequence=NEW.from_sequence AND a.completed=0))
            OR (OLD.state='ACK_IN_FLIGHT' AND NEW.state='ACK_CONFIRMED' AND synveil_projection_authorized()=3
                AND EXISTS(SELECT 1 FROM sync_ack_attempts a WHERE a.scope_id=NEW.scope_id AND a.epoch=NEW.epoch AND a.from_sequence=NEW.from_sequence AND a.completed=1 AND a.checkpoint IS NOT NULL)))
        )
        BEGIN SELECT RAISE(ABORT,'committed projection authority required'); END
        """,
        """
        CREATE TRIGGER ack_attempt_identity_immutable BEFORE UPDATE OF scope_id,epoch,from_sequence,attempt_id,attempt_owner,previous_sequence,applied_sequence,started_at ON sync_ack_attempts
        BEGIN SELECT RAISE(ABORT,'immutable ACK attempt'); END
        """,
        """
        CREATE TRIGGER ack_attempt_transition BEFORE UPDATE ON sync_ack_attempts
        WHEN OLD.completed=1 OR NEW.dispatched<OLD.dispatched OR NEW.completed<OLD.completed
          OR (NEW.completed=1 AND (NEW.dispatched<>1 OR NEW.checkpoint IS NULL))
        BEGIN SELECT RAISE(ABORT,'invalid ACK attempt transition'); END
        """,
        """
        CREATE TRIGGER purge_resurrection_gate BEFORE UPDATE ON cached_nodes
        WHEN OLD.lifecycle='PURGED'
        BEGIN SELECT RAISE(ABORT,'purge retained'); END
        """,
    ]
    static var statements: [String] {
        var result = base
        result += guarded("cached_nodes", modes: "1")
        result += guarded("cached_libraries", modes: "1")
        result += guarded("node_projection_state", modes: "1,3,4")
        result += guarded("projection_commits", modes: "1", immutable: true)
        result += guarded("projection_events", modes: "1", immutable: true)
        result += guarded("sync_ack_attempts", modes: "2,3", retained: true)
        return result
    }

    private static func guarded(
        _ table: String, modes: String, immutable: Bool = false, retained: Bool = false
    ) -> [String] {
        ["INSERT", "UPDATE", "DELETE"].map { operation in
            let forbidden =
                (immutable && operation != "INSERT")
                || ((retained || table == "cached_nodes") && operation == "DELETE")
            return
                "CREATE TRIGGER \(table)_\(operation.lowercased())_gate BEFORE \(operation) ON \(table)\nWHEN \(forbidden ? "1" : "synveil_projection_authorized() NOT IN (\(modes))")\nBEGIN SELECT RAISE(ABORT,'projection transaction required'); END"
        }
    }
}

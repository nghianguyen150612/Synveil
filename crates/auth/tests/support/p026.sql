-- Qualification only. Never a product migration.
PRAGMA recursive_triggers=ON;
CREATE TABLE grants(id TEXT PRIMARY KEY, revoked INTEGER NOT NULL CHECK(revoked IN(0,1)), revision INTEGER NOT NULL);
CREATE TABLE approvals(id TEXT PRIMARY KEY, consumed INTEGER NOT NULL, withdrawn INTEGER NOT NULL);
CREATE TABLE audit(sequence INTEGER PRIMARY KEY, event TEXT NOT NULL);
CREATE TRIGGER no_restore BEFORE UPDATE ON grants WHEN OLD.revoked=1 BEGIN SELECT RAISE(ABORT,'revocation tombstone'); END;
CREATE TRIGGER no_delete BEFORE DELETE ON grants BEGIN SELECT RAISE(ABORT,'immutable identity'); END;
CREATE TRIGGER no_replace BEFORE INSERT ON grants WHEN EXISTS(SELECT 1 FROM grants WHERE id=NEW.id) BEGIN SELECT RAISE(ABORT,'duplicate identity'); END;
INSERT INTO grants VALUES ('root',0,7),('child',0,7);
INSERT INTO approvals VALUES ('approval',0,0);

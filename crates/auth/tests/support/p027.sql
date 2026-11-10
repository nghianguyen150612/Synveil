-- P027 ONLY. Installed into a UUID-named private test schema, never migrations.
CREATE TABLE policy (
 id integer PRIMARY KEY CHECK (id = 1), epoch bigint NOT NULL DEFAULT 0,
 version integer NOT NULL, revision bigint NOT NULL CHECK (revision > 0),
 role_revision bigint NOT NULL CHECK (role_revision > 0),
 approval_required boolean NOT NULL, recursive boolean NOT NULL DEFAULT false
);
CREATE TABLE actors (
 id text PRIMARY KEY CHECK (id <> ''), human text NOT NULL CHECK (human <> ''),
 device text, revoked boolean NOT NULL DEFAULT false
);
CREATE TABLE sessions (
 id text PRIMARY KEY, actor text NOT NULL REFERENCES actors(id),
 revoked boolean NOT NULL DEFAULT false
);
CREATE TABLE ownership (
 resource text PRIMARY KEY, owner_id text NOT NULL, host text NOT NULL,
 owner_actor text NOT NULL REFERENCES actors(id), approver text NOT NULL REFERENCES actors(id),
 resource_type text NOT NULL DEFAULT 'library', action text NOT NULL DEFAULT 'read', device text,
 legacy_allow boolean NOT NULL DEFAULT true
);
CREATE TABLE grants (
 id text PRIMARY KEY CHECK (id <> ''), recipient text NOT NULL REFERENCES actors(id),
 issuer text NOT NULL REFERENCES actors(id), parent text REFERENCES grants(id),
 owner_id text NOT NULL, host text NOT NULL, resource text NOT NULL REFERENCES ownership(resource),
 resource_type text NOT NULL DEFAULT 'library', action text NOT NULL DEFAULT 'read', device text,
 role text NOT NULL DEFAULT 'viewer', revision bigint NOT NULL, role_revision bigint NOT NULL,
 issued bigint NOT NULL CHECK (issued >= 0), expires bigint NOT NULL CHECK (expires > issued),
 deny boolean NOT NULL DEFAULT false, maximum text NOT NULL DEFAULT 'read',
 CHECK (parent IS DISTINCT FROM id)
);
CREATE FUNCTION reject_delegation_cycle() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF EXISTS (
   WITH RECURSIVE ancestors AS (
     SELECT id,parent,ARRAY[id] AS seen FROM grants WHERE id=NEW.parent
     UNION ALL
     SELECT g.id,g.parent,a.seen || g.id FROM grants g JOIN ancestors a ON g.id=a.parent
       WHERE NOT g.id=ANY(a.seen)
   ) SELECT 1 FROM ancestors WHERE id=NEW.id
 ) THEN RAISE EXCEPTION 'P027 cyclic delegation' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER reject_delegation_cycle AFTER INSERT ON grants FOR EACH ROW EXECUTE FUNCTION reject_delegation_cycle();
CREATE TABLE revocations (
 grant_id text PRIMARY KEY REFERENCES grants(id), reason text NOT NULL,
 revoked_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE approvals (
 id text PRIMARY KEY CHECK (id <> ''), request_id text NOT NULL UNIQUE CHECK (request_id <> ''), operation text NOT NULL CHECK (operation <> ''),
 requester text NOT NULL REFERENCES actors(id), approver text NOT NULL REFERENCES actors(id),
 owner_id text NOT NULL, host text NOT NULL, resource text NOT NULL REFERENCES ownership(resource),
 resource_type text NOT NULL DEFAULT 'library', action text NOT NULL DEFAULT 'read', device text,
 revision bigint NOT NULL, issued bigint NOT NULL CHECK (issued >= 0),
 expires bigint NOT NULL CHECK (expires > issued), withdrawn boolean NOT NULL DEFAULT false,
 consumed_by text UNIQUE, UNIQUE(id,consumed_by),
 CHECK (requester <> approver)
);
CREATE TABLE operations (
 id text PRIMARY KEY CHECK (id <> ''), payload text NOT NULL, actor text NOT NULL REFERENCES actors(id),
 revision bigint NOT NULL, grant_id text NOT NULL REFERENCES grants(id),
 approval_id text UNIQUE REFERENCES approvals(id), outcome text NOT NULL CHECK (outcome = 'synthetic-success'),
 committed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 FOREIGN KEY (approval_id,id) REFERENCES approvals(id,consumed_by) DEFERRABLE INITIALLY DEFERRED
);
ALTER TABLE approvals ADD CONSTRAINT consumption_operation FOREIGN KEY (consumed_by)
 REFERENCES operations(id) DEFERRABLE INITIALLY DEFERRED;
CREATE TABLE audit (
 seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, event text NOT NULL,
 identity text NOT NULL, detail text NOT NULL,
 observed_at timestamptz NOT NULL DEFAULT clock_timestamp(), transaction_id bigint NOT NULL DEFAULT txid_current()
);
CREATE TABLE outbox (
 operation text PRIMARY KEY REFERENCES operations(id), event text NOT NULL CHECK (event = 'synthetic-success')
);
CREATE FUNCTION immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'P027 immutable reference history' USING ERRCODE = '23514'; END $$;
CREATE TRIGGER immutable_grants BEFORE UPDATE OR DELETE ON grants FOR EACH ROW EXECUTE FUNCTION immutable();
CREATE TRIGGER immutable_revocations BEFORE UPDATE OR DELETE ON revocations FOR EACH ROW EXECUTE FUNCTION immutable();
CREATE TRIGGER immutable_operations BEFORE UPDATE OR DELETE ON operations FOR EACH ROW EXECUTE FUNCTION immutable();
CREATE TRIGGER immutable_audit BEFORE UPDATE OR DELETE ON audit FOR EACH ROW EXECUTE FUNCTION immutable();
CREATE TRIGGER immutable_outbox BEFORE UPDATE OR DELETE ON outbox FOR EACH ROW EXECUTE FUNCTION immutable();
CREATE TRIGGER grants_no_truncate BEFORE TRUNCATE ON grants FOR EACH STATEMENT EXECUTE FUNCTION immutable();
CREATE TRIGGER revocations_no_truncate BEFORE TRUNCATE ON revocations FOR EACH STATEMENT EXECUTE FUNCTION immutable();
CREATE TRIGGER operations_no_truncate BEFORE TRUNCATE ON operations FOR EACH STATEMENT EXECUTE FUNCTION immutable();
CREATE TRIGGER audit_no_truncate BEFORE TRUNCATE ON audit FOR EACH STATEMENT EXECUTE FUNCTION immutable();
CREATE TRIGGER outbox_no_truncate BEFORE TRUNCATE ON outbox FOR EACH STATEMENT EXECUTE FUNCTION immutable();
CREATE TRIGGER approvals_no_truncate BEFORE TRUNCATE ON approvals FOR EACH STATEMENT EXECUTE FUNCTION immutable();
CREATE FUNCTION approval_monotonic() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP = 'DELETE' THEN RAISE EXCEPTION 'P027 approval deletion forbidden' USING ERRCODE = '23514'; END IF;
 IF (to_jsonb(OLD) - 'withdrawn' - 'consumed_by') IS DISTINCT FROM
    (to_jsonb(NEW) - 'withdrawn' - 'consumed_by') OR
    (OLD.withdrawn AND NOT NEW.withdrawn) OR
    (OLD.consumed_by IS NOT NULL AND OLD.consumed_by IS DISTINCT FROM NEW.consumed_by)
 THEN RAISE EXCEPTION 'P027 approval evidence immutable/monotonic' USING ERRCODE = '23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER approval_monotonic BEFORE UPDATE OR DELETE ON approvals FOR EACH ROW EXECUTE FUNCTION approval_monotonic();
CREATE FUNCTION policy_monotonic() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.revision < OLD.revision OR NEW.role_revision < OLD.role_revision THEN
 RAISE EXCEPTION 'P027 stale policy replacement' USING ERRCODE = '23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER policy_monotonic BEFORE UPDATE ON policy FOR EACH ROW EXECUTE FUNCTION policy_monotonic();
INSERT INTO policy(id,version,revision,role_revision,approval_required) VALUES (1,2,7,1,true);
INSERT INTO actors(id,human,device) VALUES
 ('owner-actor','owner-human',NULL),('alice','alice-human',NULL),
 ('bob','bob-human',NULL),('carol','carol-human',NULL),('alice-session2','alice-human',NULL),
 ('device-actor','alice-human','device-a');
INSERT INTO sessions SELECT 'session-' || id,id,false FROM actors;
INSERT INTO ownership(resource,owner_id,host,owner_actor,approver) VALUES ('lib','owner-record','host','owner-actor','carol');
INSERT INTO grants(id,recipient,issuer,parent,owner_id,host,resource,revision,role_revision,issued,expires)
 VALUES ('root','alice','owner-actor',NULL,'owner-record','host','lib',7,1,1,4102444800),
 ('child','bob','alice','root','owner-record','host','lib',7,1,1,4102444800);
INSERT INTO approvals(id,request_id,operation,requester,approver,owner_id,host,resource,revision,issued,expires)
 VALUES ('approval','request','operation','bob','carol','owner-record','host','lib',7,1,4102444800);
INSERT INTO audit(event,identity,detail) VALUES ('grant-created','root','trusted fixture owner issuance'),
 ('delegated','child','trusted fixture parent issuance'),('approval-decided','approval','trusted independent fixture decision');

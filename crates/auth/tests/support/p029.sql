-- P029 evidence extension inside a generated P027 qualification schema only.
-- It is not a second authority ledger and is never installed as a migration.
CREATE TABLE p029_heads (
 owner_id text PRIMARY KEY CHECK (owner_id <> ''),
 last_sequence bigint NOT NULL CHECK (last_sequence >= 0),
 last_digest char(64) NOT NULL CHECK (last_digest ~ '^[0-9a-f]{64}$')
);
INSERT INTO p029_heads(owner_id,last_sequence,last_digest)
 SELECT DISTINCT owner_id,0,
 '0000000000000000000000000000000000000000000000000000000000000000'
 FROM ownership;

CREATE TABLE p029_events (
 owner_id text NOT NULL REFERENCES p029_heads(owner_id),
 durable_sequence bigint NOT NULL CHECK (durable_sequence > 0),
 authority_order bigint NOT NULL UNIQUE CHECK (authority_order > 0),
 event_id text NOT NULL UNIQUE CHECK (event_id <> ''),
 schema_version smallint NOT NULL CHECK (schema_version = 1),
 event_type text NOT NULL CHECK (event_type IN (
  'authentication','authorization','ownership-change','role-assignment','delegation',
  'approval-issued','approval-consumed','approval-rejected','grant-revocation','credential-revocation',
  'session-revocation','device-revocation','policy-revision','privileged-operation-rejected'
 )),
 actor_id text,
 principal_kind text NOT NULL CHECK (principal_kind IN ('browser','device','service','unauthenticated','unknown')),
 device_id text,
 resource_id text,
 operation text NOT NULL CHECK (operation IN (
  'authenticate','read','write','delete','change-owner','assign-role','delegate',
  'approve','revoke','change-policy','privileged'
 )),
 policy_revision bigint,
 outcome text NOT NULL CHECK (outcome IN ('allowed','denied','committed','recovered','rejected')),
 denial text CHECK (denial IS NULL OR denial IN (
  'unauthenticated','unauthorized','foreign-or-unknown-resource','revoked','expired',
  'stale-policy','approval-required','approval-invalid','unsupported','audit-unavailable'
 )),
 operation_id text NOT NULL,
 correlation_id text NOT NULL,
 observed_at_unix_ns bigint NOT NULL CHECK (observed_at_unix_ns >= 0),
 timestamp_provenance text NOT NULL CHECK (timestamp_provenance = 'database-clock'),
 related_reference text,
 previous_digest char(64) NOT NULL CHECK (previous_digest ~ '^[0-9a-f]{64}$'),
 digest char(64) NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
 PRIMARY KEY(owner_id,durable_sequence),
 CHECK (owner_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK (event_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK (actor_id IS NULL OR actor_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK (device_id IS NULL OR device_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK ((principal_kind='device') = (device_id IS NOT NULL)),
 CHECK ((principal_kind='unauthenticated') = (actor_id IS NULL)),
 CHECK (resource_id IS NULL OR resource_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK (operation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK (correlation_id ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK (related_reference IS NULL OR related_reference ~ '^[A-Za-z0-9_.:-]{1,128}$'),
 CHECK (policy_revision IS NULL OR policy_revision > 0),
 CHECK (previous_digest ~ '^[0-9a-f]{64}$' AND digest ~ '^[0-9a-f]{64}$'),
 CHECK ((outcome IN ('denied','rejected')) = (denial IS NOT NULL))
);
CREATE UNIQUE INDEX p029_one_committed_operation
 ON p029_events(owner_id,operation_id,event_type) WHERE outcome='committed';

CREATE FUNCTION p029_immutable_evidence() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'P029 reference evidence is append-only' USING ERRCODE='23514'; END $$;
CREATE TRIGGER p029_events_immutable BEFORE UPDATE OR DELETE ON p029_events
 FOR EACH ROW EXECUTE FUNCTION p029_immutable_evidence();
CREATE TRIGGER p029_heads_no_delete BEFORE DELETE ON p029_heads
 FOR EACH ROW EXECUTE FUNCTION p029_immutable_evidence();
CREATE FUNCTION p029_check_head_advance() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.owner_id <> OLD.owner_id OR NEW.last_sequence <> OLD.last_sequence + 1 OR
    NOT EXISTS (SELECT 1 FROM p029_events e WHERE e.owner_id=NEW.owner_id
      AND e.durable_sequence=NEW.last_sequence AND e.authority_order > 0
      AND e.previous_digest=OLD.last_digest AND e.digest=NEW.last_digest
      AND e.authority_order > COALESCE((SELECT prior.authority_order FROM p029_events prior
       WHERE prior.owner_id=OLD.owner_id AND prior.durable_sequence=OLD.last_sequence),0))
 THEN RAISE EXCEPTION 'P029 checkpoint must advance with exactly one linked record' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER p029_heads_advance BEFORE UPDATE ON p029_heads
 FOR EACH ROW EXECUTE FUNCTION p029_check_head_advance();
CREATE TRIGGER p029_events_no_truncate BEFORE TRUNCATE ON p029_events
 FOR EACH STATEMENT EXECUTE FUNCTION p029_immutable_evidence();

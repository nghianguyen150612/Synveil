-- Test-only delivery projection. The P029 event remains the evidence authority.
-- Lock out concurrent inserts until the empty-stream check and trigger installation commit.
BEGIN;
LOCK TABLE p029_events IN ACCESS EXCLUSIVE MODE;
DO $$ BEGIN
 IF EXISTS (SELECT 1 FROM p029_events) THEN
  RAISE EXCEPTION 'P030 installation requires an empty P029 event stream';
 END IF;
END $$;
CREATE TABLE p030_delivery (
 event_id text PRIMARY KEY REFERENCES p029_events(event_id),
 owner_id text NOT NULL,
 durable_sequence bigint NOT NULL,
 digest char(64) NOT NULL,
 schema_version smallint NOT NULL CHECK (schema_version = 1),
 state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','claimed','delivering','acknowledged','blocked')),
 generation bigint NOT NULL DEFAULT 0 CHECK (generation >= 0),
 lease_until timestamptz,
 attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
 next_attempt timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(owner_id,durable_sequence),
 FOREIGN KEY(owner_id,durable_sequence) REFERENCES p029_events(owner_id,durable_sequence),
 CHECK ((state IN ('claimed','delivering')) = (lease_until IS NOT NULL))
);
CREATE FUNCTION p030_enqueue() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 INSERT INTO p030_delivery(event_id,owner_id,durable_sequence,digest,schema_version)
 VALUES (NEW.event_id,NEW.owner_id,NEW.durable_sequence,NEW.digest,NEW.schema_version);
 RETURN NEW;
END $$;
CREATE TRIGGER p030_enqueue AFTER INSERT ON p029_events
 FOR EACH ROW EXECUTE FUNCTION p030_enqueue();

COMMIT;

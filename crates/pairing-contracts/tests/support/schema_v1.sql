-- Qualification database only. Never a production migration.
PRAGMA user_version = 1;
CREATE TABLE authority (
 installation TEXT PRIMARY KEY, owner TEXT NOT NULL, endpoint TEXT NOT NULL,
 fingerprint TEXT NOT NULL, policy INTEGER NOT NULL CHECK(policy > 0),
 active INTEGER NOT NULL CHECK(active IN (0,1))
);
-- Trusted harness approvals stand in for an UNAPPROVED authenticated ceremony.
-- No token, bearer, proof secret, or cryptographic verifier is represented.
CREATE TABLE approvals (invitation TEXT PRIMARY KEY, binding TEXT NOT NULL);
CREATE TABLE invitations (
 id TEXT PRIMARY KEY, binding TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('CREATED','ISSUED','CONSUMED','CANCELLED','EXPIRED')),
 verifier_id TEXT UNIQUE,
 CHECK ((state = 'CONSUMED') = (verifier_id IS NOT NULL)),
 CHECK (verifier_id IS NULL OR verifier_id = id),
 FOREIGN KEY(verifier_id) REFERENCES synthetic_verifiers(invitation) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE synthetic_verifiers (
 invitation TEXT PRIMARY KEY REFERENCES invitations(id),
 marker TEXT NOT NULL CHECK(marker = 'NON_PRODUCTION_SYNTHETIC_V1')
);
CREATE TRIGGER invitation_insert BEFORE INSERT ON invitations
WHEN NEW.state != 'CREATED' BEGIN SELECT RAISE(ABORT, 'invalid initial state'); END;
CREATE TRIGGER invitation_update BEFORE UPDATE ON invitations
WHEN NEW.id != OLD.id OR NEW.binding != OLD.binding OR NOT (
 (OLD.state = 'CREATED' AND NEW.state IN ('ISSUED','CANCELLED','EXPIRED')) OR
 (OLD.state = 'ISSUED' AND NEW.state IN ('CONSUMED','CANCELLED','EXPIRED'))
) BEGIN SELECT RAISE(ABORT, 'invalid transition'); END;
CREATE TRIGGER invitation_delete BEFORE DELETE ON invitations
BEGIN SELECT RAISE(ABORT, 'immutable record'); END;
CREATE TRIGGER verifier_insert BEFORE INSERT ON synthetic_verifiers
WHEN NOT EXISTS (SELECT 1 FROM invitations WHERE id = NEW.invitation AND state = 'CONSUMED')
BEGIN SELECT RAISE(ABORT, 'not consumed'); END;
CREATE TRIGGER verifier_update BEFORE UPDATE ON synthetic_verifiers
BEGIN SELECT RAISE(ABORT, 'immutable verifier'); END;
CREATE TRIGGER verifier_delete BEFORE DELETE ON synthetic_verifiers
BEGIN SELECT RAISE(ABORT, 'immutable verifier'); END;

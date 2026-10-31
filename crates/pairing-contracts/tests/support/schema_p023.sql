-- P022/P023 qualification tables only. Never a production migration.
CREATE TABLE ceremonies (
    id TEXT PRIMARY KEY,
    invitation TEXT NOT NULL UNIQUE REFERENCES invitations(id),
    challenge TEXT NOT NULL UNIQUE,
    snapshot TEXT NOT NULL
);
CREATE TABLE independent_identity (
    installation TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    tls INTEGER NOT NULL CHECK (tls IN (0,1))
);
CREATE TABLE device_authority (
    device TEXT PRIMARY KEY,
    active INTEGER NOT NULL CHECK (active IN (0,1))
);
CREATE TABLE ceremony_events (
    ceremony_id TEXT NOT NULL REFERENCES ceremonies(id),
    kind TEXT NOT NULL CHECK (kind IN ('owner','client')),
    evidence_id TEXT NOT NULL UNIQUE,
    principal TEXT NOT NULL,
    binding TEXT NOT NULL,
    issued INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    revoked INTEGER NOT NULL CHECK (revoked IN (0,1)),
    PRIMARY KEY (ceremony_id, kind)
);

-- This row contains only a fixed marker, never a credential or verifier.
CREATE TABLE p023_credential_reference (
    contract_version INTEGER NOT NULL CHECK (contract_version = 1),
    invitation TEXT PRIMARY KEY REFERENCES synthetic_verifiers(invitation),
    installation TEXT NOT NULL,
    owner TEXT NOT NULL,
    device TEXT NOT NULL,
    policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
    state TEXT NOT NULL CHECK (state IN (
        'ACTIVE','REVOKED','EXPIRED','SECURE_STORAGE_LOST',
        'SECURE_STORAGE_INACCESSIBLE','AMBIGUOUS_ISSUANCE'
    )),
    issued_at INTEGER NOT NULL,
    changed_at INTEGER NOT NULL,
    marker TEXT NOT NULL CHECK (marker = 'NON_PRODUCTION_REFERENCE_ONLY')
);
CREATE TRIGGER p023_credential_binding_immutable
BEFORE UPDATE ON p023_credential_reference
WHEN NEW.invitation != OLD.invitation
  OR NEW.contract_version != OLD.contract_version
  OR NEW.installation != OLD.installation
  OR NEW.owner != OLD.owner
  OR NEW.device != OLD.device
  OR NEW.policy_revision != OLD.policy_revision
  OR NEW.issued_at != OLD.issued_at
  OR NEW.marker != OLD.marker
  OR NEW.changed_at < OLD.changed_at
  OR NOT (
      (OLD.state = 'ACTIVE' AND NEW.state IN (
          'REVOKED','EXPIRED','SECURE_STORAGE_LOST',
          'SECURE_STORAGE_INACCESSIBLE','AMBIGUOUS_ISSUANCE'
      )) OR
      (OLD.state IN ('EXPIRED','SECURE_STORAGE_LOST',
          'SECURE_STORAGE_INACCESSIBLE','AMBIGUOUS_ISSUANCE')
          AND NEW.state = 'REVOKED')
  )
BEGIN SELECT RAISE(ABORT, 'invalid reference credential transition'); END;
CREATE TRIGGER p023_credential_delete_immutable
BEFORE DELETE ON p023_credential_reference
BEGIN SELECT RAISE(ABORT, 'immutable reference credential'); END;

CREATE TABLE p023_recovery_records (
    contract_version INTEGER NOT NULL CHECK (contract_version = 1),
    recovery_id TEXT PRIMARY KEY,
    invitation TEXT NOT NULL UNIQUE REFERENCES invitations(id),
    binding TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'PAIRING_PENDING','PAIRING_CANCELLATION_PENDING','PAIRING_CANCELLED',
        'INVITATION_REVOKED','OWNER_REVOKED','DEVICE_REVOKED','ENROLLED',
        'CREDENTIAL_EXPIRED','SECURE_STORAGE_LOST',
        'SECURE_STORAGE_INACCESSIBLE','AMBIGUOUS_CREDENTIAL_ISSUANCE',
        'EXPLICIT_REENROLLMENT_REQUIRED','REENROLLMENT_STARTED'
    )),
    recorded_at INTEGER NOT NULL CHECK (recorded_at >= 0)
);
CREATE TRIGGER p023_recovery_binding_immutable
BEFORE UPDATE ON p023_recovery_records
WHEN NEW.recovery_id != OLD.recovery_id
  OR NEW.contract_version != OLD.contract_version
  OR NEW.invitation != OLD.invitation
  OR NEW.binding != OLD.binding
  OR NEW.recorded_at < OLD.recorded_at
  OR NOT (
      (OLD.state = 'PAIRING_PENDING' AND NEW.state IN (
          'PAIRING_CANCELLATION_PENDING','INVITATION_REVOKED',
          'OWNER_REVOKED','DEVICE_REVOKED','ENROLLED'
      )) OR
      (OLD.state = 'PAIRING_CANCELLATION_PENDING' AND NEW.state = 'PAIRING_CANCELLED') OR
      (OLD.state = 'ENROLLED' AND NEW.state IN (
          'OWNER_REVOKED','DEVICE_REVOKED','CREDENTIAL_EXPIRED',
          'SECURE_STORAGE_LOST','SECURE_STORAGE_INACCESSIBLE',
          'AMBIGUOUS_CREDENTIAL_ISSUANCE'
      ))
  )
BEGIN SELECT RAISE(ABORT, 'invalid recovery state transition'); END;
CREATE TRIGGER p023_recovery_delete_immutable
BEFORE DELETE ON p023_recovery_records
BEGIN SELECT RAISE(ABORT, 'immutable recovery record'); END;

-- One explicit fresh-pairing reservation per failed credential/invitation.
CREATE TABLE p023_reenrollment_attempts (
    source_invitation TEXT PRIMARY KEY REFERENCES invitations(id),
    request_id TEXT NOT NULL UNIQUE,
    new_invitation TEXT NOT NULL UNIQUE REFERENCES invitations(id),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    CHECK (source_invitation != new_invitation)
);
CREATE TRIGGER p023_reenrollment_update_immutable
BEFORE UPDATE ON p023_reenrollment_attempts
BEGIN SELECT RAISE(ABORT, 'immutable reenrollment attempt'); END;
CREATE TRIGGER p023_reenrollment_delete_immutable
BEFORE DELETE ON p023_reenrollment_attempts
BEGIN SELECT RAISE(ABORT, 'immutable reenrollment attempt'); END;

-- Trusted test harness evidence for a separately authenticated explicit owner
-- action. The table has no public request ingestion or production consumer.
CREATE TABLE p023_owner_reenrollment_actions (
    evidence_id TEXT PRIMARY KEY,
    recovery_id TEXT NOT NULL REFERENCES p023_recovery_records(recovery_id),
    owner TEXT NOT NULL,
    source_invitation TEXT NOT NULL REFERENCES invitations(id),
    new_invitation TEXT NOT NULL,
    device TEXT NOT NULL,
    approved_at INTEGER NOT NULL CHECK (approved_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > approved_at),
    revoked INTEGER NOT NULL CHECK (revoked IN (0,1))
);
CREATE TRIGGER p023_owner_action_update_immutable
BEFORE UPDATE ON p023_owner_reenrollment_actions
BEGIN SELECT RAISE(ABORT, 'immutable owner action evidence'); END;
CREATE TRIGGER p023_owner_action_delete_immutable
BEFORE DELETE ON p023_owner_reenrollment_actions
BEGIN SELECT RAISE(ABORT, 'immutable owner action evidence'); END;

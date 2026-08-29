-- Device machine authentication reuses the canonical devices lifecycle.
-- Only domain-separated SHA-256 verifiers are durable, never bearer or grant
-- presentation strings. A consumed grant never reissues its one-time secret.

CREATE TABLE device_credentials (
    credential_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    device_id UUID NOT NULL,
    secret_digest BYTEA NOT NULL,
    format_version SMALLINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ(6) NOT NULL,
    revoked_at TIMESTAMPTZ(6),
    CONSTRAINT device_credentials_device_owner_fk
        FOREIGN KEY (device_id, owner_user_id)
        REFERENCES devices (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT device_credentials_digest_shape CHECK (octet_length(secret_digest) = 32),
    CONSTRAINT device_credentials_digest_unique UNIQUE (secret_digest),
    CONSTRAINT device_credentials_format_version CHECK (format_version = 1),
    CONSTRAINT device_credentials_revocation_time
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),
    CONSTRAINT device_credentials_scope_unique
        UNIQUE (credential_id, device_id, owner_user_id)
);

CREATE INDEX device_credentials_owner_device_idx
    ON device_credentials (owner_user_id, device_id);

CREATE TABLE device_enrollment_grants (
    grant_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    device_id UUID NOT NULL,
    secret_digest BYTEA NOT NULL,
    format_version SMALLINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ(6) NOT NULL,
    expires_at TIMESTAMPTZ(6) NOT NULL,
    consumed_at TIMESTAMPTZ(6),
    revoked_at TIMESTAMPTZ(6),
    consumed_credential_id UUID,
    CONSTRAINT device_enrollment_grants_device_owner_fk
        FOREIGN KEY (device_id, owner_user_id)
        REFERENCES devices (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT device_enrollment_grants_consumed_credential_fk
        FOREIGN KEY (consumed_credential_id, device_id, owner_user_id)
        REFERENCES device_credentials (credential_id, device_id, owner_user_id)
        ON DELETE RESTRICT,
    CONSTRAINT device_enrollment_grants_digest_shape CHECK (octet_length(secret_digest) = 32),
    CONSTRAINT device_enrollment_grants_digest_unique UNIQUE (secret_digest),
    CONSTRAINT device_enrollment_grants_format_version CHECK (format_version = 1),
    CONSTRAINT device_enrollment_grants_short_ttl
        CHECK (expires_at > created_at AND expires_at <= created_at + INTERVAL '15 minutes'),
    CONSTRAINT device_enrollment_grants_consumed_time
        CHECK (consumed_at IS NULL OR (consumed_at >= created_at AND consumed_at < expires_at)),
    CONSTRAINT device_enrollment_grants_revocation_time
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),
    CONSTRAINT device_enrollment_grants_consumed_shape
        CHECK ((consumed_at IS NULL) = (consumed_credential_id IS NULL))
);

CREATE INDEX device_enrollment_grants_expiry_idx
    ON device_enrollment_grants (expires_at);

CREATE INDEX device_enrollment_grants_owner_device_idx
    ON device_enrollment_grants (owner_user_id, device_id);

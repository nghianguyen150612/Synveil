-- Synveil browser-session persistence.
--
-- The bearer token is generated and presented by synveil-auth. PostgreSQL
-- stores only its SHA-256 verifier; the raw token is never written here.

CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    token_digest BYTEA NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    expires_at TIMESTAMPTZ(6) NOT NULL,
    revoked_at TIMESTAMPTZ(6),
    CONSTRAINT sessions_user_fk
        FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT sessions_token_digest_length
        CHECK (octet_length(token_digest) = 32),
    CONSTRAINT sessions_expiry_after_creation
        CHECK (expires_at > created_at),
    CONSTRAINT sessions_revoked_after_creation
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),
    CONSTRAINT sessions_token_digest_unique UNIQUE (token_digest)
);

CREATE INDEX sessions_expiry_cleanup_idx
    ON sessions (expires_at);

CREATE INDEX sessions_revoked_cleanup_idx
    ON sessions (revoked_at)
    WHERE revoked_at IS NOT NULL;

-- Synveil authentication foundation: password credentials and one-time setup.
--
-- Browser sessions, device/API credentials, recovery, and login transport are
-- later migrations. This migration stores no plaintext or reusable token.

ALTER TABLE users
    ADD COLUMN is_instance_admin BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE user_credentials (
    user_id UUID PRIMARY KEY,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT user_credentials_user_fk
        FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT user_credentials_hash_length
        CHECK (octet_length(password_hash) BETWEEN 1 AND 4096)
);

CREATE TABLE bootstrap_state (
    id SMALLINT PRIMARY KEY,
    state TEXT NOT NULL,
    closed_at TIMESTAMPTZ(6),
    CONSTRAINT bootstrap_state_singleton CHECK (id = 1),
    CONSTRAINT bootstrap_state_value CHECK (state IN ('OPEN', 'CLOSED')),
    CONSTRAINT bootstrap_state_closed_shape CHECK (
        (state = 'OPEN' AND closed_at IS NULL)
        OR (state = 'CLOSED' AND closed_at IS NOT NULL)
    )
);

INSERT INTO bootstrap_state (id, state, closed_at)
VALUES (1, 'OPEN', NULL);

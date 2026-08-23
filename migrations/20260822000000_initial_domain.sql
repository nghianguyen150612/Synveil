-- Synveil initial canonical domain schema.
--
-- This migration stores only the validated core domain. Authentication
-- credentials, sessions, journals, outbox/jobs, replicas, and sharing belong
-- to later reviewed migrations.

CREATE TABLE users (
    id UUID PRIMARY KEY,
    login_value TEXT NOT NULL,
    login_key TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    revision NUMERIC NOT NULL,
    CONSTRAINT users_login_value_length CHECK (octet_length(login_value) BETWEEN 1 AND 1024),
    CONSTRAINT users_login_key_length CHECK (octet_length(login_key) BETWEEN 1 AND 1024),
    CONSTRAINT users_status_value CHECK (status IN ('PENDING', 'ACTIVE', 'LOCKED', 'DISABLED')),
    CONSTRAINT users_revision_u64 CHECK (
        revision >= 0
        AND revision = trunc(revision)
        AND revision <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT users_login_key_unique UNIQUE (login_key)
);

CREATE TABLE devices (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    display_name TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    revision NUMERIC NOT NULL,
    CONSTRAINT devices_owner_user_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT devices_display_name_length
        CHECK (octet_length(display_name) BETWEEN 1 AND 1024),
    CONSTRAINT devices_status_value
        CHECK (status IN ('PENDING', 'ACTIVE', 'PAUSED', 'REVOKED')),
    CONSTRAINT devices_revision_u64 CHECK (
        revision >= 0
        AND revision = trunc(revision)
        AND revision <= 18446744073709551615::NUMERIC
    )
);

CREATE TABLE libraries (
    id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    name TEXT NOT NULL,
    root_node_id UUID NOT NULL,
    dedup_domain_id UUID NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    revision NUMERIC NOT NULL,
    CONSTRAINT libraries_owner_user_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT libraries_name_length CHECK (octet_length(name) BETWEEN 1 AND 1024),
    CONSTRAINT libraries_status_value
        CHECK (status IN ('ACTIVE', 'READ_ONLY', 'QUARANTINED', 'DELETING')),
    CONSTRAINT libraries_revision_u64 CHECK (
        revision >= 0
        AND revision = trunc(revision)
        AND revision <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT libraries_id_dedup_domain_unique UNIQUE (id, dedup_domain_id)
);

CREATE TABLE objects (
    id UUID PRIMARY KEY,
    dedup_domain_id UUID NOT NULL,
    canonical_hash BYTEA NOT NULL,
    plaintext_length NUMERIC NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    CONSTRAINT objects_canonical_hash_length CHECK (octet_length(canonical_hash) = 32),
    CONSTRAINT objects_plaintext_length_u64 CHECK (
        plaintext_length >= 0
        AND plaintext_length = trunc(plaintext_length)
        AND plaintext_length <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT objects_id_dedup_domain_unique UNIQUE (id, dedup_domain_id)
);

CREATE TABLE nodes (
    id UUID PRIMARY KEY,
    library_id UUID NOT NULL,
    parent_node_id UUID,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    current_version_id UUID,
    state TEXT NOT NULL,
    created_at TIMESTAMPTZ(6) NOT NULL,
    updated_at TIMESTAMPTZ(6) NOT NULL,
    revision NUMERIC NOT NULL,
    CONSTRAINT nodes_library_fk
        FOREIGN KEY (library_id) REFERENCES libraries (id) ON DELETE RESTRICT,
    CONSTRAINT nodes_parent_fk
        FOREIGN KEY (parent_node_id, library_id)
        REFERENCES nodes (id, library_id) ON DELETE RESTRICT,
    CONSTRAINT nodes_kind_value CHECK (kind IN ('FILE', 'DIRECTORY')),
    CONSTRAINT nodes_state_value CHECK (state IN ('ACTIVE', 'TRASHED', 'PURGING')),
    CONSTRAINT nodes_name_length CHECK (octet_length(name) BETWEEN 1 AND 1024),
    CONSTRAINT nodes_parent_shape CHECK (
        (parent_node_id IS NULL AND kind = 'DIRECTORY' AND state = 'ACTIVE')
        OR parent_node_id IS NOT NULL
    ),
    CONSTRAINT nodes_not_self_parent CHECK (parent_node_id IS NULL OR parent_node_id <> id),
    CONSTRAINT nodes_directory_has_no_version CHECK (
        kind = 'FILE' OR current_version_id IS NULL
    ),
    CONSTRAINT nodes_revision_u64 CHECK (
        revision >= 0
        AND revision = trunc(revision)
        AND revision <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT nodes_id_library_unique UNIQUE (id, library_id)
);

CREATE UNIQUE INDEX nodes_one_root_per_library
    ON nodes (library_id)
    WHERE parent_node_id IS NULL;

CREATE TABLE file_versions (
    id UUID PRIMARY KEY,
    library_id UUID NOT NULL,
    node_id UUID NOT NULL,
    object_id UUID NOT NULL,
    object_dedup_domain_id UUID NOT NULL,
    parent_version_id UUID,
    committed_at TIMESTAMPTZ(6) NOT NULL,
    revision NUMERIC NOT NULL,
    CONSTRAINT file_versions_node_fk
        FOREIGN KEY (node_id, library_id)
        REFERENCES nodes (id, library_id) ON DELETE RESTRICT,
    CONSTRAINT file_versions_object_fk
        FOREIGN KEY (object_id, object_dedup_domain_id)
        REFERENCES objects (id, dedup_domain_id) ON DELETE RESTRICT,
    CONSTRAINT file_versions_library_dedup_domain_fk
        FOREIGN KEY (library_id, object_dedup_domain_id)
        REFERENCES libraries (id, dedup_domain_id) ON DELETE RESTRICT,
    CONSTRAINT file_versions_parent_fk
        FOREIGN KEY (parent_version_id, library_id)
        REFERENCES file_versions (id, library_id) ON DELETE RESTRICT,
    CONSTRAINT file_versions_not_self_parent
        CHECK (parent_version_id IS NULL OR parent_version_id <> id),
    CONSTRAINT file_versions_revision_u64 CHECK (
        revision >= 0
        AND revision = trunc(revision)
        AND revision <= 18446744073709551615::NUMERIC
    ),
    CONSTRAINT file_versions_id_library_unique UNIQUE (id, library_id)
);

ALTER TABLE libraries
    ADD CONSTRAINT libraries_root_node_fk
    FOREIGN KEY (root_node_id, id)
    REFERENCES nodes (id, library_id)
    ON DELETE NO ACTION
    DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE nodes
    ADD CONSTRAINT nodes_current_version_fk
    FOREIGN KEY (current_version_id, library_id)
    REFERENCES file_versions (id, library_id)
    ON DELETE RESTRICT;

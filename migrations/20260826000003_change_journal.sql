-- Durable, owner/library-scoped logical change journal foundation.
--
-- `libraries.sync_head` is deliberately a transactional row counter rather
-- than a PostgreSQL sequence. A mutation locks the library row late, advances
-- the counter, inserts its journal row(s), and commits them together. A
-- rollback therefore cannot leave a cursor-visible sequence gap.

ALTER TABLE libraries
    ADD COLUMN journal_epoch BIGINT NOT NULL DEFAULT 1,
    ADD COLUMN sync_head BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN minimum_retained_sequence BIGINT NOT NULL DEFAULT 0,
    ADD CONSTRAINT libraries_journal_epoch_positive
        CHECK (journal_epoch > 0),
    ADD CONSTRAINT libraries_sync_head_nonnegative
        CHECK (sync_head >= 0),
    ADD CONSTRAINT libraries_minimum_retained_sequence_nonnegative
        CHECK (minimum_retained_sequence >= 0),
    ADD CONSTRAINT libraries_minimum_retained_sequence_within_head
        CHECK (minimum_retained_sequence <= sync_head),
    ADD CONSTRAINT libraries_owner_pair_unique
        UNIQUE (id, owner_user_id);

CREATE TABLE change_journal (
    entry_id UUID PRIMARY KEY,
    owner_user_id UUID NOT NULL,
    library_id UUID NOT NULL,
    journal_epoch BIGINT NOT NULL,
    sequence BIGINT NOT NULL,
    schema_version SMALLINT NOT NULL,
    resource_kind TEXT NOT NULL,
    resource_id UUID NOT NULL,
    change_kind TEXT NOT NULL,
    occurred_at TIMESTAMPTZ(6) NOT NULL,
    resource_revision NUMERIC NOT NULL,
    parent_node_id UUID,
    node_kind TEXT,
    node_state TEXT,
    current_version_id UUID,
    CONSTRAINT change_journal_owner_fk
        FOREIGN KEY (owner_user_id) REFERENCES users (id) ON DELETE RESTRICT,
    CONSTRAINT change_journal_library_owner_fk
        FOREIGN KEY (library_id, owner_user_id)
        REFERENCES libraries (id, owner_user_id) ON DELETE RESTRICT,
    CONSTRAINT change_journal_epoch_positive
        CHECK (journal_epoch > 0),
    CONSTRAINT change_journal_sequence_positive
        CHECK (sequence > 0),
    CONSTRAINT change_journal_schema_version_value
        CHECK (schema_version = 1),
    CONSTRAINT change_journal_resource_kind_value
        CHECK (resource_kind IN ('NODE')),
    CONSTRAINT change_journal_change_kind_value
        CHECK (
            change_kind IN (
                'NODE_CREATED',
                'NODE_RENAMED',
                'NODE_MOVED',
                'NODE_TRASHED',
                'NODE_RESTORED',
                'FILE_CONTENT_COMMITTED',
                'FILE_VERSION_RESTORED',
                'NODE_PURGED'
            )
        ),
    CONSTRAINT change_journal_node_kind_value
        CHECK (node_kind IS NULL OR node_kind IN ('FILE', 'DIRECTORY')),
    CONSTRAINT change_journal_node_state_value
        CHECK (node_state IS NULL OR node_state IN ('ACTIVE', 'TRASHED', 'PURGING')),
    CONSTRAINT change_journal_revision_u64
        CHECK (
            resource_revision >= 0
            AND resource_revision = trunc(resource_revision)
            AND resource_revision <= 18446744073709551615::NUMERIC
        ),
    CONSTRAINT change_journal_scope_sequence_unique
        UNIQUE (library_id, journal_epoch, sequence)
);

-- The read service filters by both authenticated owner and library before the
-- keyset range. Keep that scope explicit even though the unique key above is
-- sufficient for ordering.
CREATE INDEX change_journal_owner_library_sequence_idx
    ON change_journal (owner_user_id, library_id, journal_epoch, sequence);

CREATE INDEX change_journal_resource_sequence_idx
    ON change_journal (library_id, resource_kind, resource_id, journal_epoch, sequence);

-- Enforce append-only history at the database boundary as well as in the
-- application API. Retention/compaction is intentionally absent in this
-- phase; a future reviewed migration must replace this policy explicitly.
CREATE FUNCTION synveil_reject_change_journal_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'change_journal is append-only';
END;
$$;

CREATE TRIGGER change_journal_append_only
    BEFORE UPDATE OR DELETE ON change_journal
    FOR EACH ROW
    EXECUTE FUNCTION synveil_reject_change_journal_mutation();

-- Purge tombstones have no FK to resource_id/current_version_id so the
-- logical evidence survives deletion of the Node and its FileVersions.

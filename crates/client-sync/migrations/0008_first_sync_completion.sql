-- P041: legacy replicas are existing configured libraries; only replicas
-- admitted after this migration begin with an unproven first synchronization.
ALTER TABLE replicas ADD COLUMN first_sync_completed INTEGER NOT NULL DEFAULT 1
    CHECK(first_sync_completed IN (0,1));

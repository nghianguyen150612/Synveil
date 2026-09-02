-- Composite keyset indexes for the heterogeneous backup-operation feed.
--
-- Restore and prune plans resolve their backup-set scope through the source
-- snapshot. Their existing owner/created indexes leave backup_set_id as a
-- residual filter, which can scan unrelated owner history before producing a
-- bounded page. Keep the activity branches bounded by owner, set, and the
-- deterministic chronological/id tuple.

CREATE INDEX backup_restore_plans_owner_set_created_idx
    ON backup_restore_plans (owner_user_id, backup_set_id, created_at DESC, id DESC);

CREATE INDEX backup_prune_plans_owner_set_created_idx
    ON backup_prune_plans (owner_user_id, backup_set_id, created_at DESC, id DESC);

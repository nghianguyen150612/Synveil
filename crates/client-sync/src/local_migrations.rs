//! Fail-closed validation around the frozen SQLite migrations. Never repair,
//! remove, or infer missing durable state from an incomplete migration ledger.
use sqlx::{Row, SqliteConnection, SqlitePool, migrate::Migrator};

use crate::{ClientSyncError, LOCAL_SCHEMA_VERSION};

pub(crate) async fn run(pool: &SqlitePool, migrator: &Migrator) -> Result<(), ClientSyncError> {
    // Keep the migration connection owned until failure teardown completes.
    // Returning it through the pool spawns an asynchronous release task; that
    // task can race a single-connection pool's close and retain Windows handles.
    let mut connection = pool.acquire().await?;
    let result = run_inner(&mut connection, migrator).await;
    if result.is_err() {
        // Await the SQLite worker acknowledgement before releasing the pool.
        connection.close().await?;
        pool.close().await;
    }
    result
}

async fn run_inner(
    connection: &mut SqliteConnection,
    migrator: &Migrator,
) -> Result<(), ClientSyncError> {
    let ledger: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE name = '_sqlx_migrations' AND type = 'table'",
    )
    .fetch_one(&mut *connection)
    .await?;
    let version = if ledger == 0 {
        let objects: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'")
                .fetch_one(&mut *connection)
                .await?;
        if objects != 0 {
            return Err(ClientSyncError::LocalSchemaInvalid);
        }
        0
    } else {
        // Fetch at most the supported prefix plus one unknown entry. A corrupt
        // ledger cannot allocate an unbounded startup vector.
        let rows =
            sqlx::query("SELECT version, success FROM _sqlx_migrations ORDER BY version LIMIT 8")
                .fetch_all(&mut *connection)
                .await
                .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
        let mut version = 0;
        for row in rows {
            let recorded: i64 = row
                .try_get("version")
                .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
            let success: i64 = row
                .try_get("success")
                .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
            if recorded > LOCAL_SCHEMA_VERSION {
                return Err(ClientSyncError::LocalSchemaUnsupported);
            }
            if recorded != version + 1 || success != 1 {
                return Err(ClientSyncError::LocalSchemaInvalid);
            }
            version = recorded;
        }
        version
    };
    validate_objects(connection, version).await?;
    migrator
        .run(&mut *connection)
        .await
        .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
    validate_objects(connection, LOCAL_SCHEMA_VERSION).await
}

async fn validate_objects(
    connection: &mut SqliteConnection,
    version: i64,
) -> Result<(), ClientSyncError> {
    // Unreviewed tables or triggers can also participate in a table rebuild
    // through foreign-key cascades. Preserve the whole unknown schema rather
    // than executing migrations against it.
    let objects = sqlx::query(
        "SELECT type, name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name LIMIT ?",
    )
    .bind(
        i64::try_from(REQUIRED_OBJECTS.len() + 3)
            .map_err(|_| ClientSyncError::LocalSchemaInvalid)?,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
    for object in objects {
        let kind: String = object
            .try_get("type")
            .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
        let name: String = object
            .try_get("name")
            .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
        let profile_trigger = version >= 2
            && kind == "trigger"
            && name
                == if version >= 7 {
                    "server_profile_id_is_immutable"
                } else {
                    "server_profile_identity_is_immutable"
                };
        let known = (kind == "table" && name == "_sqlx_migrations")
            || profile_trigger
            || REQUIRED_OBJECTS
                .iter()
                .any(|&(introduced, expected_kind, expected_name)| {
                    introduced <= version && kind == expected_kind && name == expected_name
                });
        if !known {
            return Err(ClientSyncError::LocalSchemaInvalid);
        }
    }
    // Names and introduction versions come from migrations 0001-0008. These
    // metadata-only probes do not scan content or manufacture missing tables.
    for &(introduced, kind, name) in REQUIRED_OBJECTS {
        if introduced > version {
            continue;
        }
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE type = ? AND name = ?")
                .bind(kind)
                .bind(name)
                .fetch_one(&mut *connection)
                .await
                .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
        if count != 1 {
            return Err(ClientSyncError::LocalSchemaInvalid);
        }
    }
    if version >= 2 {
        let trigger = if version >= 7 {
            "server_profile_id_is_immutable"
        } else {
            "server_profile_identity_is_immutable"
        };
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'trigger' AND name = ?",
        )
        .bind(trigger)
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
        if count != 1 {
            return Err(ClientSyncError::LocalSchemaInvalid);
        }
    }
    // Migration 0004 rebuilds tables by copying reviewed columns. Unknown
    // columns must be fenced before that rebuild, or their durable data would
    // disappear despite a valid-looking historical ledger.
    for &(introduced, table, expected) in REQUIRED_COLUMN_COUNTS {
        if introduced > version
            || REQUIRED_COLUMN_COUNTS
                .iter()
                .any(|&(later, name, _)| name == table && later > introduced && later <= version)
        {
            continue;
        }
        let actual: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_xinfo(?)")
            .bind(table)
            .fetch_one(&mut *connection)
            .await
            .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
        if actual != expected {
            return Err(ClientSyncError::LocalSchemaInvalid);
        }
    }
    for &(introduced, probe) in REQUIRED_COLUMNS {
        if introduced <= version {
            sqlx::query(probe)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|_| ClientSyncError::LocalSchemaInvalid)?;
        }
    }
    Ok(())
}

const REQUIRED_OBJECTS: &[(i64, &str, &str)] = &[
    (1, "index", "local_nodes_parent_idx"),
    (1, "index", "local_operations_recovery_idx"),
    (1, "index", "one_unresolved_issue_per_fact"),
    (1, "table", "applied_events"),
    (1, "table", "bootstrap_nodes"),
    (1, "table", "bootstrap_sessions"),
    (1, "table", "local_apply_issues"),
    (1, "table", "local_nodes"),
    (1, "table", "local_operations"),
    (1, "table", "pending_acknowledgements"),
    (1, "table", "pending_events"),
    (1, "table", "pending_pages"),
    (1, "table", "replicas"),
    (1, "trigger", "pending_ack_requires_local_apply"),
    (2, "table", "profile_device_enrollments"),
    (2, "table", "profile_secret_cleanup"),
    (2, "table", "server_profiles"),
    (2, "trigger", "profile_enrollment_scope_is_immutable"),
    (2, "trigger", "replica_server_profile_is_immutable"),
    (3, "index", "active_outbound_intent_dedupe"),
    (3, "index", "observation_suppressions_path_idx"),
    (3, "index", "one_unresolved_observation_issue_per_fact"),
    (3, "index", "outbound_intents_node_idx"),
    (3, "index", "outbound_intents_pending_idx"),
    (3, "table", "observation_issues"),
    (3, "table", "observation_nodes"),
    (3, "table", "observation_scan_seen"),
    (3, "table", "observation_scan_seen_intents"),
    (3, "table", "observation_scan_work"),
    (3, "table", "observation_state"),
    (3, "table", "observation_suppressions"),
    (3, "table", "outbound_intents"),
    (4, "table", "outbound_mutation_requests"),
    (4, "table", "outbound_submission_results"),
    (4, "table", "outbound_upload_sessions"),
    (5, "table", "rebaseline_applied_handoffs"),
    (5, "table", "rebaseline_candidate_cursors"),
    (5, "table", "rebaseline_candidate_nodes"),
    (5, "table", "rebaseline_candidates"),
    (6, "index", "sync_conflicts_library_status_page_idx"),
    (6, "index", "sync_conflicts_node_idx"),
    (6, "table", "sync_conflicts"),
];

const REQUIRED_COLUMNS: &[(i64, &str)] = &[
    (
        1,
        "SELECT library_id, owner_user_id, device_id, root_binding_id, root_node_id, journal_epoch, applied_sequence, acknowledged_sequence, status, created_at_ms, updated_at_ms FROM replicas LIMIT 0",
    ),
    (
        1,
        "SELECT library_id, node_id, parent_node_id, relative_path, collision_key, logical_name, node_kind, node_state, revision, current_version_id, content_length, content_sha256, local_length, local_sha256, bootstrap_generation, present, quarantine_relative_path FROM local_nodes LIMIT 0",
    ),
    (
        1,
        "SELECT library_id, bootstrap_id, generation, snapshot_epoch, resume_sequence, manifest_item_count, state, next_cursor, completion_evidence, terminal_fetched, created_at_ms, updated_at_ms FROM bootstrap_sessions LIMIT 0",
    ),
    (
        1,
        "SELECT library_id, bootstrap_id, node_id, parent_node_id, logical_name, node_kind, node_state, revision, current_version_id, content_length, content_sha256, applied FROM bootstrap_nodes LIMIT 0",
    ),
    (
        1,
        "SELECT library_id, epoch, from_sequence, through_sequence, high_watermark, has_more, ack_evidence, state, created_at_ms, updated_at_ms FROM pending_pages LIMIT 0",
    ),
    (
        1,
        "SELECT library_id, sequence, event_id, schema_version, resource_id, change_kind, resource_revision, parent_node_id, logical_name, node_kind, node_state, current_version_id, content_length, content_sha256 FROM pending_events LIMIT 0",
    ),
    (
        1,
        "SELECT library_id, epoch, from_sequence, through_sequence, high_watermark, evidence, created_at_ms FROM pending_acknowledgements LIMIT 0",
    ),
    (
        1,
        "SELECT operation_id, library_id, node_id, server_sequence, bootstrap_generation, operation_kind, source_relative_path, destination_relative_path, staging_relative_path, expected_kind, expected_length, expected_sha256, desired_revision, desired_version_id, desired_length, desired_sha256, state, created_at_ms, updated_at_ms FROM local_operations LIMIT 0",
    ),
    (
        1,
        "SELECT library_id, epoch, sequence, event_id, resource_id, resource_revision FROM applied_events LIMIT 0",
    ),
    (
        1,
        "SELECT issue_id, library_id, node_id, server_sequence, bootstrap_generation, issue_kind, expected_state, created_at_ms, resolved_at_ms FROM local_apply_issues LIMIT 0",
    ),
    (2, "SELECT server_profile_id FROM replicas LIMIT 0"),
    (8, "SELECT first_sync_completed FROM replicas LIMIT 0"),
    (
        2,
        "SELECT profile_id, canonical_base_url, transport_policy, display_label, created_at_ms, last_connected_at_ms FROM server_profiles LIMIT 0",
    ),
    (
        2,
        "SELECT profile_id, owner_user_id, device_id, credential_id, completed_at_ms, forgotten_at_ms FROM profile_device_enrollments LIMIT 0",
    ),
    (
        2,
        "SELECT profile_id, credential_id, requested_at_ms FROM profile_secret_cleanup LIMIT 0",
    ),
    (
        3,
        "SELECT library_id, rescan_required, scan_active, scan_generation, last_reconciled_at_ms, updated_at_ms FROM observation_state LIMIT 0",
    ),
    (
        3,
        "SELECT library_id, node_id, relative_path, present, observed_kind, observed_length, observed_sha256, updated_at_ms FROM observation_nodes LIMIT 0",
    ),
    (
        3,
        "SELECT intent_id, library_id, node_id, parent_node_id, intent_kind, state, observed_relative_path, old_relative_path, observed_kind, observed_length, observed_sha256, base_epoch, base_applied_sequence, base_revision, base_current_version_id, base_parent_revision, dedupe_version, dedupe_sha256, created_at_ms, updated_at_ms FROM outbound_intents LIMIT 0",
    ),
    (
        3,
        "SELECT operation_id, library_id, node_id, expected_relative_path, expected_present, expected_kind, expected_length, expected_sha256, created_at_ms FROM observation_suppressions LIMIT 0",
    ),
    (
        3,
        "SELECT issue_id, library_id, node_id, relative_path, issue_kind, created_at_ms, resolved_at_ms FROM observation_issues LIMIT 0",
    ),
    (
        3,
        "SELECT library_id, generation, relative_path, cursor_name FROM observation_scan_work LIMIT 0",
    ),
    (
        3,
        "SELECT library_id, generation, node_id FROM observation_scan_seen LIMIT 0",
    ),
    (
        3,
        "SELECT library_id, generation, intent_id FROM observation_scan_seen_intents LIMIT 0",
    ),
    (
        4,
        "SELECT intent_id, mutation_id, mutation_kind, base_epoch, base_sequence, request_json, fingerprint_version, fingerprint_sha256, created_at_ms, updated_at_ms FROM outbound_mutation_requests LIMIT 0",
    ),
    (
        4,
        "SELECT intent_id, upload_session_id, operation, staging_relative_path, expected_length, expected_sha256, acknowledged_offset, state, created_at_ms, updated_at_ms FROM outbound_upload_sessions LIMIT 0",
    ),
    (
        4,
        "SELECT intent_id, mutation_id, upload_session_id, outcome, resource_id, result_revision, file_version_id, result_length, result_sha256, journal_event_id, journal_sequence, conflict_id, conflict_reason, replayed, created_at_ms, updated_at_ms FROM outbound_submission_results LIMIT 0",
    ),
    (
        5,
        "SELECT library_id, snapshot_id, journal_epoch, resume_sequence, expected_count, received_count, next_cursor, terminal_fetched, state, created_at_ms, updated_at_ms FROM rebaseline_candidates LIMIT 0",
    ),
    (
        5,
        "SELECT library_id, snapshot_id, node_id, parent_node_id, logical_name, node_kind, node_state, revision, current_version_id, content_length, content_sha256 FROM rebaseline_candidate_nodes LIMIT 0",
    ),
    (
        5,
        "SELECT library_id, cursor FROM rebaseline_candidate_cursors LIMIT 0",
    ),
    (
        5,
        "SELECT library_id, snapshot_id, journal_epoch, resume_sequence, applied_at_ms FROM rebaseline_applied_handoffs LIMIT 0",
    ),
    (
        6,
        "SELECT conflict_id, library_id, intent_id, node_id, kind, local_base_revision, remote_observed_revision, remote_observed_state, remote_parent_node_id, remote_epoch, remote_sequence, detected_at_ms, status, resolution_id, resolution, resolved_at_ms, replacement_intent_id FROM sync_conflicts LIMIT 0",
    ),
];

const REQUIRED_COLUMN_COUNTS: &[(i64, &str, i64)] = &[
    (1, "applied_events", 6),
    (1, "bootstrap_nodes", 12),
    (1, "bootstrap_sessions", 12),
    (1, "local_apply_issues", 9),
    (1, "local_nodes", 17),
    (1, "local_operations", 19),
    (1, "pending_acknowledgements", 7),
    (1, "pending_events", 14),
    (1, "pending_pages", 10),
    (1, "replicas", 11),
    (2, "profile_device_enrollments", 6),
    (2, "profile_secret_cleanup", 3),
    (2, "replicas", 12),
    (8, "replicas", 13),
    (2, "server_profiles", 6),
    (3, "observation_issues", 7),
    (3, "observation_nodes", 8),
    (3, "observation_scan_seen", 3),
    (3, "observation_scan_seen_intents", 3),
    (3, "observation_scan_work", 4),
    (3, "observation_state", 6),
    (3, "observation_suppressions", 9),
    (3, "outbound_intents", 20),
    (4, "outbound_mutation_requests", 10),
    (4, "outbound_submission_results", 16),
    (4, "outbound_upload_sessions", 10),
    (5, "rebaseline_applied_handoffs", 5),
    (5, "rebaseline_candidate_cursors", 2),
    (5, "rebaseline_candidate_nodes", 11),
    (5, "rebaseline_candidates", 11),
    (6, "sync_conflicts", 17),
];

#[cfg(test)]
mod tests;

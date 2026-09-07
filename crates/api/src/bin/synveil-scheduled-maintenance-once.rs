//! Internal one-shot scheduled-maintenance runtime.
//!
//! This binary is an explicitly operator-triggered, non-interactive one-shot
//! process. One invocation loads runtime configuration, connects to PostgreSQL
//! using the canonical [`DatabaseConfig`], runs pending migrations, constructs
//! one [`ScheduledMaintenanceCycleRunner`] with a fresh process-scoped worker
//! identity, obtains the observation timestamp exactly once at the process
//! boundary, executes exactly one canonical bounded cycle, logs a structured
//! summary, closes the pool, and exits.
//!
//! ```text
//! Process boundary
//!       ↓
//! load runtime config
//!       ↓
//! connect DatabasePool
//!       ↓
//! MigrationRunner
//!       ↓
//! ScheduledMaintenanceCycleRunner::new(pool)
//!       ↓
//! Timestamp::now_utc()  (once)
//!       ↓
//! run_one_scheduled_backup_maintenance_cycle(observed_at_utc, lease_seconds)
//!       ↓
//! structured log + exit
//! ```
//!
//! # Invariants
//!
//! - exactly one `run_one_scheduled_backup_maintenance_cycle` per process
//! - exactly one `Timestamp::now_utc()` read at the outer edge (no hidden clock reads inside runner/cycle)
//! - lease duration validated 10..=900 without clamping
//! - no loop, polling, sleep, interval, heartbeat, lease renewal, retry/backoff, cron, daemon, leader election, worker pool, queue
//! - no public HTTP/OpenAPI/SSE/WebSocket/frontend exposure
//! - Idle and SkippedExpired are success (exit 0); any scheduler/worker/config/database failure is non-zero
//! - durable PostgreSQL remains the source of truth; abrupt termination is recovered via Prompt 66 fencing on next invocation
//!
//! # Configuration
//!
//! - Database credential (required): delivered via systemd `LoadCredential=` as
//!   file `database-url` under `$CREDENTIALS_DIRECTORY` (production) or via
//!   non-secret env `SYNVEIL_DATABASE_CREDENTIAL_FILE=%d/database-url`. The
//!   source file is `root:root 0600` under `/etc/synveil/credentials/database-url`
//!   and exposed read-only to `synveil` via systemd. For development/manual
//!   use, `DATABASE_URL` env remains supported as a fallback when no credential
//!   file is configured. See `runtime_database_credential` for precedence.
//! - `SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS` (optional): unsigned integer 10..=900, default 120
//!
//! # Exit codes
//!
//! - `0`: cycle executed successfully, including `Idle` and `SkippedExpired`
//! - `1`: invalid configuration, database/migration failure, scheduler failure (worker not executed), worker failure (scheduler writes preserved)
//!
//! # Invocation
//!
//! ```sh
//! DATABASE_URL=postgresql://postgres@127.0.0.1:5432/synveil \
//!   cargo run -p synveil-api --bin synveil-scheduled-maintenance-once
//! ```
//!
//! Suitable for manual operator invocation or future `systemd` timer/service that
//! invokes the process again externally. This binary itself is not a daemon.

use std::{env, process};

use synveil_api::{database_config_from_runtime, init_tracing};
use synveil_core::{
    DEFAULT_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS, Timestamp,
    validate_scheduled_maintenance_lease_duration,
};
use synveil_metadata::{DatabasePool, MigrationRunner, ScheduledMaintenanceCycleRunner};

const LEASE_ENV: &str = "SYNVEIL_SCHEDULED_MAINTENANCE_LEASE_SECONDS";

fn lease_duration_from_env() -> Result<u64, String> {
    match env::var(LEASE_ENV) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(format!("{LEASE_ENV} must not be empty"));
            }
            trimmed.parse::<u64>().map_err(|_| {
                format!("{LEASE_ENV} must be an unsigned integer in 10..=900, got '{value}'")
            })
        }
        Err(env::VarError::NotPresent) => Ok(DEFAULT_BACKUP_SCHEDULED_MAINTENANCE_LEASE_SECONDS),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{LEASE_ENV} contains invalid unicode")),
    }
}

#[tokio::main]
async fn main() {
    init_tracing().ok();

    let lease_seconds = match lease_duration_from_env() {
        Ok(value) => value,
        Err(message) => {
            eprintln!("configuration error: {message}");
            process::exit(1);
        }
    };

    if let Err(error) = validate_scheduled_maintenance_lease_duration(lease_seconds) {
        eprintln!(
            "configuration error: {LEASE_ENV}={lease_seconds} is outside the valid range 10..=900: {error}"
        );
        process::exit(1);
    }

    // Credential loading happens before any DB work (migration/cycle). We
    // intentionally do not log the credential value or file contents.
    let database_config = match database_config_from_runtime() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration error: database credential invalid: {error}");
            process::exit(1);
        }
    };

    let pool = match DatabasePool::connect(&database_config).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("database error: failed to connect to PostgreSQL: {error}");
            process::exit(1);
        }
    };

    if let Err(error) = MigrationRunner::new().run(&pool).await {
        eprintln!("database error: migration failed: {error}");
        pool.close().await;
        process::exit(1);
    }

    let runner = ScheduledMaintenanceCycleRunner::new(pool.clone());

    // Single wall-clock read at the outer process boundary.
    let observed_at_utc = Timestamp::now_utc();

    // Exactly one canonical cycle per process execution.
    let result = runner
        .run_one_scheduled_backup_maintenance_cycle(observed_at_utc, lease_seconds)
        .await;

    match result {
        Ok(outcome) => {
            let tick = outcome.tick();
            let worker = outcome.worker();
            tracing::info!(
                observed_at_utc = %observed_at_utc,
                lease_seconds = lease_seconds,
                tick = ?tick,
                worker = ?worker,
                is_idle = outcome.is_idle(),
                "scheduled maintenance one-shot cycle completed"
            );
            // Also emit a concise stdout summary for operator visibility without exposing physical identities.
            println!(
                "cycle completed: tick={:?} worker={:?} idle={} observed_at_utc={} lease_seconds={}",
                tick,
                worker,
                outcome.is_idle(),
                observed_at_utc,
                lease_seconds
            );
            pool.close().await;
            process::exit(0);
        }
        Err(error) => {
            eprintln!("cycle error: {error}");
            tracing::error!(
                observed_at_utc = %observed_at_utc,
                lease_seconds = lease_seconds,
                error = %error,
                "scheduled maintenance one-shot cycle failed"
            );
            pool.close().await;
            process::exit(1);
        }
    }
}

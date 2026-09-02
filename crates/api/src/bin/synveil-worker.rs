//! Internal Synveil maintenance-worker runtime composition.
//!
//! This binary intentionally exposes no listener and no public control plane.
//! It composes the bounded GC coordinator with PostgreSQL and the configured
//! managed local ObjectStore, then lets durable lease/generation fencing—not a
//! hostname, PID, or singleton assumption—govern multi-instance safety.

use std::{env, error::Error, io, sync::Arc};

use synveil_api::init_tracing;
use synveil_core::{GcWorkerConfig, ObjectGcPolicy};
use synveil_metadata::{DatabaseConfig, DatabasePool, MigrationRunner};
use synveil_storage::{GcWorker, ObjectStore, open_local_object_store};
use tokio::{sync::watch, time::timeout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    init_tracing()?;

    let worker_config = GcWorkerConfig::from_env()?;
    if !worker_config.enabled() {
        tracing::info!("GC worker is disabled by configuration");
        return Ok(());
    }

    let database_config = DatabaseConfig::from_env()?;
    let policy = ObjectGcPolicy::from_env()?;
    let object_root = env::var("SYNVEIL_OBJECT_ROOT").map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "SYNVEIL_OBJECT_ROOT is required when the GC worker is enabled",
        )
    })?;
    let pool = DatabasePool::connect(&database_config).await?;
    MigrationRunner::new().run(&pool).await?;
    let object_store: Arc<dyn ObjectStore> = Arc::new(open_local_object_store(object_root)?);
    let worker = GcWorker::new(pool.clone(), policy, vec![object_store], worker_config)?;

    let (shutdown_sender, mut shutdown) = watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = shutdown_sender.send(true);
        }
    });
    tracing::info!(
        cycle_interval_seconds = worker.config().cycle_interval().as_secs(),
        max_candidate_claims = worker.config().max_candidate_claims_per_cycle(),
        max_active_operations = worker.config().max_active_operations(),
        max_replica_actions = worker.config().max_replica_actions_per_cycle(),
        max_concurrent_executions = worker.config().max_concurrent_executions(),
        max_concurrent_replica_deletes = worker.config().max_concurrent_replica_deletes(),
        "internal GC worker started"
    );

    loop {
        if *shutdown.borrow_and_update() {
            break;
        }
        let cycle = worker.run_once();
        tokio::pin!(cycle);
        let shutdown_during_cycle = tokio::select! {
            result = &mut cycle => {
                match result {
                    Ok(report) => tracing::info!(
                        status = ?report.status(),
                        duration_ms = report.duration().as_millis(),
                        recovery_claimed = report.recovery().recovery_claimed(),
                        candidates_claimed = report.new_work().candidates_claimed(),
                        actions_attempted = report.recovery().replica_actions_attempted()
                            + report.new_work().replica_actions_attempted(),
                        completed = report.recovery().operations_completed()
                            + report.new_work().operations_completed(),
                        retry_scheduled = report.recovery().retry_scheduled()
                            + report.new_work().retry_scheduled(),
                        needs_attention = report.reconciliation().needs_attention()
                            + report.recovery().needs_attention()
                            + report.new_work().needs_attention(),
                        "internal GC worker cycle completed"
                    ),
                    Err(error) => tracing::warn!(error = %error, "internal GC worker cycle failed safely"),
                }
                false
            }
            changed = shutdown.changed() => {
                let _ = changed;
                true
            }
        };
        if shutdown_during_cycle {
            tracing::info!("GC worker shutdown requested; draining the current bounded cycle");
            match timeout(worker.config().shutdown_timeout(), &mut cycle).await {
                Ok(Ok(report)) => tracing::info!(
                    status = ?report.status(),
                    "GC worker bounded cycle completed during shutdown"
                ),
                Ok(Err(error)) => tracing::warn!(
                    error = %error,
                    "GC worker cycle stopped with a safe durable error during shutdown"
                ),
                Err(_) => tracing::warn!(
                    "GC worker shutdown timeout elapsed; unfinished fenced work will reconcile after restart"
                ),
            }
            break;
        }
        if worker.wait_for_next_cycle(&mut shutdown).await
            == synveil_storage::GcWorkerWaitOutcome::Shutdown
        {
            break;
        }
    }

    pool.close().await;
    tracing::info!("internal GC worker stopped");
    Ok(())
}

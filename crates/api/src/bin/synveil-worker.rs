//! Internal Synveil maintenance-worker runtime composition.
//!
//! This binary intentionally exposes no listener and no public control plane.
//! It composes the bounded GC coordinator with PostgreSQL and the configured
//! managed local ObjectStore, then lets durable lease/generation fencing—not a
//! hostname, PID, or singleton assumption—govern multi-instance safety.

use std::{env, error::Error, io, sync::Arc};

use synveil_api::{
    RuntimeServerConfiguration, database_config_from_runtime, init_tracing,
    server_configuration_from_runtime,
};
use synveil_core::{GcWorkerConfig, ObjectGcPolicy};
use synveil_metadata::{DatabasePool, MigrationRunner};
use synveil_server_storage::open_existing_managed_local_storage;
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

    let server_configuration = server_configuration_from_runtime()?;
    let managed_config = match &server_configuration {
        RuntimeServerConfiguration::Managed(config) => Some(config),
        RuntimeServerConfiguration::LegacyOperator => None,
    };
    let database_config = database_config_from_runtime()?;
    let policy = ObjectGcPolicy::from_env()?;
    let pool = DatabasePool::connect(&database_config).await?;
    MigrationRunner::new().run(&pool).await?;
    let object_store: Arc<dyn ObjectStore> = match managed_config {
        Some(config) => Arc::new(open_existing_managed_local_storage(config)?),
        None => {
            let object_root = env::var("SYNVEIL_OBJECT_ROOT").map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "SYNVEIL_OBJECT_ROOT is required when the GC worker is enabled",
                )
            })?;
            Arc::new(open_local_object_store(object_root)?)
        }
    };
    let worker = GcWorker::new(pool.clone(), policy, vec![object_store], worker_config)?;
    let cycle_activity = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cycle_success = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let health_window = worker
        .config()
        .cycle_interval()
        .as_secs()
        .saturating_add(90);
    let heartbeat = managed_config.cloned().map(|config| {
        let health_pool = pool.clone();
        let cycle_activity = Arc::clone(&cycle_activity);
        let cycle_success = Arc::clone(&cycle_success);
        tokio::spawn(async move {
            loop {
                let database_ready = timeout(
                    std::time::Duration::from_secs(10),
                    health_pool.readiness(&MigrationRunner::new()),
                )
                .await
                .is_ok_and(|result| result.is_ok_and(|ready| ready.is_ready()));
                let storage_ready = open_existing_managed_local_storage(&config).is_ok();
                #[cfg(target_os = "linux")]
                if write_host_worker_health(
                    &config,
                    database_ready,
                    storage_ready,
                    cycle_activity.load(std::sync::atomic::Ordering::Acquire),
                    cycle_success.load(std::sync::atomic::Ordering::Acquire),
                    health_window,
                )
                .is_err()
                {
                    tracing::warn!("managed worker health publication unavailable");
                }
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            }
        })
    });

    let (shutdown_sender, mut shutdown) = watch::channel(false);
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        let _ = shutdown_sender.send(true);
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
                cycle_success.store(result.is_ok(), std::sync::atomic::Ordering::Release);
                cycle_activity.store(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_secs()), std::sync::atomic::Ordering::Release);
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

    if let Some(heartbeat) = heartbeat {
        heartbeat.abort();
        let _ = heartbeat.await;
    }
    pool.close().await;
    tracing::info!("internal GC worker stopped");
    Ok(())
}

#[cfg(target_os = "linux")]
fn write_host_worker_health(
    config: &synveil_server_config::ServerConfig,
    database_ready: bool,
    storage_ready: bool,
    cycle_observed_at: u64,
    cycle_successful: bool,
    health_window: u64,
) -> io::Result<()> {
    use std::{
        fs,
        io::Write,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };
    let directory = Path::new("/run/synveil-worker");
    let meta = fs::symlink_metadata(directory)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != nix::unistd::geteuid().as_raw()
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid worker health directory",
        ));
    }
    let target = directory.join("health.json");
    if let Ok(meta) = fs::symlink_metadata(&target) {
        if !meta.is_file()
            || meta.uid() != nix::unistd::geteuid().as_raw()
            || meta.nlink() != 1
            || meta.mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid worker health file",
            ));
        }
    }
    let temporary = directory.join(format!(".health-{}", uuid::Uuid::now_v7()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)?;
    let storage_id = match &config.storage {
        synveil_server_config::StorageConfiguration::ConfiguredLocal { storage_id, .. } => {
            storage_id.as_str()
        }
        _ => "",
    };
    let value = serde_json::json!({"server_id":config.server_installation_id,"storage_id":storage_id,"pid":std::process::id(),"observed_at":SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_secs(),"database_ready":database_ready,"storage_ready":storage_ready,"cycle_observed_at":cycle_observed_at,"cycle_successful":cycle_successful,"health_window":health_window});
    file.write_all(&serde_json::to_vec(&value).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&temporary, target)?;
    fs::File::open(directory)?.sync_all()
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            result = tokio::signal::ctrl_c() => { let _ = result; }
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

use std::{env, error::Error, net::SocketAddr, sync::Arc};

use synveil_api::{
    ApiState, CookieConfig, RuntimeServerConfiguration, database_config_from_runtime,
    database_credential_source_configured, init_tracing, rebaseline_key_from_runtime, router,
    server_configuration_from_runtime,
};
use synveil_auth::{PasswordHasherConfig, SessionConfig};
use synveil_core::TrashRetentionPolicy;
use synveil_metadata::{
    ContentReadMetadataBackend, DatabasePool, MigrationRunner, PostgresContentReadRepository,
    PostgresUploadRepository, UploadMetadataBackend,
};
use synveil_server_config::{NetworkConfiguration, StorageConfiguration};
use synveil_storage::{
    ContentReadApplicationService, ObjectStore, UploadApplicationService, UploadLimits,
    open_local_object_store,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    init_tracing()?;

    let server_configuration = server_configuration_from_runtime()?;
    let managed_config = match &server_configuration {
        RuntimeServerConfiguration::Managed(config) => Some(config),
        RuntimeServerConfiguration::LegacyOperator => None,
    };
    let bind_address: SocketAddr = if let Some(config) = managed_config {
        match &config.network {
            NetworkConfiguration::NotConfigured => "127.0.0.1:3000".parse()?,
            NetworkConfiguration::LocalPrivate { bind_address } => bind_address.parse()?,
        }
    } else {
        env::var("SYNVEIL_BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".to_owned())
            .parse()?
    };

    let trash_retention_policy = TrashRetentionPolicy::from_env()?;
    let mut state =
        ApiState::from_current_platform().with_trash_retention_policy(trash_retention_policy);
    if managed_config.is_none() {
        if let Ok(origin) = env::var("SYNVEIL_PUBLIC_ORIGIN") {
            state = state.with_allowed_origin(origin);
        }
    }

    let development_mode = env::var("SYNVEIL_ENV")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("development"));
    let explicitly_insecure = env::var("SYNVEIL_ALLOW_INSECURE_COOKIES")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    if development_mode && explicitly_insecure {
        state = state.with_cookie_config(CookieConfig::development());
        tracing::warn!("insecure development cookies enabled explicitly");
    }

    let database_configured = managed_config.is_some() || database_credential_source_configured();
    if database_configured {
        let rebaseline_token_key = rebaseline_key_from_runtime()?;
        let database_config = database_config_from_runtime()?;
        let pool = DatabasePool::connect(&database_config).await?;
        MigrationRunner::new().run(&pool).await?;
        let pool = Arc::new(pool);
        state = state.with_postgres_auth(
            Arc::clone(&pool),
            PasswordHasherConfig::default(),
            SessionConfig::default(),
            rebaseline_token_key,
        );
        tracing::info!("PostgreSQL authentication backend configured");

        let object_root = match managed_config.map(|config| &config.storage) {
            Some(StorageConfiguration::NotConfigured) => None,
            Some(StorageConfiguration::ConfiguredLocal { root }) => Some(root.clone()),
            None => env::var("SYNVEIL_OBJECT_ROOT").ok(),
        };
        if let Some(object_root) = object_root {
            let metadata: Arc<dyn UploadMetadataBackend> =
                Arc::new(PostgresUploadRepository::new(pool.as_ref().clone()));
            let object_store: Arc<dyn ObjectStore> =
                Arc::new(open_local_object_store(object_root)?);
            let content_metadata: Arc<dyn ContentReadMetadataBackend> =
                Arc::new(PostgresContentReadRepository::new(pool.as_ref().clone()));
            let content_service =
                ContentReadApplicationService::new(content_metadata, Arc::clone(&object_store));
            let upload_service = UploadApplicationService::new(
                metadata,
                Arc::clone(&object_store),
                UploadLimits::default(),
            )?;
            state = state.with_upload_backend(Arc::new(upload_service));
            state = state.with_download_backend(Arc::new(content_service));
            tracing::info!("PostgreSQL/local-object-store upload/download backends configured");
        } else {
            tracing::warn!(
                "SYNVEIL_OBJECT_ROOT is not configured; HTTP upload backend is unavailable"
            );
        }
    } else {
        tracing::warn!(
            "database credential is not configured; HTTP authentication backend is unavailable"
        );
    }

    // Bind only after configuration, required secrets, dependency settings,
    // migrations and the selected object-store adapter have been validated.
    let listener = TcpListener::bind(bind_address).await?;
    tracing::info!(address = %listener.local_addr()?, "Synveil API listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

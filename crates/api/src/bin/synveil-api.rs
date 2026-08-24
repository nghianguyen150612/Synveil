use std::{env, error::Error, net::SocketAddr, sync::Arc};

use synveil_api::{ApiState, CookieConfig, init_tracing, router};
use synveil_auth::{PasswordHasherConfig, SessionConfig};
use synveil_metadata::{
    DatabaseConfig, DatabasePool, MigrationRunner, PostgresUploadRepository, UploadMetadataBackend,
};
use synveil_storage::{
    ObjectStore, UploadApplicationService, UploadLimits, open_local_object_store,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    init_tracing()?;

    let bind_address =
        env::var("SYNVEIL_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let bind_address: SocketAddr = bind_address.parse()?;
    let listener = TcpListener::bind(bind_address).await?;

    let mut state = ApiState::from_current_platform();
    if let Ok(origin) = env::var("SYNVEIL_PUBLIC_ORIGIN") {
        state = state.with_allowed_origin(origin);
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

    if env::var_os("DATABASE_URL").is_some() {
        let database_config = DatabaseConfig::from_env()?;
        let pool = DatabasePool::connect(&database_config).await?;
        MigrationRunner::new().run(&pool).await?;
        let pool = Arc::new(pool);
        state = state.with_postgres_auth(
            Arc::clone(&pool),
            PasswordHasherConfig::default(),
            SessionConfig::default(),
        );
        tracing::info!("PostgreSQL authentication backend configured");

        if let Ok(object_root) = env::var("SYNVEIL_OBJECT_ROOT") {
            let metadata: Arc<dyn UploadMetadataBackend> =
                Arc::new(PostgresUploadRepository::new(pool.as_ref().clone()));
            let object_store: Arc<dyn ObjectStore> =
                Arc::new(open_local_object_store(object_root)?);
            let upload_service =
                UploadApplicationService::new(metadata, object_store, UploadLimits::default())?;
            state = state.with_upload_backend(Arc::new(upload_service));
            tracing::info!("PostgreSQL/local-object-store upload backend configured");
        } else {
            tracing::warn!(
                "SYNVEIL_OBJECT_ROOT is not configured; HTTP upload backend is unavailable"
            );
        }
    } else {
        tracing::warn!(
            "DATABASE_URL is not configured; HTTP authentication backend is unavailable"
        );
    }

    tracing::info!(address = %listener.local_addr()?, "Synveil API listening");
    axum::serve(listener, router(state)).await?;
    Ok(())
}

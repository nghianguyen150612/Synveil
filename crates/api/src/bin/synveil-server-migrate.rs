//! Canonical, non-root migration gate for the managed server topology.

use std::{error::Error, io};

use synveil_api::{
    RuntimeServerConfiguration, database_config_from_runtime, init_tracing,
    server_configuration_from_runtime,
};
use synveil_metadata::{DatabasePool, MigrationRunner};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    init_tracing()?;
    let config = server_configuration_from_runtime()?;
    let RuntimeServerConfiguration::Managed(managed) = config else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "managed server config is required",
        )
        .into());
    };
    if managed.database.postgres_major != 17 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "PostgreSQL major 17 is required",
        )
        .into());
    }
    let database = database_config_from_runtime()?;
    let pool = DatabasePool::connect(&database).await?;
    MigrationRunner::new().run(&pool).await?;
    pool.close().await;
    tracing::info!("canonical server migrations are current");
    Ok(())
}

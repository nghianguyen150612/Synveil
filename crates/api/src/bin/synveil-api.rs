use std::{env, error::Error, net::SocketAddr};

use synveil_api::{ApiState, init_tracing, router};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    init_tracing()?;

    let bind_address =
        env::var("SYNVEIL_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let bind_address: SocketAddr = bind_address.parse()?;
    let listener = TcpListener::bind(bind_address).await?;

    tracing::info!(address = %listener.local_addr()?, "Synveil API listening");
    axum::serve(listener, router(ApiState::from_current_platform())).await?;
    Ok(())
}

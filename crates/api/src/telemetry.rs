/// Install a JSON `tracing` subscriber for a server composition root.
///
/// The library does not install a global subscriber implicitly, which keeps
/// embedding applications and tests in control of process-wide telemetry.
pub fn init_tracing() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("synveil_api=info"));

    tracing_subscriber::fmt()
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_env_filter(filter)
        .try_init()?;
    Ok(())
}

//! Logger module - logging initialization
//! Ported from Go version

use tracing::Level;

/// Initialize logging
pub fn init(level: Level) -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_max_level(level)
        .init();

    Ok(())
}

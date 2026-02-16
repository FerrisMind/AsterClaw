//! Logger module - logging initialization
//! Ported from Go version

use tracing::Level;

/// Initialize logging
pub fn init(level: Level) -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
    use tracing_subscriber::EnvFilter;

    let default_directive = match level {
        Level::TRACE => "trace",
        Level::DEBUG => "debug",
        Level::INFO => "info",
        Level::WARN => "warn",
        Level::ERROR => "error",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_directive));

    tracing_subscriber::fmt().with_env_filter(filter).init();

    Ok(())
}

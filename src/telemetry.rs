//! Tracing/logging initialisation.

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialise structured logging. Reads `RUST_LOG` for the filter, defaulting
/// to `info`. Safe to call once at startup.
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,platform_inspector=debug"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .try_init();
}

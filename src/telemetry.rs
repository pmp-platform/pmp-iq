//! Tracing/logging initialisation.

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialise structured logging. `RUST_LOG` always wins; otherwise the
/// configured `level` (from `config.yaml`/`LOG_LEVEL`) is used, falling back to
/// a sensible default. Safe to call once at startup.
pub fn init(level: Option<&str>) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| match level {
        Some(level) if !level.trim().is_empty() => {
            EnvFilter::new(format!("{level},platform_inspector={level}"))
        }
        _ => EnvFilter::new("info,platform_inspector=debug"),
    });
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_accepts_a_level_and_is_idempotent() {
        // `try_init` makes repeated/concurrent calls safe; exercise both the
        // configured-level and the default branches.
        init(Some("info"));
        init(Some(""));
        init(None);
    }
}

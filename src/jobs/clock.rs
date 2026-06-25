//! Clock abstraction for deterministic time in tests.

use chrono::{DateTime, Utc};

/// Provides the current time.
#[cfg_attr(test, mockall::automock)]
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Real wall-clock implementation.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

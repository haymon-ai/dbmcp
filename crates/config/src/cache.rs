//! In-memory cache settings.

use std::time::Duration;

/// In-memory cache settings; present only when caching is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheConfig {
    /// Time a cached catalog entry lives after it is filled.
    pub ttl: Duration,
}

impl CacheConfig {
    /// Default cache time-to-live, in seconds.
    pub const DEFAULT_TTL_SECS: u64 = 3600;
}

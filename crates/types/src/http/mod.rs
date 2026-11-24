//! HTTP transport settings and the CORS origin allowlist.

mod config;
mod origin;

pub use config::Config;
pub use origin::{AllowedOrigins, is_valid_origin};

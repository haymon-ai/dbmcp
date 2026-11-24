//! SSH tunnel settings and the host-key policy.

mod config;
mod host;

pub use config::Config;
pub use host::StrictHostKeyChecking;

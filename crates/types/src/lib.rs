//! Plain dbmcp types shared across crates, without the config loader.
//!
//! The root holds the composed [`Config`] and the top-level settings
//! ([`Transport`], [`LogLevel`]). Each config section lives in its own public
//! module with plain names: [`database::Config`], [`http::Config`],
//! [`privacy::Config`], [`ssh::Config`].
//! Callers import the module and qualify: `privacy::Entity`, `ssh::Config`.

mod config;
pub mod database;
pub mod http;
mod logging;
pub mod privacy;
pub mod ssh;
mod transport;

pub use config::Config;
pub use logging::LogLevel;
pub use transport::Transport;

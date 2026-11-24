//! Plain dbmcp configuration types and privacy vocabulary.
//!
//! The root holds [`Config`], [`Transport`], and [`LogLevel`]; each config
//! section is a module ([`database`], [`http`], [`privacy`], [`ssh`]).
//!
//! # Security
//!
//! The database password, SSH password and key passphrase, and HTTP auth
//! token are stored as [`secrecy::SecretString`], so their values never
//! reach [`Debug`] output.

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

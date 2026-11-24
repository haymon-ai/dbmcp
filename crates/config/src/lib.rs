//! Configuration loading for the dbmcp project.
//!
//! [`load`] builds a validated [`dbmcp_types::Config`] from the process env,
//! optional env files, and optional YAML config files. The crate's interface is
//! [`load`], its inputs ([`LoadOptions`], [`home_dir`]), and its outcome
//! ([`Loaded`], [`LoadError`]).
//!
//! Settings resolve with this precedence: non-empty process env > the
//! last env file's value, if non-empty > the last config file's value >
//! default. All defaults (backend-aware port, user, host) are resolved by
//! the loader, so consumers access plain values directly.

mod choices;
mod dotenv;
mod env;
mod error;
mod interpolate;
mod loader;
mod node;
mod options;
mod rules;
mod schema;
mod settings;
mod source;
mod template;
mod validate;

pub use loader::{LoadError, Loaded, load};
pub use options::{LoadOptions, home_dir};

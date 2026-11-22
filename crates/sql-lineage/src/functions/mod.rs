//! Per-dialect tables of built-in functions that expose nothing beyond their arguments and the clock.
//!
//! Left out on purpose: built-ins that run SQL text, read relations, files or
//! settings, report session state, quote arguments in errors, or have side
//! effects. Calls the parser models as their own expressions (`cast`,
//! `extract`, `trim`, …) never reach the lookup; a cast's target type
//! resolves against [`crate::types`] instead.

mod mariadb;
mod mysql;
mod postgres;
mod resolve;
mod sqlite;

pub(crate) use resolve::{lookup, resolve};

//! Per-dialect tables of built-in functions that expose nothing beyond their arguments.
//!
//! Each dialect has a hand-reviewed `const` list of names, grouped by kind
//! in comments. A name it lists is recorded as a known
//! [`FunctionUse`](crate::FunctionUse); a call it lacks is still analyzed
//! (its sources are the union of its arguments' sources) but recorded as
//! unknown.
//!
//! A listed function's result depends only on its arguments and the clock,
//! and none of its error messages includes an argument value. Built-ins that
//! expose values the parser cannot see are deliberately left out: functions
//! that run SQL text or read whole relations by name, read files, large
//! objects or server settings, report session or server state, put argument
//! values in error messages, or have side effects.
//!
//! Calls the parser turns into their own expressions (`ceil`, `floor`,
//! `extract`, `substr`, `substring`, `trim`, `overlay`, `position`,
//! `interval`, `cast`, `convert`) never reach the lookup, so no table lists
//! them.

mod mariadb;
mod mysql;
mod postgres;
mod resolve;
mod sqlite;

pub(crate) use resolve::resolve;

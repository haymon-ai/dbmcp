//! Server-authored SQL of the `MySQL`/`MariaDB` backend.
//!
//! Every SQL statement this crate writes itself lives here, as a private
//! documented `const` beside the `pub(crate)` function that runs it: one
//! file per object kind (`databases`, `tables`, `views`, `triggers`,
//! `functions`, `procedures`) plus `session` (connection hooks), `policy`
//! (`privacy.rules` catalog rounds, transaction start, and the `EXPLAIN`
//! format pin), `ident` (identifier quoting), and `explain`.
//!
//! User SQL stays outside, running through [`Connection`](dbmcp_sql::Connection)
//! or [`enforce`](crate::MysqlHandler::enforce). [`explain::statement`]
//! is the one exception: it builds the `EXPLAIN` text around user SQL but
//! does not run it. Test fixtures keep their SQL inline.

pub(crate) mod databases;
pub(crate) mod explain;
pub(crate) mod functions;
pub(crate) mod ident;
pub(crate) mod policy;
pub(crate) mod procedures;
pub(crate) mod session;
pub(crate) mod tables;
pub(crate) mod triggers;
pub(crate) mod views;

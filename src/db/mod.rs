//! Database layer: backend trait, SQL validation, and identifier checking.

pub mod backend;
pub mod identifier;
pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod validation;

//! Per-dialect tables of built-in cast target types.
//!
//! A cast runs the target type's input routine on its operand. An extension
//! or user type's routine is arbitrary code, like a user function, so every
//! such type is left out. Left out on purpose too: built-in types whose input
//! quotes the operand in errors (`PostgreSQL` `tsvector`, `tsquery`,
//! `jsonpath`) and pseudo-types. `SQLite` casts never fail, so every name is
//! known there.

use sqlparser::ast::{ArrayElemTypeDef, DataType};

use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::functions;

/// `PostgreSQL` built-in type names, lowercase, as written after `pg_catalog.` or alone.
///
/// Names the parser models as variants (`int4`, `text`) are listed too: only
/// their unmodeled `pg_catalog.`-qualified spellings reach this table.
const POSTGRES: &[&str] = &[
    // Numeric.
    "int2",
    "int4",
    "int8",
    "float4",
    "float8",
    "numeric",
    "money",
    "oid",
    // Character and binary.
    "text",
    "varchar",
    "bpchar",
    "char",
    "name",
    "bytea",
    "bit",
    "varbit",
    // Boolean, date, and time.
    "bool",
    "date",
    "time",
    "timetz",
    "timestamp",
    "timestamptz",
    "interval",
    // JSON, XML, and UUID.
    "json",
    "jsonb",
    "xml",
    "uuid",
    // Network.
    "inet",
    "cidr",
    "macaddr",
    "macaddr8",
    // Geometric.
    "point",
    "line",
    "lseg",
    "box",
    "path",
    "polygon",
    "circle",
    // Ranges.
    "int4range",
    "int8range",
    "numrange",
    "daterange",
    "tsrange",
    "tstzrange",
    // Object identifiers.
    "regclass",
    "regcollation",
    "regconfig",
    "regdictionary",
    "regnamespace",
    "regoper",
    "regoperator",
    "regproc",
    "regprocedure",
    "regrole",
    "regtype",
    // System.
    "xid",
    "xid8",
    "cid",
    "tid",
    "pg_lsn",
    "pg_snapshot",
    "txid_snapshot",
    "int2vector",
    "oidvector",
];

/// `MySQL` and `MariaDB` cast target names the parser leaves unmodeled, lowercase.
const MYSQL: &[&str] = &["nchar", "year"];

/// Returns the dialect's built-in names for types the parser leaves unmodeled.
const fn names(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres => POSTGRES,
        Dialect::Mysql | Dialect::Mariadb => MYSQL,
        Dialect::Sqlite => &[],
    }
}

/// Resolves a cast target type to its recorded name and whether it is known.
///
/// Known when the dialect lists the type, unqualified or, on `PostgreSQL`,
/// `pg_catalog.`-qualified; an array is known when its element type is.
/// Matching is by name only, so a same-named user type is still reported
/// known.
///
/// # Errors
///
/// Returns [`LineageError::Unsupported`] if a name part is not an identifier.
pub(crate) fn resolve(dialect: Dialect, data_type: &DataType) -> Result<(String, bool), LineageError> {
    let (name, listed) = match data_type {
        DataType::Array(
            ArrayElemTypeDef::SquareBracket(element, _)
            | ArrayElemTypeDef::AngleBracket(element)
            | ArrayElemTypeDef::Parenthesis(element),
        ) => {
            let (name, listed) = resolve(dialect, element)?;
            (format!("{name}[]"), listed)
        }
        DataType::Custom(name, _) => functions::lookup(dialect, name, names(dialect))?,
        _ => (data_type.to_string().to_lowercase(), modeled(dialect, data_type)),
    };
    Ok((name, listed || dialect == Dialect::Sqlite))
}

/// Whether the parser's own variant names a built-in cast target of the dialect.
const fn modeled(dialect: Dialect, data_type: &DataType) -> bool {
    match dialect {
        Dialect::Postgres => matches!(
            data_type,
            DataType::Int(_)
                | DataType::Integer(_)
                | DataType::Int2(_)
                | DataType::Int4(_)
                | DataType::Int8(_)
                | DataType::SmallInt(_)
                | DataType::BigInt(_)
                | DataType::Real
                | DataType::DoublePrecision
                | DataType::Float(_)
                | DataType::Float4
                | DataType::Float8
                | DataType::Numeric(_)
                | DataType::Decimal(_)
                | DataType::Text
                | DataType::Varchar(_)
                | DataType::CharacterVarying(_)
                | DataType::Char(_)
                | DataType::Character(_)
                | DataType::Boolean
                | DataType::Bool
                | DataType::Date
                | DataType::Time(..)
                | DataType::Timestamp(..)
                | DataType::Interval { .. }
                | DataType::JSON
                | DataType::JSONB
                | DataType::Uuid
                | DataType::Bytea
                | DataType::Bit(_)
                | DataType::BitVarying(_)
                | DataType::VarBit(_)
                | DataType::Regclass
        ),
        Dialect::Mysql | Dialect::Mariadb => matches!(
            data_type,
            DataType::Char(_)
                | DataType::Binary(_)
                | DataType::Signed
                | DataType::SignedInteger
                | DataType::Unsigned
                | DataType::UnsignedInteger
                | DataType::Int(_)
                | DataType::Integer(_)
                | DataType::Date
                | DataType::Datetime(_)
                | DataType::Time(..)
                | DataType::Decimal(_)
                | DataType::Double(_)
                | DataType::Float(_)
                | DataType::Real
                | DataType::JSON
        ),
        Dialect::Sqlite => false,
    }
}

#[cfg(test)]
mod tests {
    use sqlparser::ast::{Expr, SelectItem, SetExpr, Statement};

    use super::*;

    const DIALECTS: [Dialect; 4] = [Dialect::Postgres, Dialect::Mysql, Dialect::Mariadb, Dialect::Sqlite];

    /// Resolves the target type of `SELECT CAST(x AS <ty>)` or `SELECT x::<ty>` as `dialect` parses it.
    fn cast(dialect: Dialect, sql: &str) -> (String, bool) {
        let statements = dialect.parse(sql).expect("parse");
        let Statement::Query(query) = &statements[0] else {
            panic!("not a query: {sql}");
        };
        let SetExpr::Select(select) = &*query.body else {
            panic!("not a select: {sql}");
        };
        let SelectItem::UnnamedExpr(Expr::Cast { data_type, .. }) = &select.projection[0] else {
            panic!("not a cast: {sql}");
        };
        resolve(dialect, data_type).expect("resolve")
    }

    #[test]
    fn names_are_lowercase_and_unique_per_dialect() {
        for dialect in DIALECTS {
            let mut names = names(dialect).to_vec();
            for name in &names {
                assert_eq!(*name, name.to_lowercase(), "{dialect:?}: {name}");
            }
            let total = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), total, "{dialect:?}: duplicate name");
        }
    }

    #[test]
    fn value_echoing_postgres_types_are_unknown_in_every_spelling() {
        for sql in [
            "SELECT CAST(x AS tsvector)",
            "SELECT CAST(x AS tsquery)",
            "SELECT x::tsvector",
            "SELECT x::TSQUERY",
            "SELECT x::pg_catalog.tsvector",
            "SELECT x::tsquery[]",
            "SELECT x::jsonpath",
            "SELECT x::record",
            "SELECT x::cstring",
            "SELECT x::citext",
            "SELECT x::public.mood",
        ] {
            assert!(!cast(Dialect::Postgres, sql).1, "{sql}");
        }
    }

    #[test]
    fn built_in_postgres_types_are_known() {
        for sql in [
            "SELECT x::int",
            "SELECT x::numeric(10, 2)",
            "SELECT x::text",
            "SELECT x::pg_catalog.text",
            "SELECT x::\"char\"",
            "SELECT x::timestamp with time zone",
            "SELECT x::regclass",
            "SELECT x::regtype",
            "SELECT x::inet",
            "SELECT x::int[]",
            "SELECT CAST(x AS jsonb)",
            "SELECT CAST(x AS double precision)",
        ] {
            assert!(cast(Dialect::Postgres, sql).1, "{sql}");
        }
    }

    #[test]
    fn names_fold_per_dialect() {
        assert_eq!(
            cast(Dialect::Postgres, "SELECT x::TSVector"),
            ("tsvector".into(), false)
        );
        assert_eq!(cast(Dialect::Postgres, "SELECT x::\"INET\""), ("INET".into(), false));
        assert_eq!(
            cast(Dialect::Postgres, "SELECT x::public.inet"),
            ("public.inet".into(), false)
        );
        assert_eq!(
            cast(Dialect::Postgres, "SELECT x::citext[]"),
            ("citext[]".into(), false)
        );
        assert_eq!(cast(Dialect::Mysql, "SELECT CAST(x AS YEAR)"), ("year".into(), true));
    }

    #[test]
    fn mysql_cast_targets_are_known() {
        for dialect in [Dialect::Mysql, Dialect::Mariadb] {
            for ty in [
                "CHAR",
                "CHAR(3)",
                "BINARY",
                "SIGNED",
                "UNSIGNED INTEGER",
                "DATE",
                "DATETIME(3)",
                "TIME",
                "DECIMAL(10, 2)",
                "DOUBLE",
                "FLOAT",
                "REAL",
                "JSON",
                "NCHAR",
                "YEAR",
            ] {
                assert!(cast(dialect, &format!("SELECT CAST(x AS {ty})")).1, "{dialect:?}: {ty}");
            }
            for ty in ["POINT", "GEOMETRY", "VECTOR", "TSVECTOR"] {
                assert!(
                    !cast(dialect, &format!("SELECT CAST(x AS {ty})")).1,
                    "{dialect:?}: {ty}"
                );
            }
        }
    }

    #[test]
    fn sqlite_casts_are_known() {
        for ty in ["INTEGER", "TEXT", "tsvector", "anything"] {
            assert!(cast(Dialect::Sqlite, &format!("SELECT CAST(x AS {ty})")).1, "{ty}");
        }
    }
}

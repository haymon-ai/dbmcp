//! Written relation names as each dialect reads them, before any catalog lookup.

use dbmcp_sql_lineage::{Dialect, NamePart, Origin, WrittenName};

/// The longest `PostgreSQL` identifier in bytes (`NAMEDATALEN` - 1).
const MAX_IDENTIFIER_BYTES: usize = 63;

/// A written relation name as its dialect reads it.
///
/// [`Step::Missing`](crate::Step::Missing) hands one out per name that can
/// name a relation; the backend looks it up in its own catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationRef {
    /// The schema the name is in; `None` when the server's search order decides.
    ///
    /// `PostgreSQL` searches `search_path`; `SQLite` searches `temp`, `main`,
    /// then attached schemas. `MySQL` and `MariaDB` always set it.
    pub schema: Option<String>,
    /// The relation name.
    ///
    /// `PostgreSQL` names come folded and truncated as the server does;
    /// others come as written.
    pub name: String,
}

impl RelationRef {
    /// Reads `written` like `dialect`; `None` when it can name no relation.
    ///
    /// `database` is the `MySQL` default database or the `PostgreSQL`
    /// connected database. One or two parts read as `[schema.]name` on every
    /// dialect, and `PostgreSQL` also reads `database.schema.name`.
    ///
    /// A bare `MySQL` name is in the view's schema inside a view body, else
    /// in `database`. A bare `SQLite` name inside a view stored outside
    /// `temp` is in that view's schema.
    pub(crate) fn new(dialect: Dialect, database: Option<&str>, written: &WrittenName) -> Option<Self> {
        if dialect == Dialect::Postgres {
            let mut parts: Vec<String> = written.parts.iter().map(fold).collect();
            let name = parts.pop()?;
            let schema = parts.pop();
            return match parts.as_slice() {
                [] => Some(Self { schema, name }),
                [catalog] if database == Some(catalog.as_str()) => Some(Self { schema, name }),
                _ => None,
            };
        }
        let (schema, name) = match written.parts.as_slice() {
            [name] => match (&written.context, dialect) {
                (Origin::View(view), Dialect::Sqlite) if view.schema.eq_ignore_ascii_case("temp") => (None, name),
                (Origin::View(view), _) => (Some(view.schema.clone()), name),
                (Origin::Query, Dialect::Sqlite) => (None, name),
                (Origin::Query, _) => (Some(database?.to_owned()), name),
            },
            [schema, name] => (Some(schema.value.clone()), name),
            _ => return None,
        };
        Some(Self {
            schema,
            name: name.value.clone(),
        })
    }
}

/// Folds a `PostgreSQL` name part like the server.
///
/// Unquoted parts fold to ASCII lowercase; every part is cut to
/// [`MAX_IDENTIFIER_BYTES`] on a character boundary.
fn fold(part: &NamePart) -> String {
    let mut folded = if part.quoted {
        part.value.clone()
    } else {
        part.value.to_ascii_lowercase()
    };
    folded.truncate(folded.floor_char_boundary(MAX_IDENTIFIER_BYTES));
    folded
}

#[cfg(test)]
mod tests {
    use dbmcp_sql_lineage::RelationId;

    use super::*;

    /// Reads a query name of `parts` on `dialect` connected to `app`.
    fn read(dialect: Dialect, parts: Vec<NamePart>) -> Option<RelationRef> {
        RelationRef::new(dialect, Some("app"), &WrittenName::query(parts))
    }

    /// A name of `parts` inside the body of a view stored in `schema`.
    fn in_view(schema: &str, parts: Vec<NamePart>) -> WrittenName {
        WrittenName {
            parts,
            context: Origin::View(RelationId::new("def", schema, "v")),
        }
    }

    /// A reference to `name`, in `schema` when given.
    fn reference(schema: Option<&str>, name: &str) -> RelationRef {
        RelationRef {
            schema: schema.map(Into::into),
            name: name.into(),
        }
    }

    #[test]
    fn mysql_bare_names_are_in_the_default_database_or_the_view_schema() {
        for dialect in [Dialect::Mysql, Dialect::Mariadb] {
            let bare = WrittenName::query(vec![NamePart::bare("Users")]);
            assert_eq!(
                RelationRef::new(dialect, Some("app"), &bare),
                Some(reference(Some("app"), "Users"))
            );
            assert_eq!(RelationRef::new(dialect, None, &bare), None, "no database selected");
            let body = in_view("other", vec![NamePart::quoted("users")]);
            assert_eq!(
                RelationRef::new(dialect, Some("app"), &body),
                Some(reference(Some("other"), "users"))
            );
        }
    }

    #[test]
    fn mysql_qualified_names_take_their_schema_and_three_parts_never_read() {
        let two = vec![NamePart::bare("analytics"), NamePart::bare("events")];
        assert_eq!(read(Dialect::Mysql, two), Some(reference(Some("analytics"), "events")));
        let three = vec![NamePart::bare("def"), NamePart::bare("app"), NamePart::bare("users")];
        assert_eq!(read(Dialect::Mysql, three), None);
    }

    #[test]
    fn postgres_unquoted_parts_fold_and_quoted_parts_stay() {
        let two = vec![NamePart::bare("Public"), NamePart::quoted("Users")];
        assert_eq!(read(Dialect::Postgres, two), Some(reference(Some("public"), "Users")));
        assert_eq!(
            read(Dialect::Postgres, vec![NamePart::bare("USERS")]),
            Some(reference(None, "users"))
        );
    }

    #[test]
    fn postgres_bare_names_in_view_bodies_follow_the_search_path() {
        let body = in_view("other", vec![NamePart::bare("users")]);
        assert_eq!(
            RelationRef::new(Dialect::Postgres, Some("app"), &body),
            Some(reference(None, "users"))
        );
    }

    #[test]
    fn postgres_database_qualifier_must_be_the_connected_database() {
        let three = |catalog| {
            read(
                Dialect::Postgres,
                vec![catalog, NamePart::bare("public"), NamePart::bare("users")],
            )
        };
        assert_eq!(three(NamePart::bare("APP")), Some(reference(Some("public"), "users")));
        assert_eq!(three(NamePart::quoted("APP")), None);
        assert_eq!(three(NamePart::bare("other")), None);
        let four = ["app", "a", "b", "c"].map(NamePart::bare).to_vec();
        assert_eq!(read(Dialect::Postgres, four), None);
    }

    #[test]
    fn postgres_long_identifiers_truncate_to_63_bytes_on_char_boundaries() {
        let long = read(Dialect::Postgres, vec![NamePart::bare("a".repeat(70))]);
        assert_eq!(long, Some(reference(None, &"a".repeat(63))));
        let wide = read(
            Dialect::Postgres,
            vec![NamePart::quoted(format!("{}é", "b".repeat(62)))],
        );
        assert_eq!(wide, Some(reference(None, &"b".repeat(62))), "é straddles byte 63");
    }

    #[test]
    fn sqlite_bare_names_search_except_in_views_outside_temp() {
        assert_eq!(
            read(Dialect::Sqlite, vec![NamePart::bare("T")]),
            Some(reference(None, "T"))
        );
        let read_in = |schema| RelationRef::new(Dialect::Sqlite, None, &in_view(schema, vec![NamePart::bare("t")]));
        assert_eq!(read_in("main"), Some(reference(Some("main"), "t")));
        assert_eq!(read_in("TEMP"), Some(reference(None, "t")));
    }

    #[test]
    fn sqlite_qualified_names_take_their_schema_and_three_parts_never_read() {
        let two = vec![NamePart::bare("AUX"), NamePart::quoted("users")];
        assert_eq!(read(Dialect::Sqlite, two), Some(reference(Some("AUX"), "users")));
        assert_eq!(
            read(Dialect::Sqlite, ["a", "b", "c"].map(NamePart::bare).to_vec()),
            None
        );
    }
}

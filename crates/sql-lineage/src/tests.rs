//! Unit tests for catalog-aware lineage, one per resolution rule and error.

use std::collections::BTreeSet;

use crate::{
    Catalog, CatalogColumn, Dialect, Entry, Lexical, Lineage, LineageError, NamePart, Relation, RelationId,
    RelationKind, Role, SourceRef, WrittenName, analyze,
};

/// Builds a relation in schema `public`; a leading `~` marks a star-invisible column.
fn relation(name: &str, kind: RelationKind, columns: &[&str]) -> Relation {
    let columns = columns
        .iter()
        .map(|c| match c.strip_prefix('~') {
            Some(hidden) => CatalogColumn::hidden(hidden),
            None => CatalogColumn::new(*c),
        })
        .collect();
    Relation::new(RelationId::new("db", "public", name), kind, columns)
}

/// Builds a catalog of tables keyed by their unquoted single-part name.
fn catalog(tables: &[(&str, &[&str])]) -> Catalog {
    let mut catalog = Catalog::new();
    for (name, columns) in tables {
        catalog.insert(
            WrittenName::query(vec![NamePart::bare(*name)]),
            Entry::Resolved(relation(name, RelationKind::Table, columns)),
        );
    }
    catalog
}

/// The standard fixture: `users`, `orders`, `a`, `b`.
fn fixture() -> Catalog {
    catalog(&[
        ("users", &["id", "email", "ssn", "~secret"]),
        ("orders", &["id", "user_id", "total"]),
        ("a", &["id", "x"]),
        ("b", &["id", "y"]),
    ])
}

/// Analyzes against the fixture on `PostgreSQL`.
fn pg(sql: &str) -> Result<Lineage, LineageError> {
    analyze(sql, Dialect::Postgres, &fixture())
}

/// Renders sources as sorted `relation.column` strings.
fn names(sources: &BTreeSet<SourceRef>) -> Vec<String> {
    sources
        .iter()
        .map(|s| format!("{}.{}", s.relation.name, s.column))
        .collect()
}

/// Renders each output column as `(name, sources)`.
fn columns(lineage: &Lineage) -> Vec<(String, Vec<String>)> {
    lineage
        .columns
        .iter()
        .map(|c| (c.name.clone(), names(&c.sources)))
        .collect()
}

/// Renders references as sorted `(relation.column, role)` pairs.
fn refs(lineage: &Lineage) -> Vec<(String, Role)> {
    lineage
        .references
        .iter()
        .map(|r| (format!("{}.{}", r.source.relation.name, r.source.column), r.role))
        .collect()
}

/// Shorthand for an expected output column.
fn col(name: &str, sources: &[&str]) -> (String, Vec<String>) {
    (name.to_string(), sources.iter().map(ToString::to_string).collect())
}

/// Shorthand for an expected reference.
fn r(source: &str, role: Role) -> (String, Role) {
    (source.to_string(), role)
}

/// Asserts that analyzing `sql` fails with `Unsupported`.
fn assert_unsupported(sql: &str, dialect: Dialect, catalog: &Catalog) {
    let result = analyze(sql, dialect, catalog);
    assert!(
        matches!(result, Err(LineageError::Unsupported(_))),
        "{sql}: expected Unsupported, got {result:?}"
    );
}

#[test]
fn star_expands_in_ordinal_order_and_skips_invisible() {
    let lineage = pg("SELECT * FROM users").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![
            col("id", &["users.id"]),
            col("email", &["users.email"]),
            col("ssn", &["users.ssn"])
        ]
    );
}

#[test]
fn invisible_column_binds_when_named() {
    let lineage = pg("SELECT secret FROM users").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("secret", &["users.secret"])]);
}

#[test]
fn qualified_star_expands_one_relation() {
    let lineage = pg("SELECT o.* FROM users u JOIN orders o ON o.user_id = u.id").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![
            col("id", &["orders.id"]),
            col("user_id", &["orders.user_id"]),
            col("total", &["orders.total"])
        ]
    );
    assert_eq!(
        refs(&lineage),
        vec![r("orders.user_id", Role::Join), r("users.id", Role::Join)]
    );
}

#[test]
fn using_merges_keys_first_with_both_sources() {
    let lineage = pg("SELECT * FROM a JOIN b USING (id)").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("id", &["a.id", "b.id"]), col("x", &["a.x"]), col("y", &["b.y"])]
    );
    assert_eq!(refs(&lineage), vec![r("a.id", Role::Join), r("b.id", Role::Join)]);
}

#[test]
fn using_key_is_unambiguous_unqualified() {
    let lineage = pg("SELECT id FROM a JOIN b USING (id)").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("id", &["a.id", "b.id"])]);
}

#[test]
fn natural_join_merges_common_columns() {
    let lineage = pg("SELECT * FROM a NATURAL JOIN b").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("id", &["a.id", "b.id"]), col("x", &["a.x"]), col("y", &["b.y"])]
    );
}

#[test]
fn ambiguous_unqualified_column_errors() {
    assert!(matches!(
        pg("SELECT id FROM a, b"),
        Err(LineageError::AmbiguousColumn { .. })
    ));
}

#[test]
fn unqualified_column_binds_to_its_only_owner() {
    let lineage = pg("SELECT x, y FROM a, b").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("x", &["a.x"]), col("y", &["b.y"])]);
}

#[test]
fn nonexistent_column_in_single_table_errors() {
    assert!(matches!(
        pg("SELECT nope FROM users"),
        Err(LineageError::ColumnNotFound { .. })
    ));
    assert!(matches!(
        pg("SELECT u.nope FROM users u"),
        Err(LineageError::ColumnNotFound { .. })
    ));
}

#[test]
fn correlated_outer_reference_takes_role_where_it_appears() {
    let lineage = pg("SELECT (SELECT b.y FROM b ORDER BY a.x LIMIT 1) AS v FROM a").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("v", &["b.y"])]);
    assert_eq!(refs(&lineage), vec![r("a.x", Role::Sort)]);

    let lineage = pg("SELECT a.x FROM a WHERE EXISTS (SELECT 1 FROM b WHERE b.id = a.id)").expect("analyze");
    assert_eq!(refs(&lineage), vec![r("a.id", Role::Filter), r("b.id", Role::Filter)]);
}

#[test]
fn unqualified_outer_reference_binds_through_scope_chain() {
    let lineage =
        pg("SELECT id FROM users WHERE EXISTS (SELECT 1 FROM orders WHERE user_id = email)").expect("analyze");
    assert_eq!(
        refs(&lineage),
        vec![r("orders.user_id", Role::Filter), r("users.email", Role::Filter)]
    );
}

#[test]
fn alias_column_list_renames_positionally() {
    let lineage = pg("SELECT p FROM (SELECT id, email FROM users) AS d(p)").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("p", &["users.id"])]);
    let lineage = pg("SELECT email FROM (SELECT id, email FROM users) AS d(p)").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("email", &["users.email"])]);
}

#[test]
fn alias_column_list_too_long_errors() {
    assert!(matches!(
        pg("SELECT * FROM (SELECT id FROM users) AS d(p, q)"),
        Err(LineageError::ColumnCountMismatch { .. })
    ));
    assert!(matches!(
        pg("WITH c(p, q) AS (SELECT id FROM users) SELECT * FROM c"),
        Err(LineageError::ColumnCountMismatch { .. })
    ));
}

#[test]
fn case_condition_is_a_projection_source() {
    let lineage = pg("SELECT CASE WHEN ssn = '1' THEN 'a' ELSE 'b' END AS c FROM users").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("c", &["users.ssn"])]);
    assert!(lineage.references.is_empty());
}

#[test]
fn aggregate_arguments_are_projection_sources() {
    let lineage = pg("SELECT count(*), max(ssn) FILTER (WHERE email = 'x') AS m FROM users").expect("analyze");
    assert_eq!(columns(&lineage)[0].1, Vec::<String>::new());
    assert_eq!(columns(&lineage)[1], col("m", &["users.email", "users.ssn"]));
}

#[test]
fn clause_roles_are_recorded() {
    let lineage = pg(
        "SELECT email, count(*) AS n FROM users WHERE id > 1 GROUP BY email HAVING max(ssn) > '1' \
         ORDER BY n",
    )
    .expect("analyze");
    assert_eq!(
        refs(&lineage),
        vec![
            r("users.email", Role::GroupBy),
            r("users.id", Role::Filter),
            r("users.ssn", Role::Filter)
        ]
    );
}

#[test]
fn window_role_covers_partition_and_order() {
    let lineage = pg("SELECT row_number() OVER (PARTITION BY email ORDER BY ssn) AS n FROM users").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("n", &[])]);
    assert_eq!(
        refs(&lineage),
        vec![r("users.email", Role::Window), r("users.ssn", Role::Window)]
    );
}

#[test]
fn subquery_column_takes_enclosing_role() {
    let lineage = pg("SELECT id FROM users WHERE email IN (SELECT ssn FROM users WHERE id = 2)").expect("analyze");
    assert_eq!(
        refs(&lineage),
        vec![
            r("users.email", Role::Filter),
            r("users.id", Role::Filter),
            r("users.ssn", Role::Filter)
        ]
    );
}

#[test]
fn order_by_position_and_alias_resolve_to_output() {
    let lineage = pg("SELECT ssn AS s, email FROM users ORDER BY 2, s").expect("analyze");
    assert_eq!(
        refs(&lineage),
        vec![r("users.email", Role::Sort), r("users.ssn", Role::Sort)]
    );
}

#[test]
fn set_operation_unions_sources_per_position() {
    let lineage = pg("SELECT email FROM users UNION SELECT ssn FROM users ORDER BY 1").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("email", &["users.email", "users.ssn"])]);
    assert_eq!(
        refs(&lineage),
        vec![r("users.email", Role::Sort), r("users.ssn", Role::Sort)]
    );
}

#[test]
fn cte_and_derived_table_carry_sources() {
    let lineage = pg("WITH c AS (SELECT ssn AS k FROM users) SELECT d.k FROM (SELECT k FROM c) d").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("k", &["users.ssn"])]);
}

#[test]
fn cte_shadows_catalog_table() {
    let lineage = pg("WITH users AS (SELECT total FROM orders) SELECT * FROM users").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("total", &["orders.total"])]);
}

#[test]
fn relations_and_functions_are_recorded() {
    let lineage = pg("SELECT lower(email) FROM users").expect("analyze");
    let used: Vec<_> = lineage.relations.iter().map(|r| r.id.name.clone()).collect();
    assert_eq!(used, vec!["users"]);
    let function = lineage.functions.first().expect("function");
    assert_eq!(function.name, "lower");
    assert!(!function.known);
}

#[test]
fn missing_lists_every_unknown_name_in_one_call() {
    let sql = "SELECT t1.a FROM t1 JOIN s.t2 ON t1.a = t2.b WHERE EXISTS (SELECT 1 FROM \"T3\")";
    let mut catalog = Catalog::new();
    let Err(LineageError::Missing(names)) = analyze(sql, Dialect::Postgres, &catalog) else {
        panic!("expected Missing");
    };
    assert_eq!(
        names,
        vec![
            WrittenName::query(vec![NamePart::bare("t1")]),
            WrittenName::query(vec![NamePart::bare("s"), NamePart::bare("t2")]),
            WrittenName::query(vec![NamePart::quoted("T3")]),
        ]
    );

    for (name, table, cols) in [
        (&names[0], "t1", &["a"][..]),
        (&names[1], "t2", &["b"][..]),
        (&names[2], "T3", &["c"][..]),
    ] {
        catalog.insert(
            name.clone(),
            Entry::Resolved(relation(table, RelationKind::Table, cols)),
        );
    }
    let lineage = analyze(sql, Dialect::Postgres, &catalog).expect("analyze after insert");
    assert_eq!(columns(&lineage), vec![col("a", &["t1.a"])]);
}

#[test]
fn missing_takes_precedence_over_column_errors() {
    let result = analyze("SELECT nope FROM t1, users", Dialect::Postgres, &fixture());
    assert!(matches!(result, Err(LineageError::Missing(names)) if names.len() == 1));
}

#[test]
fn absent_entry_is_unknown_relation() {
    let mut catalog = fixture();
    catalog.insert(WrittenName::query(vec![NamePart::bare("ghost")]), Entry::Absent);
    assert!(matches!(
        analyze("SELECT * FROM ghost", Dialect::Postgres, &catalog),
        Err(LineageError::UnknownRelation { .. })
    ));
}

#[test]
fn postgres_matches_quoted_exactly_and_folds_unquoted() {
    let catalog = catalog(&[("t", &["Email", "email"])]);
    let quoted = analyze("SELECT \"Email\" FROM t", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(columns(&quoted), vec![col("Email", &["t.Email"])]);
    let bare = analyze("SELECT Email FROM t", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(columns(&bare), vec![col("email", &["t.email"])]);
    let only_upper = catalog_with_upper();
    assert!(matches!(
        analyze("SELECT Email FROM t", Dialect::Postgres, &only_upper),
        Err(LineageError::ColumnNotFound { .. })
    ));
}

/// A catalog whose table `t` has only the column `Email`.
fn catalog_with_upper() -> Catalog {
    catalog(&[("t", &["Email"])])
}

#[test]
fn postgres_quoted_relation_name_is_its_own_key() {
    let mut catalog = Catalog::new();
    catalog.insert(
        WrittenName::query(vec![NamePart::quoted("Users")]),
        Entry::Resolved(relation("Users", RelationKind::Table, &["id"])),
    );
    let lineage = analyze("SELECT id FROM \"Users\"", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("id", &["Users.id"])]);
    assert!(matches!(
        analyze("SELECT id FROM users", Dialect::Postgres, &catalog),
        Err(LineageError::Missing(_))
    ));
}

#[test]
fn mysql_and_sqlite_match_columns_case_insensitively() {
    for dialect in [Dialect::Mysql, Dialect::Mariadb, Dialect::Sqlite] {
        let lineage = analyze("SELECT EMAIL, U.Ssn FROM users AS u", dialect, &fixture()).expect("analyze");
        assert_eq!(
            columns(&lineage),
            vec![col("EMAIL", &["users.email"]), col("Ssn", &["users.ssn"])],
            "{dialect:?}"
        );
    }
}

#[test]
fn mysql_rejects_cte_name_differing_only_by_case() {
    let sql = "WITH C AS (SELECT id FROM users) SELECT * FROM c";
    assert_unsupported(sql, Dialect::Mysql, &fixture());
    assert!(analyze(sql, Dialect::Sqlite, &fixture()).is_ok());
}

#[test]
fn sqlite_where_falls_back_to_output_alias() {
    let sql = "SELECT ssn AS k FROM users WHERE k = '1'";
    let lineage = analyze(sql, Dialect::Sqlite, &fixture()).expect("analyze");
    assert_eq!(refs(&lineage), vec![r("users.ssn", Role::Filter)]);
    assert!(matches!(pg(sql), Err(LineageError::ColumnNotFound { .. })));
}

#[test]
fn lexical_mismatch_rejects() {
    let mut catalog = fixture();
    catalog.set_lexical(Lexical::Mismatch("ANSI_QUOTES".to_string()));
    assert!(matches!(
        analyze("SELECT id FROM users", Dialect::Mysql, &catalog),
        Err(LineageError::LexicalMismatch(_))
    ));
}

#[test]
fn removed_fail_open_arms_now_error() {
    let cases: &[(&str, Dialect)] = &[
        ("SHOW TABLES", Dialect::Mysql),
        ("INSERT INTO users (id) VALUES (1)", Dialect::Postgres),
        ("UPDATE users SET email = 'x'", Dialect::Postgres),
        ("DELETE FROM users", Dialect::Postgres),
        ("CREATE TABLE t2 (id INT)", Dialect::Postgres),
        ("SELECT * FROM users TABLESAMPLE BERNOULLI (10)", Dialect::Postgres),
        ("SELECT * FROM generate_series(1, 3)", Dialect::Postgres),
        ("SELECT * FROM UNNEST(ARRAY[1, 2])", Dialect::Postgres),
        ("SELECT * FROM users, LATERAL (SELECT 1) l", Dialect::Postgres),
        ("WITH RECURSIVE r AS (SELECT 1) SELECT * FROM r", Dialect::Postgres),
        ("SELECT row_to_json(u.*) FROM users u", Dialect::Postgres),
        ("SELECT id INTO t2 FROM users", Dialect::Postgres),
    ];
    for (sql, dialect) in cases {
        assert_unsupported(sql, *dialect, &fixture());
    }
}

#[test]
fn wildcard_alias_is_unsupported() {
    // PostgreSQL parses `* AS name`; the other modifiers fail in the parser.
    for sql in ["SELECT * AS u FROM users", "SELECT users.* AS u FROM users"] {
        let result = pg(sql);
        assert!(matches!(result, Err(LineageError::Unsupported(_))), "{sql}: {result:?}");
    }
}

#[test]
fn long_operator_chain_hits_depth_limit_without_overflow() {
    let terms = vec!["1"; 1_000].join("+");
    let sql = format!("SELECT {terms} AS x");
    let result = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let result = analyze(&sql, Dialect::Postgres, &Catalog::new());
            // Drop the parsed tree's result here, inside the bounded stack.
            matches!(result, Err(LineageError::DepthLimit))
        })
        .expect("spawn")
        .join()
        .expect("no stack overflow");
    assert!(result);
}

#[test]
fn missing_survives_errors_hidden_relations_cause() {
    for sql in [
        "SELECT * FROM (SELECT * FROM m1 UNION SELECT 1) d JOIN m2 ON true",
        "WITH c AS (SELECT * FROM m1 ORDER BY 2) SELECT * FROM c, m2",
    ] {
        let Err(LineageError::Missing(names)) = pg(sql) else {
            panic!("{sql}: expected Missing");
        };
        assert_eq!(names.len(), 2, "{sql}");
    }
}

#[test]
fn order_by_bare_output_name_wins_over_input_column() {
    let lineage = pg("SELECT id AS email FROM users ORDER BY email").expect("analyze");
    assert_eq!(refs(&lineage), vec![r("users.id", Role::Sort)]);
}

#[test]
fn star_without_from_errors() {
    assert!(matches!(pg("SELECT *"), Err(LineageError::ColumnNotFound { .. })));
}

#[test]
fn view_is_rejected_until_expansion_lands() {
    let mut catalog = fixture();
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("v")]),
        Entry::Resolved(relation("v", RelationKind::View, &["id"]).with_view_sql("SELECT id FROM users")),
    );
    assert_unsupported("SELECT * FROM v", Dialect::Postgres, &catalog);
}

#[test]
fn parse_and_statement_count_errors() {
    assert!(matches!(pg("SELEC 1"), Err(LineageError::Parse(_))));
    assert!(matches!(pg(""), Err(LineageError::Parse(_))));
    assert!(matches!(pg("SELECT 1; SELECT 2"), Err(LineageError::MultiStatement)));
}

#[test]
fn nesting_resolves_up_to_the_parser_recursion_limit() {
    let nested = |levels| {
        let mut sql = "SELECT id FROM users".to_string();
        for _ in 0..levels {
            sql = format!("SELECT id FROM ({sql}) AS d");
        }
        sql
    };
    let lineage = pg(&nested(20)).expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![("id".to_string(), vec!["users.id".to_string()])]
    );

    let result = pg(&nested(25));
    assert!(matches!(result, Err(LineageError::Parse(_))), "{result:?}");
}

#[test]
fn long_set_operation_chain_does_not_overflow() {
    let sql = vec!["SELECT id FROM users"; 2_000].join(" UNION ");
    let lineage = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || pg(&sql).map(|lineage| columns(&lineage)))
        .expect("spawn")
        .join()
        .expect("no stack overflow");
    assert_eq!(lineage, Ok(vec![("id".to_string(), vec!["users.id".to_string()])]));
}

#[test]
fn on_sees_its_join_tree_not_earlier_from_items() {
    let catalog = catalog(&[("a", &["id", "x"]), ("b", &["id", "y"]), ("c", &["x", "y"])]);
    let lineage = analyze("SELECT 1 FROM a, b JOIN c ON x = c.y", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(refs(&lineage), vec![r("c.x", Role::Join), r("c.y", Role::Join)]);

    let result = analyze("SELECT 1 FROM a, b JOIN c ON a.x = c.x", Dialect::Postgres, &catalog);
    assert!(matches!(result, Err(LineageError::ColumnNotFound { .. })), "{result:?}");
}

#[test]
fn open_relations_defer_errors_until_the_catalog_fills() {
    for sql in [
        "SELECT * FROM m1 UNION SELECT id, x FROM a",
        "SELECT * FROM (SELECT * FROM m1) d(p, q, r)",
        "SELECT * FROM a JOIN m1 USING (x)",
        "SELECT m.* FROM m1 m",
        "SELECT * FROM m1 GROUP BY 5",
        "WITH c AS (SELECT * FROM m1) SELECT c1.k FROM c c1 JOIN c c2 ON c1.k = c2.k",
        "SELECT (SELECT nope FROM m1) FROM users",
    ] {
        let result = pg(sql);
        assert!(
            matches!(&result, Err(LineageError::Missing(names)) if names.len() == 1),
            "{sql}: expected Missing, got {result:?}"
        );
    }
}

#[test]
fn errors_independent_of_missing_relations_surface_at_once() {
    for sql in [
        "SELECT nope FROM users WHERE EXISTS (SELECT 1 FROM m1)",
        "SELECT * FROM a JOIN m1 USING (nope)",
    ] {
        let result = pg(sql);
        assert!(
            matches!(result, Err(LineageError::ColumnNotFound { .. })),
            "{sql}: expected ColumnNotFound, got {result:?}"
        );
    }
}

/// One expression per pass-through arm, with the exact sources it must keep.
///
/// Each operand reads a different column, so an arm that drops a child
/// loses a source (the fail-open direction for redaction).
const PASS_THROUGH: &[(Dialect, &str, &[&str])] = &[
    (Dialect::Postgres, "CAST(u.ssn AS TEXT)", &["users.ssn"]),
    (Dialect::Postgres, "(u.ssn)", &["users.ssn"]),
    (Dialect::Postgres, "-u.id", &["users.id"]),
    (Dialect::Postgres, "u.ssn IS NULL", &["users.ssn"]),
    (Dialect::Postgres, "EXTRACT(YEAR FROM u.ssn)", &["users.ssn"]),
    (Dialect::Postgres, "u.email COLLATE \"C\"", &["users.email"]),
    (Dialect::Postgres, "CEIL(o.total)", &["orders.total"]),
    (Dialect::Postgres, "u.email || u.ssn", &["users.email", "users.ssn"]),
    (
        Dialect::Postgres,
        "u.email IS DISTINCT FROM u.ssn",
        &["users.email", "users.ssn"],
    ),
    (Dialect::Postgres, "u.id = ANY(o.total)", &["orders.total", "users.id"]),
    (
        Dialect::Postgres,
        "POSITION(u.email IN u.ssn)",
        &["users.email", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "u.ssn AT TIME ZONE u.email",
        &["users.email", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "u.email LIKE u.ssn ESCAPE '!'",
        &["users.email", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "u.id IN (u.ssn, o.total)",
        &["orders.total", "users.id", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "u.id IN (SELECT o2.total FROM orders o2)",
        &["orders.total", "users.id"],
    ),
    (
        Dialect::Postgres,
        "u.id BETWEEN u.ssn AND o.total",
        &["orders.total", "users.id", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "SUBSTRING(u.email FROM u.id FOR o.total)",
        &["orders.total", "users.email", "users.id"],
    ),
    (
        Dialect::Postgres,
        "TRIM(BOTH u.ssn FROM u.email)",
        &["users.email", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "OVERLAY(u.email PLACING u.ssn FROM u.id FOR o.total)",
        &["orders.total", "users.email", "users.id", "users.ssn"],
    ),
    (Dialect::Postgres, "(u.id, u.ssn)", &["users.id", "users.ssn"]),
    (
        Dialect::Postgres,
        "ARRAY[u.email, u.ssn]",
        &["users.email", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "(ARRAY[u.email])[u.id]",
        &["users.email", "users.id"],
    ),
    (
        Dialect::Postgres,
        "(ARRAY[u.email])[u.id:o.total]",
        &["orders.total", "users.email", "users.id"],
    ),
    (
        Dialect::Postgres,
        "EXISTS (SELECT o2.total FROM orders o2)",
        &["orders.total"],
    ),
    (
        Dialect::Postgres,
        "count(u.id) FILTER (WHERE u.ssn IS NULL)",
        &["users.id", "users.ssn"],
    ),
    (
        Dialect::Postgres,
        "percentile_cont(0.5) WITHIN GROUP (ORDER BY o.total)",
        &["orders.total"],
    ),
    (
        Dialect::Postgres,
        "string_agg(u.email, ',' ORDER BY u.ssn)",
        &["users.email", "users.ssn"],
    ),
    (Dialect::Postgres, "u.email ->> u.ssn", &["users.email", "users.ssn"]),
    (Dialect::Sqlite, "u.email ->> u.ssn", &["users.email", "users.ssn"]),
    (Dialect::Mysql, "u.email ->> '$.a'", &["users.email"]),
    (
        Dialect::Mysql,
        "JSON_EXTRACT(u.email, u.ssn)",
        &["users.email", "users.ssn"],
    ),
    (Dialect::Mysql, "u.email RLIKE u.ssn", &["users.email", "users.ssn"]),
    (Dialect::Mysql, "u.id MEMBER OF (u.ssn)", &["users.id", "users.ssn"]),
    (
        Dialect::Mysql,
        "MATCH (u.email, u.ssn) AGAINST ('x')",
        &["users.email", "users.ssn"],
    ),
    (Dialect::Mysql, "u.ssn + INTERVAL u.id DAY", &["users.id", "users.ssn"]),
    (Dialect::Mysql, "CONVERT(u.ssn, CHAR)", &["users.ssn"]),
];

#[test]
fn pass_through_expressions_keep_every_operand_source() {
    for (dialect, expr, sources) in PASS_THROUGH {
        let sql = format!("SELECT {expr} AS v FROM users u, orders o");
        let lineage = analyze(&sql, *dialect, &fixture()).unwrap_or_else(|e| panic!("{expr} on {dialect:?}: {e}"));
        assert_eq!(columns(&lineage), vec![col("v", sources)], "{expr} on {dialect:?}");
    }
}

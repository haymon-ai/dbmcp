//! Unit tests for catalog-aware lineage, one per resolution rule and error.

use std::collections::BTreeSet;

use crate::{
    Catalog, CatalogColumn, Dialect, Entry, Lexical, Lineage, LineageError, NamePart, Origin, Relation, RelationId,
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
fn using_key_is_unambiguous_unqualified() {
    let lineage = pg("SELECT id FROM a JOIN b USING (id)").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("id", &["a.id", "b.id"])]);
}

#[test]
fn ambiguous_unqualified_column_errors() {
    assert_eq!(
        pg("SELECT id FROM a, b"),
        Err(LineageError::AmbiguousColumn {
            column: "id".to_string()
        })
    );
}

#[test]
fn unqualified_column_binds_to_its_only_owner() {
    let lineage = pg("SELECT x, y FROM a, b").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("x", &["a.x"]), col("y", &["b.y"])]);
}

#[test]
fn nonexistent_column_in_single_table_errors() {
    for (sql, column) in [
        ("SELECT nope FROM users", "nope"),
        ("SELECT u.nope FROM users u", "u.nope"),
    ] {
        assert_eq!(
            pg(sql),
            Err(LineageError::ColumnNotFound {
                column: column.to_string()
            }),
            "{sql}"
        );
    }
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
    for (sql, relation) in [
        ("SELECT * FROM (SELECT id FROM users) AS d(p, q)", "d"),
        ("WITH c(p, q) AS (SELECT id FROM users) SELECT * FROM c", "c"),
    ] {
        assert_eq!(
            pg(sql),
            Err(LineageError::ColumnCountMismatch {
                relation: relation.to_string(),
                expected: 1,
                found: 2,
            }),
            "{sql}"
        );
    }
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
    assert!(function.known);
}

#[test]
fn known_and_unknown_functions_both_carry_argument_sources() {
    let lineage =
        pg("SELECT lower(ssn), leak(ssn), pg_catalog.upper(email), public.lower(id) FROM users").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![
            col("lower(ssn)", &["users.ssn"]),
            col("leak(ssn)", &["users.ssn"]),
            col("pg_catalog.upper(email)", &["users.email"]),
            col("public.lower(id)", &["users.id"]),
        ]
    );
    let known: Vec<_> = lineage.functions.iter().map(|f| (f.name.as_str(), f.known)).collect();
    assert_eq!(
        known,
        vec![
            ("leak", false),
            ("lower", true),
            ("pg_catalog.upper", true),
            ("public.lower", false)
        ]
    );
}

/// Renders recorded functions as sorted `(name, known)` pairs.
fn functions(lineage: &Lineage) -> Vec<(&str, bool)> {
    lineage.functions.iter().map(|f| (f.name.as_str(), f.known)).collect()
}

#[test]
fn functions_are_recorded_in_every_clause() {
    let lineage = pg(
        "SELECT count(*) FILTER (WHERE lower(email) = 'x'), row_number() OVER (ORDER BY upper(ssn)) \
         FROM users WHERE abs(id) > 1 GROUP BY md5(email) ORDER BY reverse(email)",
    )
    .expect("analyze");
    assert_eq!(
        functions(&lineage),
        vec![
            ("abs", true),
            ("count", true),
            ("lower", true),
            ("md5", true),
            ("reverse", true),
            ("row_number", true),
            ("upper", true),
        ]
    );
}

#[test]
fn nested_and_subquery_functions_are_recorded() {
    let lineage = pg("SELECT lower(leak(ssn)), (SELECT max(id) FROM orders) FROM users").expect("analyze");
    assert_eq!(
        functions(&lineage),
        vec![("leak", false), ("lower", true), ("max", true)]
    );
    assert_eq!(columns(&lineage)[0], col("lower(leak(ssn))", &["users.ssn"]));
}

#[test]
fn repeated_function_is_recorded_once() {
    let lineage = pg("SELECT lower(email), lower(ssn) FROM users").expect("analyze");
    assert_eq!(functions(&lineage), vec![("lower", true)]);
}

#[test]
fn special_forms_are_not_recorded() {
    let lineage = pg("SELECT CAST(ssn AS int), substring(ssn from 1 for 3) FROM users").expect("analyze");
    assert!(lineage.functions.is_empty(), "{:?}", lineage.functions);
    assert_eq!(
        columns(&lineage).into_iter().map(|(_, s)| s).collect::<Vec<_>>(),
        vec![vec!["users.ssn".to_string()], vec!["users.ssn".to_string()]]
    );
}

#[test]
fn value_exposing_built_ins_are_unknown() {
    let cases: &[(&str, Dialect, &str)] = &[
        ("SELECT xmlelement(ssn) FROM users", Dialect::Postgres, "xmlelement"),
        (
            "SELECT query_to_xml('SELECT ssn FROM users', true, false, '')",
            Dialect::Postgres,
            "query_to_xml",
        ),
        (
            "SELECT current_setting('app.key')",
            Dialect::Postgres,
            "current_setting",
        ),
        (
            "SELECT EXTRACTVALUE(ssn, '/a') FROM users",
            Dialect::Mysql,
            "extractvalue",
        ),
        (
            "SELECT EXTRACTVALUE(ssn, '/a') FROM users",
            Dialect::Mariadb,
            "extractvalue",
        ),
        ("SELECT load_extension('x')", Dialect::Sqlite, "load_extension"),
        (
            "SELECT convert_to(ssn, 'LATIN1') FROM users",
            Dialect::Postgres,
            "convert_to",
        ),
        ("SELECT unistr(ssn) FROM users", Dialect::Postgres, "unistr"),
    ];
    for (sql, dialect, name) in cases {
        let lineage = analyze(sql, *dialect, &fixture()).expect(sql);
        let function = lineage.functions.first().expect("function");
        assert_eq!(function.name, *name, "{sql}");
        assert!(!function.known, "{sql}");
    }
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
fn absent_entry_is_unknown_relation() {
    let mut catalog = fixture();
    catalog.insert(WrittenName::query(vec![NamePart::bare("ghost")]), Entry::Absent);
    assert_eq!(
        analyze("SELECT * FROM ghost", Dialect::Postgres, &catalog),
        Err(LineageError::UnknownRelation {
            name: "ghost".to_string()
        })
    );
}

#[test]
fn postgres_matches_quoted_exactly_and_folds_unquoted() {
    let both = catalog(&[("t", &["Email", "email"])]);
    let quoted = analyze("SELECT \"Email\" FROM t", Dialect::Postgres, &both).expect("analyze");
    assert_eq!(columns(&quoted), vec![col("Email", &["t.Email"])]);
    let bare = analyze("SELECT Email FROM t", Dialect::Postgres, &both).expect("analyze");
    assert_eq!(columns(&bare), vec![col("email", &["t.email"])]);
    assert_eq!(
        analyze("SELECT Email FROM t", Dialect::Postgres, &catalog(&[("t", &["Email"])])),
        Err(LineageError::ColumnNotFound {
            column: "Email".to_string()
        })
    );
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
    assert_eq!(
        analyze("SELECT id FROM users", Dialect::Postgres, &catalog),
        Err(LineageError::Missing(vec![WrittenName::query(vec![NamePart::bare(
            "users"
        )])]))
    );
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
    assert_eq!(
        pg(sql),
        Err(LineageError::ColumnNotFound {
            column: "k".to_string()
        })
    );
}

#[test]
fn lexical_mismatch_rejects() {
    let mut catalog = fixture();
    catalog.set_lexical(Lexical::Mismatch("ANSI_QUOTES".to_string()));
    assert_eq!(
        analyze("SELECT id FROM users", Dialect::Mysql, &catalog),
        Err(LineageError::LexicalMismatch("ANSI_QUOTES".to_string()))
    );
}

#[test]
fn unsupported_constructs_fail_closed() {
    let cases: &[(&str, Dialect)] = &[
        ("INSERT INTO users (id) VALUES (1)", Dialect::Postgres),
        ("SELECT * FROM users TABLESAMPLE BERNOULLI (10)", Dialect::Postgres),
        ("SELECT * FROM generate_series(1, 3)", Dialect::Postgres),
        ("SELECT * FROM UNNEST(ARRAY[1, 2])", Dialect::Postgres),
        ("SELECT * FROM users, LATERAL (SELECT 1) l", Dialect::Postgres),
        ("WITH RECURSIVE r AS (SELECT 1) SELECT * FROM r", Dialect::Postgres),
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
        assert_unsupported(sql, Dialect::Postgres, &fixture());
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
fn order_by_bare_output_name_wins_over_input_column() {
    let lineage = pg("SELECT id AS email FROM users ORDER BY email").expect("analyze");
    assert_eq!(refs(&lineage), vec![r("users.id", Role::Sort)]);
}

#[test]
fn star_without_from_errors() {
    assert_eq!(
        pg("SELECT *"),
        Err(LineageError::ColumnNotFound {
            column: "*".to_string()
        })
    );
}

#[test]
fn parse_and_statement_count_errors() {
    assert!(matches!(pg("SELEC 1"), Err(LineageError::Parse(_))));
    assert!(matches!(pg(""), Err(LineageError::Parse(_))));
    assert_eq!(pg("SELECT 1; SELECT 2"), Err(LineageError::MultiStatement));
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
    assert_eq!(columns(&lineage), vec![col("id", &["users.id"])]);

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
    assert_eq!(lineage, Ok(vec![col("id", &["users.id"])]));
}

#[test]
fn on_sees_its_join_tree_not_earlier_from_items() {
    let catalog = catalog(&[("a", &["id", "x"]), ("b", &["id", "y"]), ("c", &["x", "y"])]);
    let lineage = analyze("SELECT 1 FROM a, b JOIN c ON x = c.y", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(refs(&lineage), vec![r("c.x", Role::Join), r("c.y", Role::Join)]);

    assert_eq!(
        analyze("SELECT 1 FROM a, b JOIN c ON a.x = c.x", Dialect::Postgres, &catalog),
        Err(LineageError::ColumnNotFound {
            column: "a.x".to_string()
        })
    );
}

#[test]
fn open_relations_defer_errors_until_the_catalog_fills() {
    for (sql, missing) in [
        ("SELECT * FROM m1 UNION SELECT id, x FROM a", &["m1"][..]),
        ("SELECT * FROM (SELECT * FROM m1) d(p, q, r)", &["m1"]),
        ("SELECT * FROM a JOIN m1 USING (x)", &["m1"]),
        ("SELECT m.* FROM m1 m", &["m1"]),
        ("SELECT * FROM m1 GROUP BY 5", &["m1"]),
        (
            "WITH c AS (SELECT * FROM m1) SELECT c1.k FROM c c1 JOIN c c2 ON c1.k = c2.k",
            &["m1"],
        ),
        ("SELECT (SELECT nope FROM m1) FROM users", &["m1"]),
        ("SELECT nope FROM m1, users", &["m1"]),
        // A second missing name is still collected past the open relation.
        (
            "SELECT * FROM (SELECT * FROM m1 UNION SELECT 1) d JOIN m2 ON true",
            &["m1", "m2"],
        ),
        (
            "WITH c AS (SELECT * FROM m1 ORDER BY 2) SELECT * FROM c, m2",
            &["m1", "m2"],
        ),
    ] {
        let missing = missing
            .iter()
            .map(|name| WrittenName::query(vec![NamePart::bare(*name)]))
            .collect();
        assert_eq!(pg(sql), Err(LineageError::Missing(missing)), "{sql}");
    }
}

#[test]
fn errors_independent_of_missing_relations_surface_at_once() {
    for sql in [
        "SELECT nope FROM users WHERE EXISTS (SELECT 1 FROM m1)",
        "SELECT * FROM a JOIN m1 USING (nope)",
    ] {
        assert_eq!(
            pg(sql),
            Err(LineageError::ColumnNotFound {
                column: "nope".to_string()
            }),
            "{sql}"
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

#[test]
fn values_names_columns_per_dialect_and_merges_rows() {
    let rows = [
        (
            Dialect::Postgres,
            "VALUES (1, 'a'), (2, 'b')",
            vec![col("column1", &[]), col("column2", &[])],
        ),
        (
            Dialect::Sqlite,
            "VALUES (1, 'a')",
            vec![col("column1", &[]), col("column2", &[])],
        ),
        (
            Dialect::Mysql,
            "VALUES ROW(1, 2)",
            vec![col("column_0", &[]), col("column_1", &[])],
        ),
        (
            Dialect::Mariadb,
            "VALUES (1, 2)",
            vec![col("column_0", &[]), col("column_1", &[])],
        ),
        (
            Dialect::Postgres,
            "VALUES ((SELECT email FROM users)), ((SELECT ssn FROM users))",
            vec![col("column1", &["users.email", "users.ssn"])],
        ),
        (
            Dialect::Postgres,
            "SELECT email FROM users UNION VALUES ((SELECT ssn FROM users))",
            vec![col("email", &["users.email", "users.ssn"])],
        ),
        (
            Dialect::Postgres,
            "SELECT * FROM (VALUES (1), (2)) AS v(x)",
            vec![col("x", &[])],
        ),
    ];
    for (dialect, sql, expected) in rows {
        let lineage = analyze(sql, dialect, &fixture()).unwrap_or_else(|e| panic!("{sql} on {dialect:?}: {e}"));
        assert_eq!(columns(&lineage), expected, "{sql} on {dialect:?}");
    }

    let result = pg("SELECT * FROM (VALUES (1, 2), (3)) AS v");
    assert_eq!(
        result,
        Err(LineageError::ColumnCountMismatch {
            relation: "VALUES".to_string(),
            expected: 2,
            found: 1,
        })
    );
}

#[test]
fn every_join_kind_merges_shared_keys_first_with_both_sources() {
    // Outer joins keep both key sources: a superset of the merged column's
    // real source, which errs on the side of redaction.
    for join in [
        "JOIN b USING (id)",
        "INNER JOIN b USING (id)",
        "LEFT JOIN b USING (id)",
        "LEFT OUTER JOIN b USING (id)",
        "RIGHT JOIN b USING (id)",
        "RIGHT OUTER JOIN b USING (id)",
        "FULL JOIN b USING (id)",
        "NATURAL JOIN b",
    ] {
        let sql = format!("SELECT * FROM a {join}");
        let lineage = pg(&sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
        assert_eq!(
            columns(&lineage),
            vec![col("id", &["a.id", "b.id"]), col("x", &["a.x"]), col("y", &["b.y"])],
            "{sql}"
        );
        assert_eq!(
            refs(&lineage),
            vec![r("a.id", Role::Join), r("b.id", Role::Join)],
            "{sql}"
        );
    }

    let lineage = pg("SELECT * FROM a CROSS JOIN b").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![
            col("id", &["a.id"]),
            col("x", &["a.x"]),
            col("id", &["b.id"]),
            col("y", &["b.y"])
        ]
    );
    assert!(lineage.references.is_empty());

    let lineage = analyze(
        "SELECT a.x FROM a STRAIGHT_JOIN b ON a.id = b.id",
        Dialect::Mysql,
        &fixture(),
    )
    .expect("analyze");
    assert_eq!(columns(&lineage), vec![col("x", &["a.x"])]);
    assert_eq!(refs(&lineage), vec![r("a.id", Role::Join), r("b.id", Role::Join)]);
}

#[test]
fn nested_join_alias_hides_inner_relations() {
    let lineage = pg("SELECT * FROM (a JOIN b USING (id)) AS j").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("id", &["a.id", "b.id"]), col("x", &["a.x"]), col("y", &["b.y"])]
    );

    let lineage = pg("SELECT j.q FROM (a JOIN b ON a.id = b.id) AS j(p, q)").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("q", &["a.x"])]);

    let lineage = pg("SELECT a.x, orders.total FROM (a JOIN b USING (id)) JOIN orders ON orders.user_id = a.id")
        .expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("x", &["a.x"]), col("total", &["orders.total"])]
    );

    for (sql, column) in [
        ("SELECT a.x FROM (a JOIN b USING (id)) AS j", "a.x"),
        ("SELECT j.x FROM (a JOIN b ON a.id = b.id) AS j(p, q)", "j.x"),
    ] {
        let result = pg(sql);
        assert_eq!(
            result,
            Err(LineageError::ColumnNotFound {
                column: column.to_string()
            }),
            "{sql}"
        );
    }
}

const ALL_USERS: &[&str] = &["users.email", "users.id", "users.secret", "users.ssn"];

#[test]
fn postgres_whole_row_references_cover_every_column() {
    for sql in [
        "SELECT row_to_json(u) FROM users u",
        "SELECT (u).ssn FROM users u",
        "SELECT f(u.*) FROM users u",
        "SELECT ROW(u.*) FROM users u",
        "SELECT users FROM users",
    ] {
        let lineage = pg(sql).expect(sql);
        assert_eq!(lineage.columns.len(), 1, "{sql}");
        assert_eq!(names(&lineage.columns[0].sources), ALL_USERS, "{sql}");
    }
    let lineage = pg("SELECT 1 FROM users u WHERE u IS NOT NULL").expect("analyze");
    let filtered: Vec<_> = refs(&lineage).into_iter().map(|(s, _)| s).collect();
    assert_eq!(filtered, ALL_USERS);
}

#[test]
fn postgres_column_name_wins_over_relation_name() {
    let catalog = catalog(&[("t", &["t", "x"])]);
    let lineage = analyze("SELECT row_to_json(t) FROM t", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(names(&lineage.columns[0].sources), vec!["t.t"]);
}

#[test]
fn whole_row_references_are_postgres_only() {
    for dialect in [Dialect::Mysql, Dialect::Mariadb, Dialect::Sqlite] {
        for sql in [
            "SELECT row_to_json(u) FROM users u",
            "SELECT (u).ssn FROM users u",
            "SELECT f(u.*) FROM users u",
        ] {
            let result = analyze(sql, dialect, &fixture());
            assert!(
                matches!(result, Err(LineageError::ColumnNotFound { .. })),
                "{sql} on {dialect:?}: {result:?}"
            );
        }
    }
}

#[test]
fn subscripted_qualified_column_binds_the_column() {
    let catalog = catalog(&[("orders", &["id", "tags"]), ("events", &["o", "id"])]);
    let lineage = analyze(
        "SELECT o.tags[1] FROM orders o JOIN events e ON e.o = o.id",
        Dialect::Postgres,
        &catalog,
    )
    .expect("analyze");
    assert_eq!(names(&lineage.columns[0].sources), vec!["orders.tags"]);
    let result = analyze("SELECT o.nosuch[1] FROM orders o", Dialect::Postgres, &catalog);
    assert!(matches!(result, Err(LineageError::ColumnNotFound { .. })), "{result:?}");
}

#[test]
fn ambiguous_whole_row_name_falls_back_to_output_alias() {
    let mut catalog = Catalog::new();
    for schema in ["s1", "s2"] {
        catalog.insert(
            WrittenName::query(vec![NamePart::bare(schema), NamePart::bare("users")]),
            Entry::Resolved(Relation::new(
                RelationId::new("db", schema, "users"),
                RelationKind::Table,
                vec![CatalogColumn::new("id")],
            )),
        );
    }
    let lineage = analyze(
        "SELECT s1.users.id AS users FROM s1.users, s2.users GROUP BY users",
        Dialect::Postgres,
        &catalog,
    )
    .expect("analyze");
    let grouped: Vec<_> = refs(&lineage).into_iter().map(|(s, _)| s).collect();
    assert_eq!(grouped, vec!["users.id"]);
}

/// Returns the identity of fixture relation `name`.
fn id(name: &str) -> RelationId {
    RelationId::new("db", "public", name)
}

/// Inserts a view `name` with `columns` and `body`, written unqualified in `context`.
fn with_view(mut catalog: Catalog, context: Option<&str>, name: &str, columns: &[&str], body: &str) -> Catalog {
    catalog.insert(
        WrittenName {
            parts: vec![NamePart::bare(name)],
            context: context.map_or(Origin::Query, |view| Origin::View(id(view))),
        },
        Entry::Resolved(relation(name, RelationKind::View, columns).with_view_sql(body)),
    );
    catalog
}

/// Inserts the fixture's `users` as written inside view `view`.
fn users_in_view(mut catalog: Catalog, view: &str) -> Catalog {
    catalog.insert(
        WrittenName {
            parts: vec![NamePart::bare("users")],
            context: Origin::View(id(view)),
        },
        Entry::Resolved(relation(
            "users",
            RelationKind::Table,
            &["id", "email", "ssn", "~secret"],
        )),
    );
    catalog
}

#[test]
fn view_over_view_expands_to_base_columns() {
    let catalog = with_view(fixture(), None, "v2", &["mail"], "SELECT email FROM v1");
    let catalog = with_view(
        catalog,
        Some("v2"),
        "v1",
        &["email"],
        "SELECT email FROM users WHERE ssn <> ''",
    );
    let catalog = users_in_view(catalog, "v1");
    let lineage = analyze("SELECT * FROM v2", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("mail", &["users.email"])]);

    let reference = lineage.references.first().expect("reference");
    assert_eq!(reference.role, Role::Filter);
    assert_eq!(reference.origin, Origin::View(id("v1")));
}

#[test]
fn nested_views_tag_the_innermost_view() {
    let catalog = with_view(fixture(), None, "w", &["id"], "SELECT id FROM v");
    let catalog = with_view(catalog, Some("w"), "v", &["id"], "SELECT id FROM users");
    let catalog = users_in_view(catalog, "v");
    let lineage = analyze("SELECT * FROM w", Dialect::Postgres, &catalog).expect("analyze");
    let used: Vec<_> = lineage
        .relations
        .iter()
        .map(|r| (r.id.name.as_str(), r.kind, r.origin.clone()))
        .collect();
    assert_eq!(
        used,
        vec![
            ("users", RelationKind::Table, Origin::View(id("v"))),
            ("v", RelationKind::View, Origin::View(id("w"))),
            ("w", RelationKind::View, Origin::Query),
        ]
    );
}

#[test]
fn view_body_names_are_missing_in_the_view_context() {
    let catalog = with_view(fixture(), None, "v", &["id"], "SELECT id FROM users");
    assert_eq!(
        analyze("SELECT * FROM v", Dialect::Postgres, &catalog),
        Err(LineageError::Missing(vec![WrittenName {
            parts: vec![NamePart::bare("users")],
            context: Origin::View(id("v")),
        }]))
    );
    let lineage = analyze("SELECT * FROM v", Dialect::Postgres, &users_in_view(catalog, "v")).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("id", &["users.id"])]);
}

#[test]
fn open_view_body_keeps_the_catalog_shape() {
    // The body's `*` over a missing table would give no columns; the outer
    // query still binds `id` and reports the outer missing name too.
    let catalog = with_view(fixture(), None, "v", &["id", "name"], "SELECT * FROM users");
    assert_eq!(
        analyze(
            "SELECT v.id FROM v JOIN m1 ON v.name = m1.k",
            Dialect::Postgres,
            &catalog
        ),
        Err(LineageError::Missing(vec![
            WrittenName {
                parts: vec![NamePart::bare("users")],
                context: Origin::View(id("v")),
            },
            WrittenName::query(vec![NamePart::bare("m1")]),
        ]))
    );
    assert_eq!(
        analyze("SELECT v.nope FROM v", Dialect::Postgres, &catalog),
        Err(LineageError::ColumnNotFound {
            column: "v.nope".to_string()
        })
    );
}

#[test]
fn view_body_column_count_must_match_the_catalog() {
    let catalog = users_in_view(
        with_view(fixture(), None, "v", &["id"], "SELECT id, email FROM users"),
        "v",
    );
    assert_eq!(
        analyze("SELECT * FROM v", Dialect::Postgres, &catalog),
        Err(LineageError::ColumnCountMismatch {
            relation: "v".to_string(),
            expected: 1,
            found: 2,
        })
    );
}

#[test]
fn view_chain_past_the_depth_limit_errors_without_overflow() {
    /// Builds `v1 → v2 → … → v{depth}` with the last over `users`.
    fn chain(depth: usize) -> Catalog {
        let mut catalog = users_in_view(fixture(), &format!("v{depth}"));
        for i in 1..=depth {
            let context = (i > 1).then(|| format!("v{}", i - 1));
            let body = if i == depth {
                "SELECT id FROM users".to_string()
            } else {
                format!("SELECT id FROM v{}", i + 1)
            };
            catalog = with_view(catalog, context.as_deref(), &format!("v{i}"), &["id"], &body);
        }
        catalog
    }
    let (within, past) = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let within = analyze("SELECT * FROM v1", Dialect::Postgres, &chain(63)).is_ok();
            let past = analyze("SELECT * FROM v1", Dialect::Postgres, &chain(64));
            (within, matches!(past, Err(LineageError::DepthLimit)))
        })
        .expect("spawn")
        .join()
        .expect("no stack overflow");
    assert!(within);
    assert!(past);
}

#[test]
fn self_referencing_view_is_circular() {
    let catalog = with_view(fixture(), None, "v", &["id"], "SELECT id FROM v");
    let catalog = with_view(catalog, Some("v"), "v", &["id"], "SELECT id FROM v");
    assert_eq!(
        analyze("SELECT * FROM v", Dialect::Postgres, &catalog),
        Err(LineageError::CircularReference { name: "v".to_string() })
    );
}

#[test]
fn view_without_body_or_query_body_is_rejected() {
    let mut catalog = fixture();
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("v")]),
        Entry::Resolved(relation("v", RelationKind::View, &["id"])),
    );
    assert_unsupported("SELECT * FROM v", Dialect::Postgres, &catalog);
    let catalog = with_view(fixture(), None, "v", &["id"], "DELETE FROM users");
    assert_unsupported("SELECT * FROM v", Dialect::Postgres, &catalog);
    let catalog = with_view(fixture(), None, "v", &["id"], "SELEC id");
    assert!(matches!(
        analyze("SELECT * FROM v", Dialect::Postgres, &catalog),
        Err(LineageError::Parse(message)) if message.starts_with("view `v`: ")
    ));
}

#[test]
fn materialized_view_expands_and_needs_a_body() {
    let mut catalog = fixture();
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("mv")]),
        Entry::Resolved(relation("mv", RelationKind::MatView, &["s"])),
    );
    assert_unsupported("SELECT * FROM mv", Dialect::Postgres, &catalog);
    let mut catalog = users_in_view(fixture(), "mv");
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("mv")]),
        Entry::Resolved(relation("mv", RelationKind::MatView, &["s"]).with_view_sql("SELECT ssn FROM users")),
    );
    let lineage = analyze("SELECT * FROM mv", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("s", &["users.ssn"])]);
}

#[test]
fn view_referenced_twice_resolves_both_uses() {
    let catalog = users_in_view(with_view(fixture(), None, "v", &["s"], "SELECT ssn FROM users"), "v");
    let lineage = analyze(
        "SELECT a.s, b.s FROM v a JOIN v b ON a.s = b.s",
        Dialect::Postgres,
        &catalog,
    )
    .expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("s", &["users.ssn"]), col("s", &["users.ssn"])]
    );
    assert_eq!(refs(&lineage), vec![r("users.ssn", Role::Join)]);
}

#[test]
fn whole_row_over_a_view_covers_its_base_sources() {
    let catalog = users_in_view(
        with_view(fixture(), None, "v", &["id", "~s"], "SELECT id, ssn FROM users"),
        "v",
    );
    let lineage = analyze("SELECT row_to_json(v) FROM v", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(names(&lineage.columns[0].sources), ["users.id", "users.ssn"]);
}

#[test]
fn view_body_functions_carry_the_view_origin() {
    let catalog = users_in_view(
        with_view(fixture(), None, "v", &["e"], "SELECT lower(email) FROM users"),
        "v",
    );
    let lineage = analyze("SELECT upper(e) FROM v", Dialect::Postgres, &catalog).expect("analyze");
    let used: Vec<_> = lineage
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f.origin.clone()))
        .collect();
    assert_eq!(used, vec![("lower", Origin::View(id("v"))), ("upper", Origin::Query)]);
}

#[test]
fn views_using_a_view_twice_expand_it_once_per_pass() {
    // `v{i}` reads `v{i+1}` twice; the last reads a table the catalog lacks.
    // Re-expanding per use would walk 2^30 bodies before reporting `Missing`.
    let depth = 30;
    let mut catalog = fixture();
    for i in 1..=depth {
        let context = (i > 1).then(|| format!("v{}", i - 1));
        let body = if i == depth {
            "SELECT k FROM m1".to_string()
        } else {
            format!("SELECT a.id FROM v{n} a JOIN v{n} b ON a.id = b.id", n = i + 1)
        };
        catalog = with_view(catalog, context.as_deref(), &format!("v{i}"), &["id"], &body);
    }
    let started = std::time::Instant::now();
    assert_eq!(
        analyze("SELECT * FROM v1", Dialect::Postgres, &catalog),
        Err(LineageError::Missing(vec![WrittenName {
            parts: vec![NamePart::bare("m1")],
            context: Origin::View(id(&format!("v{depth}"))),
        }]))
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

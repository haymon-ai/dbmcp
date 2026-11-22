//! Unit tests for catalog-aware lineage, one per resolution rule and error.

use std::collections::BTreeSet;

use crate::{
    Catalog, CatalogColumn, Dialect, Entry, ExplainForm, Flow, Lexical, Lineage, LineageError, NamePart, Origin,
    Relation, RelationId, RelationKind, Role, Show, SourceRef, StatementKind, WrittenName, analyze,
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

/// Returns each output column's flow, in result order.
fn flows(lineage: &Lineage) -> Vec<Flow> {
    lineage.columns.iter().map(|c| c.flow).collect()
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
fn select_distinct_records_every_output_source() {
    let lineage = pg("SELECT DISTINCT lower(email) AS e, id FROM users").expect("analyze");
    assert_eq!(
        refs(&lineage),
        vec![r("users.email", Role::Distinct), r("users.id", Role::Distinct)]
    );
    let lineage = pg("SELECT DISTINCT * FROM (SELECT ssn FROM users) d").expect("analyze");
    assert_eq!(refs(&lineage), vec![r("users.ssn", Role::Distinct)]);
    let lineage = pg("SELECT ALL email FROM users").expect("analyze");
    assert_eq!(refs(&lineage), Vec::new());
}

#[test]
fn deduplicating_set_operations_record_every_output_source() {
    for sql in [
        "SELECT ssn FROM users UNION SELECT 'x'",
        "SELECT ssn FROM users UNION DISTINCT SELECT 'x'",
        "SELECT 'x' UNION ALL SELECT email FROM users UNION SELECT ssn FROM users",
        "SELECT ssn FROM users INTERSECT SELECT email FROM users",
        "SELECT ssn FROM users INTERSECT ALL SELECT email FROM users",
        "SELECT ssn FROM users EXCEPT ALL SELECT email FROM users",
    ] {
        let lineage = pg(sql).expect(sql);
        let mut expected = vec![r("users.ssn", Role::Distinct)];
        if sql.contains("email") {
            expected.insert(0, r("users.email", Role::Distinct));
        }
        assert_eq!(refs(&lineage), expected, "{sql}");
    }
    let lineage = pg("SELECT ssn FROM users UNION ALL SELECT email FROM users").expect("analyze");
    assert_eq!(refs(&lineage), Vec::new());
    let lineage = pg("SELECT ssn FROM users UNION ALL (SELECT email FROM users UNION SELECT 'x')").expect("analyze");
    assert_eq!(refs(&lineage), vec![r("users.email", Role::Distinct)]);
}

#[test]
fn set_returning_function_arguments_are_generator_references() {
    let lineage =
        pg("SELECT generate_series(1, ascii(ssn)) AS n, string_to_table(email, ',') AS e FROM users").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("n", &["users.ssn"]), col("e", &["users.email"])]
    );
    assert_eq!(
        refs(&lineage),
        vec![r("users.email", Role::Generator), r("users.ssn", Role::Generator)]
    );
    let lineage = on("SELECT json_each(ssn) FROM users", Dialect::Sqlite).expect("analyze");
    assert_eq!(refs(&lineage), vec![r("users.ssn", Role::Generator)]);
    let lineage = pg("SELECT lower(ssn) FROM users").expect("analyze");
    assert_eq!(refs(&lineage), Vec::new());
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
    let lineage = pg("SELECT email FROM users UNION ALL SELECT ssn FROM users ORDER BY 1").expect("analyze");
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
        ("SELECT * FROM users TABLESAMPLE BERNOULLI (10)", Dialect::Postgres),
        ("SELECT * FROM generate_series(1, 3)", Dialect::Postgres),
        ("SELECT * FROM UNNEST(ARRAY[1, 2])", Dialect::Postgres),
        ("SELECT * FROM users, LATERAL (SELECT 1) l", Dialect::Postgres),
        ("WITH RECURSIVE r AS (SELECT 1) SELECT * FROM r", Dialect::Postgres),
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
    // Unoptimized aarch64-apple-darwin frames overflow the 2 MiB default
    // test thread here; a release build fits.
    let (lineage, result) = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || (pg(&nested(20)), pg(&nested(25))))
        .expect("spawn")
        .join()
        .expect("no stack overflow");
    let lineage = lineage.expect("analyze");
    assert_eq!(columns(&lineage), vec![col("id", &["users.id"])]);
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
fn view_over_an_open_view_still_ends_in_missing() {
    let catalog = with_view(fixture(), None, "v1", &["id"], "SELECT id FROM v2");
    let catalog = with_view(catalog, Some("v1"), "v2", &["id"], "SELECT * FROM m1");
    assert_eq!(
        analyze(
            "SELECT v1.id FROM v1 JOIN m2 ON v1.id = m2.k",
            Dialect::Postgres,
            &catalog
        ),
        Err(LineageError::Missing(vec![
            WrittenName {
                parts: vec![NamePart::bare("m1")],
                context: Origin::View(id("v2")),
            },
            WrittenName::query(vec![NamePart::bare("m2")]),
        ]))
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

/// The fixture plus `archive` (shaped like `users`) and vacant `t2`, `v2`.
fn create_fixture() -> Catalog {
    let mut catalog = fixture();
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("archive")]),
        Entry::Resolved(relation(
            "archive",
            RelationKind::Table,
            &["id", "email", "ssn", "~note"],
        )),
    );
    for name in ["t2", "v2"] {
        catalog.insert(WrittenName::query(vec![NamePart::bare(name)]), Entry::Vacant(id(name)));
    }
    catalog
}

/// Renders writes as `(target column, sources)`.
fn writes(lineage: &Lineage) -> Vec<(String, Vec<String>)> {
    lineage
        .writes
        .iter()
        .map(|w| (w.name.clone(), names(&w.sources)))
        .collect()
}

/// Analyzes against [`create_fixture`].
fn on(sql: &str, dialect: Dialect) -> Result<Lineage, LineageError> {
    analyze(sql, dialect, &create_fixture())
}

/// Returns the target relation's name.
fn target(lineage: &Lineage) -> Option<&str> {
    lineage.target.as_ref().map(|t| t.name.as_str())
}

/// Renders touched relations as sorted `(name, kind)` pairs.
fn relations(lineage: &Lineage) -> Vec<(String, RelationKind)> {
    lineage.relations.iter().map(|r| (r.id.name.clone(), r.kind)).collect()
}

/// Shorthand for an `EXPLAIN` statement kind of [`ExplainForm::Other`].
fn explain(inner: StatementKind) -> StatementKind {
    StatementKind::Explain(ExplainForm::Other, Box::new(inner))
}

#[test]
fn relations_only_statements_record_target_without_columns() {
    let cases: &[(&str, Dialect, StatementKind, &str)] = &[
        (
            "CREATE TABLE t2 (id INT)",
            Dialect::Postgres,
            StatementKind::CreateTable,
            "t2",
        ),
        ("DROP TABLE users", Dialect::Postgres, StatementKind::Drop, "users"),
        ("TRUNCATE users", Dialect::Postgres, StatementKind::Truncate, "users"),
        (
            "CREATE INDEX i ON users (email)",
            Dialect::Postgres,
            StatementKind::CreateIndex,
            "users",
        ),
        ("DROP INDEX i ON users", Dialect::Mysql, StatementKind::Drop, "users"),
    ];
    for (sql, dialect, kind, name) in cases {
        let lineage = on(sql, *dialect).expect(sql);
        assert_eq!(&lineage.statement, kind, "{sql}");
        assert_eq!(target(&lineage), Some(*name), "{sql}");
        assert_eq!(
            lineage.relations.iter().map(|r| r.id.name.as_str()).collect::<Vec<_>>(),
            vec![*name],
            "{sql}"
        );
        assert!(lineage.columns.is_empty() && lineage.writes.is_empty(), "{sql}");
        assert!(lineage.references.is_empty(), "{sql}");
    }
}

#[test]
fn create_table_claims_a_vacant_name() {
    let lineage = on("CREATE TABLE t2 (id INT)", Dialect::Postgres).expect("analyze");
    assert_eq!(lineage.statement, StatementKind::CreateTable);
    assert_eq!(target(&lineage), Some("t2"));
    assert_eq!(relations(&lineage), vec![("t2".to_string(), RelationKind::Table)]);
}

#[test]
fn create_table_target_goes_through_the_missing_loop() {
    let result = analyze("CREATE TABLE t2 (id INT)", Dialect::Postgres, &Catalog::new());
    assert_eq!(
        result,
        Err(LineageError::Missing(vec![WrittenName::query(vec![NamePart::bare(
            "t2"
        )])]))
    );
}

#[test]
fn create_table_as_writes_new_columns() {
    let lineage = on(
        "CREATE TABLE t2 AS SELECT ssn, email AS mail, 1 AS one FROM users",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(lineage.statement, StatementKind::CreateTableAs);
    assert_eq!(target(&lineage), Some("t2"));
    assert_eq!(
        writes(&lineage),
        vec![
            col("ssn", &["users.ssn"]),
            col("mail", &["users.email"]),
            col("one", &[])
        ]
    );
    assert!(lineage.columns.is_empty());
    assert!(relations(&lineage).contains(&("t2".to_string(), RelationKind::Table)));
}

#[test]
fn postgres_select_into_is_a_create_table_as() {
    let lineage = on("SELECT ssn INTO t2 FROM users", Dialect::Postgres).expect("analyze");
    assert_eq!(lineage.statement, StatementKind::CreateTableAs);
    assert_eq!(target(&lineage), Some("t2"));
    assert_eq!(writes(&lineage), vec![col("ssn", &["users.ssn"])]);
    assert!(lineage.columns.is_empty());

    assert_unsupported("SELECT ssn INTO @x FROM users", Dialect::Mysql, &create_fixture());
}

#[test]
fn select_into_outside_the_top_level_select_is_unsupported() {
    for sql in [
        "SELECT ssn INTO t2 FROM users UNION SELECT email FROM users",
        "SELECT ssn INTO TEMP t2 FROM users",
        "SELECT * FROM (SELECT ssn INTO t2 FROM users) s",
    ] {
        assert_unsupported(sql, Dialect::Postgres, &create_fixture());
    }
}

#[test]
fn create_view_body_feeds_view_columns() {
    let lineage = on("CREATE VIEW v2 (a) AS SELECT ssn FROM users", Dialect::Postgres).expect("analyze");
    assert_eq!(lineage.statement, StatementKind::CreateView);
    assert_eq!(target(&lineage), Some("v2"));
    assert_eq!(writes(&lineage), vec![col("a", &["users.ssn"])]);
    assert!(relations(&lineage).contains(&("v2".to_string(), RelationKind::View)));

    let lineage = on(
        "CREATE MATERIALIZED VIEW v2 AS SELECT email FROM users",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(lineage.statement, StatementKind::CreateMaterializedView);
    assert_eq!(writes(&lineage), vec![col("email", &["users.email"])]);
    assert!(relations(&lineage).contains(&("v2".to_string(), RelationKind::MatView)));
}

#[test]
fn create_target_goes_through_the_missing_loop() {
    let result = analyze(
        "CREATE TABLE t2 AS SELECT id FROM users",
        Dialect::Postgres,
        &Catalog::new(),
    );
    let Err(LineageError::Missing(missing)) = result else {
        panic!("expected Missing, got {result:?}");
    };
    let missing: Vec<String> = missing.iter().map(ToString::to_string).collect();
    assert_eq!(missing, vec!["t2", "users"]);
}

#[test]
fn ctas_star_writes_only_star_visible_columns() {
    let lineage = on("CREATE TABLE t2 AS SELECT * FROM archive", Dialect::Postgres).expect("analyze");
    assert_eq!(
        writes(&lineage),
        vec![
            col("id", &["archive.id"]),
            col("email", &["archive.email"]),
            col("ssn", &["archive.ssn"])
        ]
    );
    let lineage = on("SELECT ssn INTO UNLOGGED t2 FROM users", Dialect::Postgres).expect("unlogged is permanent");
    assert_eq!(lineage.statement, StatementKind::CreateTableAs);
}

#[test]
fn ctas_with_column_definitions_is_unsupported() {
    let result = analyze(
        "CREATE TABLE t2 (uid INT) AS SELECT id FROM users",
        Dialect::Postgres,
        &Catalog::new(),
    );
    assert!(matches!(result, Err(LineageError::Unsupported(_))), "{result:?}");
    for dialect in [Dialect::Postgres, Dialect::Mysql] {
        assert_unsupported(
            "CREATE TABLE t2 (uid INT, mail TEXT) AS SELECT id, email FROM users",
            dialect,
            &create_fixture(),
        );
    }
}

#[test]
fn create_checks_guard_ctas_and_views() {
    let cases: &[(&str, Dialect)] = &[
        (
            "CREATE TABLE t2 ENGINE=FEDERATED AS SELECT ssn FROM users",
            Dialect::Mysql,
        ),
        ("CREATE TEMPORARY TABLE t2 AS SELECT ssn FROM users", Dialect::Postgres),
        (
            "CREATE ALGORITHM=MERGE VIEW v2 AS SELECT ssn FROM users",
            Dialect::Mysql,
        ),
        ("CREATE TEMPORARY VIEW v2 AS SELECT ssn FROM users", Dialect::Postgres),
    ];
    for (sql, dialect) in cases {
        assert_unsupported(sql, *dialect, &create_fixture());
    }
}

#[test]
fn vacant_name_is_unknown_outside_a_create() {
    for sql in ["SELECT * FROM t2", "DROP TABLE t2", "INSERT INTO t2 VALUES (1)"] {
        let result = on(sql, Dialect::Postgres);
        assert!(
            matches!(result, Err(LineageError::UnknownRelation { .. })),
            "{sql}: {result:?}"
        );
    }
}

#[test]
fn drop_view_does_not_expand_the_body() {
    let catalog = with_view(fixture(), None, "v", &["a"], "SELECT nope FROM nowhere");
    let lineage = analyze("DROP VIEW v", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(target(&lineage), Some("v"));
    assert_eq!(relations(&lineage), vec![("v".to_string(), RelationKind::View)]);
}

#[test]
fn drop_if_exists_of_vacant_name_has_no_target() {
    let mut catalog = fixture();
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("gone")]),
        Entry::Vacant(id("gone")),
    );
    catalog.insert(WrittenName::query(vec![NamePart::bare("ghost")]), Entry::Absent);
    for sql in ["DROP TABLE IF EXISTS gone", "TRUNCATE IF EXISTS gone"] {
        let lineage = analyze(sql, Dialect::Postgres, &catalog).expect(sql);
        assert_eq!(lineage.target, None, "{sql}");
        assert!(lineage.relations.is_empty(), "{sql}");
    }
    for sql in [
        "DROP TABLE gone",
        "DROP TABLE IF EXISTS ghost",
        "TRUNCATE IF EXISTS ghost",
    ] {
        let result = analyze(sql, Dialect::Postgres, &catalog);
        assert!(
            matches!(result, Err(LineageError::UnknownRelation { .. })),
            "{sql}: {result:?}"
        );
    }
}

#[test]
fn mysql_engines_serving_other_rows_are_unsupported() {
    for sql in [
        "CREATE TABLE t2 (id INT) ENGINE=MERGE UNION=(users)",
        "CREATE TABLE t2 (id INT) ENGINE=FEDERATED CONNECTION='mysql://h/db/users'",
    ] {
        assert_unsupported(sql, Dialect::Mysql, &create_fixture());
    }
    let lineage = analyze(
        "CREATE TABLE t2 (id INT) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
        Dialect::Mysql,
        &create_fixture(),
    )
    .expect("analyze");
    assert_eq!(lineage.statement, StatementKind::CreateTable);
}

#[test]
fn drop_cascade_is_unsupported_and_drop_temporary_is_a_drop() {
    assert_unsupported("DROP TABLE users CASCADE", Dialect::Postgres, &fixture());
    let lineage = analyze("DROP TEMPORARY TABLE users", Dialect::Mysql, &fixture()).expect("analyze");
    assert_eq!(lineage.statement, StatementKind::Drop);
    assert_eq!(target(&lineage), Some("users"));
}

#[test]
fn ddl_expressions_and_value_constraints_are_unsupported() {
    let cases: &[(&str, Dialect)] = &[
        ("CREATE INDEX i ON users ((lower(email)))", Dialect::Postgres),
        (
            "CREATE INDEX i ON users (email) WHERE ssn IS NOT NULL",
            Dialect::Postgres,
        ),
        ("CREATE TABLE t2 (id INT DEFAULT 0)", Dialect::Postgres),
        (
            "CREATE TABLE t2 (id INT, d INT GENERATED ALWAYS AS (id * 2) STORED)",
            Dialect::Postgres,
        ),
        ("CREATE TABLE t2 (id INT CHECK (id > 0))", Dialect::Postgres),
        ("CREATE TABLE t2 (id INT, CHECK (id > 0))", Dialect::Postgres),
        ("CREATE TABLE t2 (id INT REFERENCES users (id))", Dialect::Postgres),
        (
            "CREATE TABLE t2 (id INT, FOREIGN KEY (id) REFERENCES users (id))",
            Dialect::Postgres,
        ),
    ];
    for (sql, dialect) in cases {
        assert_unsupported(sql, *dialect, &create_fixture());
    }
}

#[test]
fn ddl_with_column_names_only_is_modeled() {
    let cases: &[(&str, StatementKind)] = &[
        (
            "CREATE INDEX i ON users (email) INCLUDE (ssn)",
            StatementKind::CreateIndex,
        ),
        ("CREATE TABLE t2 (id INT PRIMARY KEY)", StatementKind::CreateTable),
        ("CREATE TABLE t2 (id INT, PRIMARY KEY (id))", StatementKind::CreateTable),
        (
            "CREATE TABLE t2 (email TEXT, UNIQUE (email))",
            StatementKind::CreateTable,
        ),
    ];
    for (sql, kind) in cases {
        let lineage = on(sql, Dialect::Postgres).expect(sql);
        assert_eq!(&lineage.statement, kind, "{sql}");
    }
}

#[test]
fn show_and_describe_have_no_sources() {
    let cases: &[(&str, Dialect, StatementKind)] = &[
        ("SHOW TABLES", Dialect::Mysql, StatementKind::Show(Show::Tables)),
        ("SHOW WARNINGS", Dialect::Mysql, StatementKind::Show(Show::Warnings)),
        (
            "SHOW PROCESSLIST",
            Dialect::Mysql,
            StatementKind::Show(Show::Unsupported),
        ),
        (
            "SHOW search_path",
            Dialect::Postgres,
            StatementKind::Show(Show::Unsupported),
        ),
        ("DESCRIBE users", Dialect::Mysql, StatementKind::Describe),
    ];
    for (sql, dialect, kind) in cases {
        let lineage = on(sql, *dialect).expect(sql);
        assert_eq!(&lineage.statement, kind, "{sql}");
        assert!(lineage.columns.is_empty() && lineage.relations.is_empty(), "{sql}");
        assert_eq!(lineage.target, None, "{sql}");
    }
}

#[test]
fn show_quoted_words_are_not_keywords() {
    let cases: &[(&str, Dialect)] = &[
        ("SHOW `warnings`", Dialect::Mysql),
        ("SHOW \"ALL\"", Dialect::Postgres),
        ("SHOW `ſtatus`", Dialect::Mysql),
    ];
    for (sql, dialect) in cases {
        let lineage = on(sql, *dialect).expect(sql);
        assert_eq!(lineage.statement, StatementKind::Show(Show::Unsupported), "{sql}");
    }
    assert_unsupported("SHOW \"WARNINGS\"", Dialect::Mysql, &fixture());
}

#[test]
fn show_filter_expressions_are_unsupported() {
    let fixture = create_fixture();
    let cases: &[(&str, Dialect)] = &[
        ("SHOW DATABASES WHERE `Database` = 'shop'", Dialect::Mysql),
        (
            "SHOW TABLES WHERE (SELECT ssn FROM users LIMIT 1) LIKE '1%'",
            Dialect::Mysql,
        ),
        (
            "SHOW FULL COLUMNS FROM users WHERE Field IN (SELECT ssn FROM users)",
            Dialect::Mariadb,
        ),
        ("SHOW TABLE STATUS where Name <=> 'users'", Dialect::Mysql),
        (
            "SHOW GLOBAL VARIABLES /* c */ WHERE Variable_name IN ('a')",
            Dialect::Mysql,
        ),
        ("EXPLAIN SHOW STATUS WHERE 1", Dialect::Mysql),
        ("SHOW TABLE STATUS LIKE (SELECT ssn FROM users)", Dialect::Mysql),
        ("SHOW OPEN TABLES LIKE N'u%'", Dialect::Mysql),
    ];
    for (sql, dialect) in cases {
        assert_unsupported(sql, *dialect, &fixture);
    }
    let literal: &[(&str, Show)] = &[
        ("SHOW TABLES LIKE 'u%'", Show::Tables),
        ("SHOW VARIABLES LIKE \"max%\"", Show::Variables),
        ("SHOW COLUMNS FROM `where` LIKE 'a'", Show::Columns),
        ("SHOW TABLE STATUS FROM shop", Show::TableStatus),
    ];
    for (sql, form) in literal {
        let lineage = on(sql, Dialect::Mysql).expect(sql);
        assert_eq!(lineage.statement, StatementKind::Show(*form), "{sql}");
    }
}

#[test]
fn show_forms_classify_by_leading_keywords() {
    let my = Dialect::Mysql;
    let maria = Dialect::Mariadb;
    let cases: &[(&str, Dialect, Show)] = &[
        // Allowed forms, each synonym and modifier position.
        ("SHOW DATABASES", my, Show::Databases),
        ("SHOW SCHEMAS LIKE 'a%'", maria, Show::Databases),
        ("SHOW TABLES", my, Show::Tables),
        ("SHOW FULL TABLES FROM d", my, Show::Tables),
        ("SHOW EXTENDED TABLES", my, Show::Tables),
        ("SHOW EXTENDED FULL TABLES", my, Show::Tables),
        ("SHOW OPEN TABLES", my, Show::OpenTables),
        ("SHOW TABLE STATUS", maria, Show::TableStatus),
        ("SHOW COLUMNS FROM users", my, Show::Columns),
        ("SHOW FIELDS FROM users", my, Show::Columns),
        ("SHOW FULL COLUMNS FROM users", my, Show::Columns),
        ("SHOW EXTENDED COLUMNS FROM users", my, Show::Columns),
        ("SHOW EXTENDED FULL FIELDS FROM users", my, Show::Columns),
        ("SHOW INDEX FROM users", my, Show::Index),
        ("SHOW INDEXES FROM users", my, Show::Index),
        ("SHOW KEYS FROM users", maria, Show::Index),
        ("SHOW EXTENDED INDEX FROM users", my, Show::Index),
        ("SHOW CREATE TABLE users", my, Show::CreateTable),
        ("SHOW CREATE VIEW v", my, Show::CreateView),
        ("SHOW CREATE TRIGGER t", my, Show::CreateTrigger),
        ("SHOW CREATE FUNCTION f", my, Show::CreateFunction),
        ("SHOW CREATE PROCEDURE p", my, Show::CreateProcedure),
        ("SHOW CREATE EVENT e", my, Show::CreateEvent),
        ("SHOW TRIGGERS", my, Show::Triggers),
        ("SHOW EVENTS", maria, Show::Events),
        ("SHOW FUNCTION STATUS", my, Show::FunctionStatus),
        ("SHOW PROCEDURE STATUS", my, Show::ProcedureStatus),
        ("SHOW FUNCTION CODE f", maria, Show::FunctionCode),
        ("SHOW PROCEDURE CODE p", maria, Show::ProcedureCode),
        ("SHOW VARIABLES", my, Show::Variables),
        ("SHOW GLOBAL VARIABLES", my, Show::Variables),
        ("SHOW SESSION VARIABLES", my, Show::Variables),
        ("SHOW STATUS", my, Show::Status),
        ("SHOW GLOBAL STATUS", my, Show::Status),
        ("SHOW SESSION STATUS", maria, Show::Status),
        ("SHOW CHARSET", my, Show::Charset),
        ("SHOW CHARACTER SET", my, Show::Charset),
        ("SHOW COLLATION", my, Show::Collation),
        ("SHOW ENGINES", my, Show::Engines),
        ("SHOW STORAGE ENGINES", maria, Show::Engines),
        ("SHOW PLUGINS", my, Show::Plugins),
        ("SHOW PRIVILEGES", my, Show::Privileges),
        ("SHOW WARNINGS", my, Show::Warnings),
        ("SHOW COUNT(*) WARNINGS", my, Show::Warnings),
        ("SHOW ERRORS", my, Show::Errors),
        ("SHOW COUNT(*) ERRORS", maria, Show::Errors),
        // Denied forms.
        ("SHOW PROCESSLIST", my, Show::Unsupported),
        ("SHOW FULL PROCESSLIST", maria, Show::Unsupported),
        ("SHOW ENGINE INNODB STATUS", my, Show::Unsupported),
        ("SHOW SLAVE STATUS", maria, Show::Unsupported),
        ("SHOW ALL SLAVES STATUS", maria, Show::Unsupported),
        ("SHOW REPLICA STATUS", my, Show::Unsupported),
        ("SHOW REPLICAS", my, Show::Unsupported),
        ("SHOW MASTER STATUS", my, Show::Unsupported),
        ("SHOW BINLOG EVENTS", my, Show::Unsupported),
        ("SHOW RELAYLOG EVENTS", my, Show::Unsupported),
        ("SHOW GRANTS", maria, Show::Unsupported),
        ("SHOW PROFILES", my, Show::Unsupported),
        ("SHOW USER_VARIABLES", maria, Show::Unsupported),
        ("SHOW EXPLAIN FOR 1", maria, Show::Unsupported),
        ("SHOW ANALYZE FOR 1", maria, Show::Unsupported),
        // Modifiers outside their positions.
        ("SHOW TERSE STATUS", my, Show::Unsupported),
        ("SHOW GLOBAL TABLE STATUS", my, Show::Unsupported),
        ("SHOW FULL FULL TABLES", my, Show::Unsupported),
        ("SHOW LOCAL VARIABLES", my, Show::Unsupported),
        ("SHOW LOCAL STATUS", my, Show::Unsupported),
        ("SHOW EXTERNAL TABLES", my, Show::Unsupported),
        ("SHOW TABLE foo", my, Show::Unsupported),
        ("SHOW FUNCTION xyz", my, Show::Unsupported),
        // Quoted words are not keywords.
        ("SHOW `TABLES`", my, Show::Unsupported),
        ("SHOW `WARNINGS`", my, Show::Unsupported),
        ("SHOW `user_variables`", maria, Show::Unsupported),
        // Comments and whitespace.
        ("SHOW /*x*/ WARNINGS", my, Show::Warnings),
        ("SHOW\nWARNINGS", my, Show::Warnings),
        ("SHOW COUNT ( * ) ERRORS", my, Show::Errors),
        ("show count(*) errors", my, Show::Errors),
        // Trailing tokens never change the class.
        ("SHOW INDEX FROM errors", my, Show::Index),
        ("SHOW VARIABLES LIKE 'warnings'", my, Show::Variables),
        ("SHOW TABLE STATUS FROM warnings", my, Show::TableStatus),
        ("SHOW WARNINGS_X", my, Show::Unsupported),
        // One table for every dialect.
        ("SHOW TABLES", Dialect::Postgres, Show::Tables),
        ("SHOW search_path", Dialect::Postgres, Show::Unsupported),
        ("SHOW ALL", Dialect::Postgres, Show::Unsupported),
    ];
    for (sql, dialect, form) in cases {
        assert_eq!(Show::classify(sql, *dialect), *form, "{sql}");
        let explained = format!("EXPLAIN {sql}");
        assert_eq!(Show::classify(&explained, *dialect), *form, "{explained}");
        // Forms sqlparser rejects never reach the classifier.
        if let Ok(lineage) = on(sql, *dialect) {
            assert_eq!(lineage.statement, StatementKind::Show(*form), "{sql}");
        }
    }
}

#[test]
fn show_classifies_from_tokens() {
    let cases: &[(&str, Dialect, Show)] = &[
        ("EXPLAIN SHOW WARNINGS", Dialect::Mysql, Show::Warnings),
        ("SELECT 1", Dialect::Mysql, Show::Unsupported),
        ("SHOW 'unterminated", Dialect::Mysql, Show::Unsupported),
        ("SHOW", Dialect::Mysql, Show::Unsupported),
    ];
    for (sql, dialect, form) in cases {
        assert_eq!(Show::classify(sql, *dialect), *form, "{sql}");
    }
}

#[test]
fn explain_carries_inner_lineage() {
    let lineage = on("EXPLAIN SELECT ssn FROM users", Dialect::Postgres).expect("analyze");
    assert_eq!(lineage.statement, explain(StatementKind::Query));
    assert_eq!(columns(&lineage), vec![col("ssn", &["users.ssn"])]);

    let lineage = on("EXPLAIN DROP TABLE users", Dialect::Postgres).expect("analyze");
    assert_eq!(lineage.statement, explain(StatementKind::Drop));
    assert_eq!(target(&lineage), Some("users"));
}

#[test]
fn explain_forms_the_explain_tools_build_wrap_a_query() {
    let cases: &[(&str, Dialect, ExplainForm)] = &[
        (
            "EXPLAIN (ANALYZE, FORMAT JSON) SELECT ssn FROM users",
            Dialect::Postgres,
            ExplainForm::Analyze,
        ),
        (
            "EXPLAIN (FORMAT JSON) SELECT ssn FROM users",
            Dialect::Postgres,
            ExplainForm::Json,
        ),
        (
            "EXPLAIN ANALYZE SELECT ssn FROM users",
            Dialect::Mysql,
            ExplainForm::Analyze,
        ),
        (
            "EXPLAIN FORMAT=JSON SELECT ssn FROM users",
            Dialect::Mysql,
            ExplainForm::Json,
        ),
        (
            "EXPLAIN QUERY PLAN SELECT ssn FROM users",
            Dialect::Sqlite,
            ExplainForm::QueryPlan,
        ),
    ];
    for (sql, dialect, form) in cases {
        let lineage = on(sql, *dialect).expect(sql);
        assert_eq!(
            lineage.statement,
            StatementKind::Explain(*form, Box::new(StatementKind::Query)),
            "{sql}"
        );
        assert_eq!(columns(&lineage), vec![col("ssn", &["users.ssn"])], "{sql}");
    }
    let lineage = on("EXPLAIN SHOW TABLES", Dialect::Mysql).expect("analyze");
    assert_eq!(lineage.statement, explain(StatementKind::Show(Show::Tables)));
}

#[test]
fn explain_forms_are_classified_strictly() {
    let cases: &[(&str, Dialect, ExplainForm)] = &[
        ("EXPLAIN (FORMAT JSON) SELECT 1", Dialect::Postgres, ExplainForm::Json),
        ("EXPLAIN (format json) SELECT 1", Dialect::Postgres, ExplainForm::Json),
        (
            "EXPLAIN (ANALYZE, FORMAT JSON) SELECT 1",
            Dialect::Postgres,
            ExplainForm::Analyze,
        ),
        (
            r#"EXPLAIN ("analyze", FORMAT JSON) SELECT 1"#,
            Dialect::Postgres,
            ExplainForm::Analyze,
        ),
        (
            "EXPLAIN (ANALYZE off, FORMAT JSON) SELECT 1",
            Dialect::Postgres,
            ExplainForm::Analyze,
        ),
        (
            "EXPLAIN (FORMAT JSON, ANALYZE 0) SELECT 1",
            Dialect::Postgres,
            ExplainForm::Analyze,
        ),
        ("EXPLAIN ANALYZE SELECT 1", Dialect::Postgres, ExplainForm::Analyze),
        (
            "EXPLAIN (VERBOSE, FORMAT JSON) SELECT 1",
            Dialect::Postgres,
            ExplainForm::Other,
        ),
        (
            "EXPLAIN (COSTS false, FORMAT JSON) SELECT 1",
            Dialect::Postgres,
            ExplainForm::Other,
        ),
        ("EXPLAIN (FORMAT TEXT) SELECT 1", Dialect::Postgres, ExplainForm::Other),
        (
            "EXPLAIN (FORMAT 'json') SELECT 1",
            Dialect::Postgres,
            ExplainForm::Other,
        ),
        ("EXPLAIN SELECT 1", Dialect::Postgres, ExplainForm::Other),
        ("EXPLAIN VERBOSE SELECT 1", Dialect::Postgres, ExplainForm::Other),
        ("EXPLAIN FORMAT=JSON SELECT 1", Dialect::Mysql, ExplainForm::Json),
        ("EXPLAIN FORMAT JSON SELECT 1", Dialect::Mysql, ExplainForm::Json),
        ("EXPLAIN FORMAT=JSON SELECT 1", Dialect::Mariadb, ExplainForm::Json),
        ("EXPLAIN ANALYZE SELECT 1", Dialect::Mysql, ExplainForm::Analyze),
        (
            "EXPLAIN ANALYZE FORMAT=JSON SELECT 1",
            Dialect::Mysql,
            ExplainForm::Analyze,
        ),
        ("EXPLAIN FORMAT=TREE SELECT 1", Dialect::Mysql, ExplainForm::Other),
        (
            "EXPLAIN VERBOSE FORMAT=JSON SELECT 1",
            Dialect::Mysql,
            ExplainForm::Other,
        ),
        ("DESCRIBE FORMAT=JSON SELECT 1", Dialect::Mysql, ExplainForm::Other),
        ("EXPLAIN SELECT 1", Dialect::Mysql, ExplainForm::Other),
        ("EXPLAIN QUERY PLAN SELECT 1", Dialect::Mysql, ExplainForm::Other),
        ("EXPLAIN QUERY PLAN SELECT 1", Dialect::Sqlite, ExplainForm::QueryPlan),
        ("EXPLAIN SELECT 1", Dialect::Sqlite, ExplainForm::Other),
    ];
    for (sql, dialect, form) in cases {
        let lineage = on(sql, *dialect).expect(sql);
        assert_eq!(
            lineage.statement,
            StatementKind::Explain(*form, Box::new(StatementKind::Query)),
            "{sql} on {dialect:?}"
        );
    }
}

#[test]
fn unmodeled_statements_are_unsupported() {
    let fixture = create_fixture();
    let cases: &[(&str, Dialect)] = &[
        ("ALTER TABLE users ADD COLUMN x INT", Dialect::Postgres),
        ("ALTER TABLE users DROP COLUMN ssn", Dialect::Mysql),
        ("RENAME TABLE users TO people", Dialect::Mysql),
        (
            "CREATE TRIGGER t AFTER INSERT ON users FOR EACH ROW EXECUTE FUNCTION f()",
            Dialect::Postgres,
        ),
        (
            "CREATE FUNCTION f() RETURNS INT LANGUAGE SQL AS 'SELECT 1'",
            Dialect::Postgres,
        ),
        ("CREATE PROCEDURE p() AS BEGIN SELECT 1; END", Dialect::Mysql),
        ("CREATE POLICY p ON users USING (true)", Dialect::Postgres),
        ("COPY users TO STDOUT", Dialect::Postgres),
        ("GRANT SELECT ON users TO bob", Dialect::Postgres),
        ("REVOKE SELECT ON users FROM bob", Dialect::Postgres),
        ("ATTACH DATABASE 'x.db' AS x", Dialect::Sqlite),
        ("PRAGMA journal_mode = 'WAL'", Dialect::Sqlite),
        ("PRAGMA table_info('users')", Dialect::Sqlite),
        (
            "WITH x AS (SELECT 1) INSERT INTO users (id) SELECT * FROM x",
            Dialect::Postgres,
        ),
        ("CREATE TEMPORARY TABLE t2 AS SELECT ssn FROM users", Dialect::Postgres),
        ("CREATE TEMPORARY TABLE t2 (id INT)", Dialect::Postgres),
        ("DROP TABLE users, orders", Dialect::Postgres),
        ("DROP INDEX i1, i2", Dialect::Postgres),
        ("TRUNCATE users, orders", Dialect::Postgres),
        ("CREATE TABLE t2 (LIKE users)", Dialect::Postgres),
        ("USE app", Dialect::Mysql),
        ("DROP SCHEMA public CASCADE", Dialect::Postgres),
        ("DROP DATABASE app", Dialect::Mysql),
        ("TRUNCATE users CASCADE", Dialect::Postgres),
        ("CREATE RULE r AS ON INSERT TO users DO NOTHING", Dialect::Postgres),
        ("SELECT id FROM ONLY users", Dialect::Postgres),
        ("SELECT id FROM users *", Dialect::Postgres),
        ("SELECT id FROM users WHERE id IN orders", Dialect::Sqlite),
        ("SELECT id FROM users JOIN orders USING (id) AS j", Dialect::Postgres),
        (
            "INSERT INTO users (id) OVERRIDING SYSTEM VALUE VALUES (1)",
            Dialect::Postgres,
        ),
    ];
    for (sql, dialect) in cases {
        assert_unsupported(sql, *dialect, &fixture);
    }
}

#[test]
fn statements_the_parser_rejects_fail_closed() {
    for (sql, dialect) in [
        ("LOAD DATA INFILE 'x' INTO TABLE users", Dialect::Mysql),
        (
            "CREATE EVENT e ON SCHEDULE EVERY 1 HOUR DO DELETE FROM users",
            Dialect::Mysql,
        ),
    ] {
        let result = on(sql, dialect);
        assert!(matches!(result, Err(LineageError::Parse(_))), "{sql}: {result:?}");
    }
}

/// Inserts a table under a multi-part written name with an explicit identity.
fn insert_qualified(catalog: &mut Catalog, parts: &[&str], id: RelationId, cols: &[&str]) {
    catalog.insert(
        WrittenName::query(parts.iter().map(|p| NamePart::bare(*p)).collect()),
        Entry::Resolved(Relation::new(
            id,
            RelationKind::Table,
            cols.iter().map(|c| CatalogColumn::new(*c)).collect(),
        )),
    );
}

#[test]
fn literal_projection_has_no_sources() {
    let lineage = analyze("SELECT 1 AS n", Dialect::Postgres, &Catalog::new()).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("n", &[])]);
    assert!(lineage.relations.is_empty());
}

#[test]
fn schema_qualified_column_binds_through_qualified_table() {
    let mut catalog = Catalog::new();
    insert_qualified(
        &mut catalog,
        &["analytics", "events"],
        RelationId::new("db", "analytics", "events"),
        &["id"],
    );
    let lineage = analyze(
        "SELECT analytics.events.id FROM analytics.events",
        Dialect::Postgres,
        &catalog,
    )
    .expect("analyze");
    assert_eq!(columns(&lineage), vec![col("id", &["events.id"])]);
    let source = lineage.columns[0].sources.first().expect("source");
    assert_eq!(source.relation.schema, "analytics");
}

#[test]
fn three_part_column_binds_through_three_part_table() {
    let mut catalog = Catalog::new();
    insert_qualified(
        &mut catalog,
        &["db", "sch", "t"],
        RelationId::new("db", "sch", "t"),
        &["id"],
    );
    let lineage = analyze("SELECT db.sch.t.id FROM db.sch.t", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("id", &["t.id"])]);
}

#[test]
fn postgres_folds_unquoted_alias_and_column() {
    let mut catalog = Catalog::new();
    insert_qualified(
        &mut catalog,
        &["Users"],
        RelationId::new("db", "public", "users"),
        &["email"],
    );
    let lineage = analyze("SELECT U.Email FROM Users U", Dialect::Postgres, &catalog).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("email", &["users.email"])]);
    let lineage = analyze("SELECT U.Email FROM Users u", Dialect::Postgres, &catalog).expect("alias folds");
    assert_eq!(columns(&lineage), vec![col("email", &["users.email"])]);
}

#[test]
fn binary_operator_unions_operand_sources() {
    let lineage = pg("SELECT email || ' ' || ssn AS full FROM users").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("full", &["users.email", "users.ssn"])]);
}

#[test]
fn case_results_and_condition_are_projection_sources() {
    let lineage = pg("SELECT CASE WHEN id > 0 THEN email ELSE ssn END AS c FROM users").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("c", &["users.email", "users.id", "users.ssn"])]
    );
}

#[test]
fn postgres_json_operators_carry_the_json_column() {
    for sql in [
        "SELECT email->>'ssn' AS v FROM users",
        "SELECT email->'a'->>'b' AS v FROM users",
        "SELECT email#>>'{a,b}' AS v FROM users",
        "SELECT email#>'{a}' AS v FROM users",
    ] {
        let lineage = pg(sql).expect(sql);
        assert_eq!(columns(&lineage), vec![col("v", &["users.email"])], "{sql}");
    }
}

#[test]
fn mysql_json_extract_carries_the_json_column() {
    for sql in [
        "SELECT JSON_EXTRACT(email, '$.ssn') AS v FROM users",
        "SELECT email->>'$.ssn' AS v FROM users",
        "SELECT email->'$.ssn' AS v FROM users",
    ] {
        for dialect in [Dialect::Mysql, Dialect::Mariadb] {
            let lineage = analyze(sql, dialect, &fixture()).expect(sql);
            assert_eq!(
                columns(&lineage),
                vec![col("v", &["users.email"])],
                "{sql} on {dialect:?}"
            );
        }
    }
}

#[test]
fn cte_reading_an_earlier_cte_traces_to_base_table() {
    let lineage =
        pg("WITH a AS (SELECT email FROM users), b AS (SELECT email FROM a) SELECT email FROM b").expect("analyze");
    assert_eq!(columns(&lineage), vec![col("email", &["users.email"])]);
}

#[test]
fn non_recursive_cte_forward_reference_is_a_catalog_name() {
    let catalog = catalog(&[]);
    let result = analyze(
        "WITH c1 AS (SELECT x FROM c2), c2 AS (SELECT x FROM c1) SELECT x FROM c1",
        Dialect::Postgres,
        &catalog,
    );
    let Err(LineageError::Missing(names)) = result else {
        panic!("expected Missing, got {result:?}");
    };
    assert_eq!(names, vec![WrittenName::query(vec![NamePart::bare("c2")])]);
}

#[test]
fn star_over_derived_table_expands_its_columns() {
    let lineage = pg("SELECT * FROM (SELECT id, email FROM users) t").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("id", &["users.id"]), col("email", &["users.email"])]
    );
}

#[test]
fn qualified_star_over_cte_expands_its_columns() {
    let lineage = pg("WITH c AS (SELECT id, email FROM users) SELECT c.* FROM c").expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("id", &["users.id"]), col("email", &["users.email"])]
    );
}

#[test]
fn insert_values_maps_listed_columns_positionally() {
    let lineage = on(
        "INSERT INTO archive (ssn, email) VALUES ((SELECT ssn FROM users WHERE id = 1), DEFAULT), ('x', 'y')",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(lineage.statement, StatementKind::Insert);
    assert_eq!(target(&lineage), Some("archive"));
    assert_eq!(writes(&lineage), vec![col("ssn", &["users.ssn"]), col("email", &[])]);
    assert_eq!(refs(&lineage), vec![r("users.id", Role::Filter)]);
    assert!(lineage.columns.is_empty());
}

#[test]
fn insert_select_star_without_column_list_maps_by_catalog_order() {
    let lineage = on(
        "INSERT INTO archive SELECT * FROM users WHERE id > 1",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(
        writes(&lineage),
        vec![
            col("id", &["users.id"]),
            col("email", &["users.email"]),
            col("ssn", &["users.ssn"])
        ]
    );
    assert_eq!(refs(&lineage), vec![r("users.id", Role::Filter)]);
    assert_eq!(
        relations(&lineage),
        vec![
            ("archive".to_string(), RelationKind::Table),
            ("users".to_string(), RelationKind::Table)
        ]
    );
}

#[test]
fn insert_value_count_must_match_target_columns() {
    let result = on("INSERT INTO archive SELECT id, email FROM users", Dialect::Postgres);
    assert!(
        matches!(result, Err(LineageError::ColumnCountMismatch { .. })),
        "{result:?}"
    );
}

#[test]
fn insert_returning_is_the_output() {
    let lineage = on(
        "INSERT INTO archive AS a (id) VALUES (1) RETURNING a.ssn, id",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(
        columns(&lineage),
        vec![col("ssn", &["archive.ssn"]), col("id", &["archive.id"])]
    );
}

#[test]
fn insert_on_conflict_excluded_carries_inserted_sources() {
    let lineage = on(
        "INSERT INTO users (id, ssn) SELECT user_id, total FROM orders \
         ON CONFLICT (id) DO UPDATE SET email = excluded.ssn WHERE users.id > 0",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(
        writes(&lineage),
        vec![
            col("id", &["orders.user_id"]),
            col("ssn", &["orders.total"]),
            col("email", &["orders.total"])
        ]
    );
    assert_eq!(refs(&lineage), vec![r("users.id", Role::Filter)]);
}

#[test]
fn mysql_insert_set_and_on_duplicate_key_update_write_target_columns() {
    let lineage = on(
        "INSERT INTO users SET email = 'x' ON DUPLICATE KEY UPDATE ssn = email",
        Dialect::Mysql,
    )
    .expect("analyze");
    assert_eq!(target(&lineage), Some("users"));
    assert_eq!(writes(&lineage), vec![col("email", &[]), col("ssn", &["users.email"])]);
}

#[test]
fn update_set_projects_into_target_and_where_filters() {
    let lineage = on("UPDATE users SET email = ssn WHERE id = 1", Dialect::Postgres).expect("analyze");
    assert_eq!(lineage.statement, StatementKind::Update);
    assert_eq!(target(&lineage), Some("users"));
    assert_eq!(writes(&lineage), vec![col("email", &["users.ssn"])]);
    assert_eq!(refs(&lineage), vec![r("users.id", Role::Filter)]);
    assert!(lineage.columns.is_empty());
}

#[test]
fn update_returning_output_column_is_sourced() {
    let lineage = on("UPDATE users SET email = 'x' RETURNING ssn", Dialect::Postgres).expect("analyze");
    assert_eq!(columns(&lineage), vec![col("ssn", &["users.ssn"])]);
}

#[test]
fn postgres_update_from_binds_set_to_target_only() {
    let lineage = on(
        "UPDATE users SET id = o.total FROM orders o WHERE o.user_id = users.id",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(target(&lineage), Some("users"));
    assert_eq!(writes(&lineage), vec![col("id", &["orders.total"])]);
    assert_eq!(
        refs(&lineage),
        vec![r("orders.user_id", Role::Filter), r("users.id", Role::Filter)]
    );
}

#[test]
fn update_tuple_takes_subquery_columns_by_position() {
    let lineage = on(
        "UPDATE users SET (email, ssn) = (SELECT total, user_id FROM orders WHERE orders.user_id = users.id)",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(
        writes(&lineage),
        vec![col("email", &["orders.total"]), col("ssn", &["orders.user_id"])]
    );
}

#[test]
fn mysql_multi_table_update_writes_the_assigned_table() {
    let lineage = on(
        "UPDATE users u JOIN orders o ON o.user_id = u.id SET u.email = o.total WHERE o.total > 0",
        Dialect::Mysql,
    )
    .expect("analyze");
    assert_eq!(target(&lineage), Some("users"));
    assert_eq!(writes(&lineage), vec![col("email", &["orders.total"])]);
    assert_eq!(
        refs(&lineage),
        vec![
            r("orders.total", Role::Filter),
            r("orders.user_id", Role::Join),
            r("users.id", Role::Join)
        ]
    );

    let result = on(
        "UPDATE users u JOIN orders o ON o.user_id = u.id SET u.email = 1, o.total = 2",
        Dialect::Mysql,
    );
    assert!(matches!(result, Err(LineageError::Unsupported(_))), "{result:?}");
}

#[test]
fn delete_using_filters_and_returns() {
    let lineage = on(
        "DELETE FROM users USING orders WHERE orders.user_id = users.id RETURNING users.ssn",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(lineage.statement, StatementKind::Delete);
    assert_eq!(target(&lineage), Some("users"));
    assert!(lineage.writes.is_empty());
    assert_eq!(
        refs(&lineage),
        vec![r("orders.user_id", Role::Filter), r("users.id", Role::Filter)]
    );
    assert_eq!(columns(&lineage), vec![col("ssn", &["users.ssn"])]);
}

#[test]
fn mysql_multi_table_delete_targets_named_table() {
    for sql in [
        "DELETE u FROM users u JOIN orders o ON o.user_id = u.id WHERE o.total > 0",
        "DELETE FROM users USING users JOIN orders ON orders.user_id = users.id WHERE orders.total > 0",
    ] {
        let lineage = on(sql, Dialect::Mysql).expect(sql);
        assert_eq!(target(&lineage), Some("users"), "{sql}");
        assert_eq!(
            refs(&lineage),
            vec![
                r("orders.total", Role::Filter),
                r("orders.user_id", Role::Join),
                r("users.id", Role::Join)
            ],
            "{sql}"
        );
    }
}

#[test]
fn merge_on_filters_and_actions_write() {
    let lineage = on(
        "MERGE INTO users u USING orders o ON u.id = o.user_id \
         WHEN MATCHED AND o.total > 0 THEN UPDATE SET email = o.total \
         WHEN NOT MATCHED THEN INSERT (id, ssn) VALUES (o.user_id, o.total)",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(lineage.statement, StatementKind::Merge);
    assert_eq!(target(&lineage), Some("users"));
    assert_eq!(
        writes(&lineage),
        vec![
            col("email", &["orders.total"]),
            col("id", &["orders.user_id"]),
            col("ssn", &["orders.total"])
        ]
    );
    assert_eq!(
        refs(&lineage),
        vec![
            r("orders.total", Role::Filter),
            r("orders.user_id", Role::Filter),
            r("users.id", Role::Filter)
        ]
    );
}

#[test]
fn writes_through_views_and_derived_tables_are_rejected() {
    let catalog = with_view(fixture(), None, "v", &["a"], "SELECT ssn FROM users");
    for sql in [
        "INSERT INTO v VALUES (1)",
        "UPDATE v SET a = 1",
        "DELETE FROM v",
        "MERGE INTO v USING orders o ON true WHEN MATCHED THEN DELETE",
        "UPDATE (SELECT id FROM users) d SET id = 1",
    ] {
        assert_unsupported(sql, Dialect::Postgres, &catalog);
    }
}

#[test]
fn insert_column_list_shorter_than_select_errors() {
    let result = pg("INSERT INTO users (id) SELECT id, user_id FROM orders");
    assert!(
        matches!(result, Err(LineageError::ColumnCountMismatch { .. })),
        "{result:?}"
    );
}

#[test]
fn data_modifying_statements_inside_queries_are_unsupported() {
    for sql in [
        "WITH x AS (DELETE FROM users RETURNING ssn) INSERT INTO archive (ssn) SELECT ssn FROM x",
        "INSERT INTO archive (ssn) WITH x AS (DELETE FROM users RETURNING ssn) SELECT ssn FROM x",
        "WITH x AS (SELECT 1) UPDATE users SET id = 1",
    ] {
        assert_unsupported(sql, Dialect::Postgres, &create_fixture());
    }
}

#[test]
fn values_in_on_duplicate_key_update_is_unsupported() {
    for (sql, dialect) in [
        (
            "INSERT INTO users (id) VALUES (1) ON DUPLICATE KEY UPDATE ssn = VALUES(email)",
            Dialect::Mysql,
        ),
        (
            "INSERT INTO users (id) VALUES (1) ON DUPLICATE KEY UPDATE ssn = VALUE(email)",
            Dialect::Mariadb,
        ),
        (
            "INSERT INTO users (id) VALUES (1) ON DUPLICATE KEY UPDATE ssn = (SELECT VALUES(email))",
            Dialect::Mysql,
        ),
    ] {
        assert_unsupported(sql, dialect, &create_fixture());
    }
}

#[test]
fn merge_returning_is_unsupported() {
    assert_unsupported(
        "MERGE INTO users u USING orders o ON u.id = o.user_id WHEN MATCHED THEN DELETE RETURNING u.ssn",
        Dialect::Postgres,
        &create_fixture(),
    );
}

#[test]
fn write_targets_bind_strictly() {
    let cases: &[(&str, Dialect, &str)] = &[
        (
            "UPDATE users SET email = ssn RETURNING old.email",
            Dialect::Postgres,
            "column",
        ),
        ("UPDATE users SET email.x = 1", Dialect::Postgres, "unknown"),
        ("UPDATE users u SET x.email = 1", Dialect::Postgres, "unknown"),
        (
            "UPDATE users u JOIN orders o ON o.user_id = u.id SET id = 1",
            Dialect::Mysql,
            "ambiguous",
        ),
        ("DELETE x FROM users", Dialect::Mysql, "unknown"),
    ];
    for (sql, dialect, kind) in cases {
        let result = on(sql, *dialect);
        let ok = match *kind {
            "column" => matches!(result, Err(LineageError::ColumnNotFound { .. })),
            "ambiguous" => matches!(result, Err(LineageError::AmbiguousColumn { .. })),
            _ => matches!(result, Err(LineageError::UnknownRelation { .. })),
        };
        assert!(ok, "{sql}: {result:?}");
    }
}

#[test]
fn an_open_write_target_defers_to_missing() {
    let result = on("INSERT INTO nope (a) SELECT id, email FROM users", Dialect::Postgres);
    assert_eq!(
        result,
        Err(LineageError::Missing(vec![WrittenName::query(vec![NamePart::bare(
            "nope"
        )])]))
    );
}

#[test]
fn dml_order_by_and_limit_are_recorded() {
    let lineage = on("UPDATE users SET email = 'x' ORDER BY ssn LIMIT 1", Dialect::Mysql).expect("analyze");
    assert_eq!(refs(&lineage), vec![r("users.ssn", Role::Sort)]);
}

#[test]
fn excluded_column_not_inserted_has_no_sources() {
    let lineage = on(
        "INSERT INTO users (id) VALUES (1) ON CONFLICT (id) DO UPDATE SET ssn = excluded.email",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(writes(&lineage), vec![col("id", &[]), col("ssn", &[])]);
}

#[test]
fn qualified_insert_column_binds_like_set() {
    let lineage = on("INSERT INTO users (users.email) VALUES ('x')", Dialect::Mysql).expect("analyze");
    assert_eq!(writes(&lineage), vec![col("email", &[])]);
}

#[test]
fn explain_of_an_update_carries_its_writes() {
    let lineage = on("EXPLAIN UPDATE users SET email = ssn", Dialect::Postgres).expect("analyze");
    assert_eq!(lineage.statement, explain(StatementKind::Update));
    assert_eq!(writes(&lineage), vec![col("email", &["users.ssn"])]);
}

/// Renders writes as `(target column, named)`.
fn named(lineage: &Lineage) -> Vec<(String, bool)> {
    lineage.writes.iter().map(|w| (w.name.clone(), w.named)).collect()
}

#[test]
fn create_columns_without_a_kept_name_are_unnamed() {
    let lineage = on(
        "CREATE TABLE t2 AS SELECT ssn, email AS mail, upper(email), 1 FROM users",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(
        named(&lineage),
        vec![
            ("ssn".to_string(), true),
            ("mail".to_string(), true),
            ("upper(email)".to_string(), false),
            ("1".to_string(), false),
        ]
    );
    let lineage = on(
        "CREATE VIEW v2 (a, b) AS SELECT upper(ssn), email FROM users",
        Dialect::Postgres,
    )
    .expect("analyze");
    assert_eq!(named(&lineage), vec![("a".to_string(), true), ("b".to_string(), true)]);
    let lineage = on("CREATE TABLE t2 AS SELECT * FROM users", Dialect::Postgres).expect("analyze");
    assert!(lineage.writes.iter().all(|w| w.named));
}

#[test]
fn dml_targets_are_always_named() {
    for sql in [
        "INSERT INTO archive (ssn) SELECT upper(email) FROM users",
        "UPDATE users SET email = upper(ssn)",
    ] {
        let lineage = on(sql, Dialect::Postgres).expect(sql);
        assert!(lineage.writes.iter().all(|w| w.named), "{sql}");
    }
}

#[test]
fn created_columns_sharing_a_name_are_unsupported() {
    for (sql, dialect) in [
        (
            "CREATE TABLE t2 AS SELECT email AS ssn, ssn FROM users",
            Dialect::Sqlite,
        ),
        ("CREATE TABLE t2 AS SELECT id, id FROM users", Dialect::Postgres),
        (
            "CREATE VIEW v2 AS SELECT email AS X, ssn AS x FROM users",
            Dialect::Mysql,
        ),
    ] {
        assert_unsupported(sql, dialect, &create_fixture());
    }
}

#[test]
fn ctas_with_constraints_is_unsupported() {
    assert_unsupported(
        "CREATE TABLE t2 (UNIQUE (ssn)) AS SELECT ssn FROM users",
        Dialect::Mysql,
        &create_fixture(),
    );
}

// Flow: how each value reaches its column.

/// Analyzes `sql` against [`create_fixture`] in `dialect` and returns the output flows.
fn flows_of(sql: &str, dialect: Dialect) -> Vec<Flow> {
    flows(&on(sql, dialect).expect(sql))
}

/// Analyzes `sql` against [`create_fixture`] in `dialect` and returns each write's `(column, flow)`.
fn write_flows(sql: &str, dialect: Dialect) -> Vec<(String, Flow)> {
    on(sql, dialect)
        .expect(sql)
        .writes
        .into_iter()
        .map(|w| (w.name, w.flow))
        .collect()
}

/// Every modeled dialect.
const DIALECTS: [Dialect; 4] = [Dialect::Postgres, Dialect::Mysql, Dialect::Mariadb, Dialect::Sqlite];

#[test]
fn column_references_are_bare() {
    for dialect in DIALECTS {
        let quoted = if matches!(dialect, Dialect::Mysql | Dialect::Mariadb) {
            "`email`"
        } else {
            "\"email\""
        };
        let sql = format!("SELECT email, email AS e, (email), ((u.email)), {quoted}, u.email FROM users u");
        assert_eq!(flows_of(&sql, dialect), [Flow::Bare; 6], "{dialect:?}");
        assert_eq!(
            flows_of("SELECT users.email FROM users", dialect),
            [Flow::Bare],
            "{dialect:?}"
        );
    }
}

#[test]
fn passthrough_relations_keep_the_flow() {
    let catalog = users_in_view(
        with_view(
            fixture(),
            None,
            "v",
            &["mail", "low"],
            "SELECT email, lower(ssn) FROM users",
        ),
        "v",
    );
    let lineage = analyze("SELECT mail, low FROM v", Dialect::Postgres, &catalog).expect("view");
    assert_eq!(flows(&lineage), [Flow::Bare, Flow::Derived]);
    let lineage = analyze("SELECT * FROM v", Dialect::Postgres, &catalog).expect("view star");
    assert_eq!(flows(&lineage), [Flow::Bare, Flow::Derived]);
    for dialect in DIALECTS {
        let cases = [
            "WITH c AS (SELECT email, lower(ssn) AS s FROM users) SELECT email, s FROM c",
            "WITH c (m, s) AS (SELECT email, lower(ssn) FROM users) SELECT m, s FROM c",
            "SELECT d.m, d.s FROM (SELECT email AS m, lower(ssn) AS s FROM users) d",
            "SELECT * FROM (SELECT email, lower(ssn) AS s FROM users) d",
            "SELECT d.* FROM (SELECT email, lower(ssn) AS s FROM users) d",
        ];
        for sql in cases {
            assert_eq!(
                flows_of(sql, dialect),
                [Flow::Bare, Flow::Derived],
                "{dialect:?}: {sql}"
            );
        }
        assert_eq!(flows_of("SELECT * FROM users", dialect), [Flow::Bare; 3], "{dialect:?}");
        assert_eq!(
            flows_of("SELECT u.* FROM users u", dialect),
            [Flow::Bare; 3],
            "{dialect:?}"
        );
    }
}

#[test]
fn distinct_keeps_the_flow() {
    for dialect in DIALECTS {
        assert_eq!(flows_of("SELECT DISTINCT email FROM users", dialect), [Flow::Bare]);
    }
    assert_eq!(
        flows_of("SELECT DISTINCT ON (id) email FROM users", Dialect::Postgres),
        [Flow::Bare]
    );
}

#[test]
fn functions_aggregates_and_windows_are_derived() {
    for dialect in DIALECTS {
        let sql = "SELECT lower(email), min(email), max(email), count(DISTINCT email), \
                   first_value(email) OVER (ORDER BY id), CASE WHEN email IS NULL THEN 'x' ELSE 'x' END, \
                   COALESCE(email, ssn), email || ssn FROM users";
        assert_eq!(flows_of(sql, dialect), [Flow::Derived; 8], "{dialect:?}");
    }
    assert_eq!(
        flows_of("SELECT IF(email IS NULL, 'x', 'x') FROM users", Dialect::Mysql),
        [Flow::Derived]
    );
    assert_eq!(
        flows_of("SELECT iif(email IS NULL, 'x', 'x') FROM users", Dialect::Sqlite),
        [Flow::Derived]
    );
}

#[test]
fn casts_and_collations_are_derived() {
    assert_eq!(
        flows_of(
            "SELECT CAST(email AS text), email::text, email COLLATE \"C\" FROM users",
            Dialect::Postgres
        ),
        [Flow::Derived; 3]
    );
    for dialect in [Dialect::Mysql, Dialect::Mariadb] {
        assert_eq!(
            flows_of(
                "SELECT CAST(email AS CHAR), CONVERT(email, CHAR), CONVERT(email USING utf8mb4), \
                 email COLLATE utf8mb4_bin FROM users",
                dialect
            ),
            [Flow::Derived; 4],
            "{dialect:?}"
        );
    }
}

#[test]
fn json_access_subqueries_and_whole_rows_are_derived() {
    assert_eq!(
        flows_of(
            "SELECT u.email -> 'a', (SELECT u2.email FROM users u2 LIMIT 1), users, u FROM users, users u",
            Dialect::Postgres
        ),
        [Flow::Derived; 4]
    );
    assert_eq!(
        flows_of("SELECT row_to_json(u.*) FROM users u", Dialect::Postgres),
        [Flow::Derived]
    );
}

#[test]
fn constants_and_parameters_are_derived() {
    assert_eq!(
        flows_of("SELECT 'a@b.c', 1, $1, TRUE", Dialect::Postgres),
        [Flow::Derived; 4]
    );
}

#[test]
fn null_literals_under_parentheses_and_casts_are_null() {
    assert_eq!(
        flows_of(
            "SELECT NULL, (NULL), CAST(NULL AS text), NULL::text, (NULL)::text, COALESCE(NULL), NULLIF(NULL, 1)",
            Dialect::Postgres
        ),
        [
            Flow::Null,
            Flow::Null,
            Flow::Null,
            Flow::Null,
            Flow::Null,
            Flow::Derived,
            Flow::Derived
        ]
    );
    for dialect in [Dialect::Mysql, Dialect::Mariadb] {
        assert_eq!(
            flows_of("SELECT CONVERT(NULL, CHAR), CONVERT(NULL USING utf8mb4)", dialect),
            [Flow::Null; 2],
            "{dialect:?}"
        );
    }
}

#[test]
fn set_operations_merge_flows_per_position() {
    for dialect in DIALECTS {
        let cases = [
            ("SELECT email FROM users UNION SELECT ssn FROM users", Flow::Bare),
            ("SELECT email FROM users UNION ALL SELECT NULL", Flow::Bare),
            ("SELECT NULL UNION SELECT email FROM users", Flow::Bare),
            ("SELECT NULL UNION SELECT NULL", Flow::Null),
            ("SELECT email FROM users UNION SELECT 'a@b.c'", Flow::Derived),
            (
                "SELECT email FROM users UNION SELECT lower(ssn) FROM users",
                Flow::Derived,
            ),
            (
                "SELECT email FROM users UNION SELECT ssn FROM users UNION SELECT 'x'",
                Flow::Derived,
            ),
        ];
        for (sql, flow) in cases {
            assert_eq!(flows_of(sql, dialect), [flow], "{dialect:?}: {sql}");
        }
    }
}

#[test]
fn values_rows_merge_flows_per_position() {
    assert_eq!(
        flows_of("SELECT * FROM (VALUES (NULL, 1), (NULL, NULL)) v", Dialect::Postgres),
        [Flow::Null, Flow::Derived]
    );
    let writes = write_flows(
        "INSERT INTO archive (email, ssn, id) VALUES (DEFAULT, NULL, DEFAULT), (DEFAULT, DEFAULT, NULL)",
        // `NULL` is the identity, so `NULL` + `DEFAULT` stays `Default`.
        Dialect::Postgres,
    );
    assert_eq!(
        writes,
        [
            ("email".to_string(), Flow::Default),
            ("ssn".to_string(), Flow::Default),
            ("id".to_string(), Flow::Default),
        ]
    );
}

#[test]
fn insert_sources_carry_their_flows() {
    for dialect in DIALECTS {
        assert_eq!(
            write_flows(
                "INSERT INTO archive (email, ssn) SELECT email, lower(ssn) FROM users",
                dialect
            ),
            [("email".to_string(), Flow::Bare), ("ssn".to_string(), Flow::Derived)],
            "{dialect:?}"
        );
        assert_eq!(
            write_flows(
                "INSERT INTO archive (email, ssn, id) VALUES (NULL, DEFAULT, 1)",
                dialect
            ),
            [
                ("email".to_string(), Flow::Null),
                ("ssn".to_string(), Flow::Default),
                ("id".to_string(), Flow::Derived),
            ],
            "{dialect:?}"
        );
    }
}

#[test]
fn update_assignments_carry_their_flows() {
    for dialect in DIALECTS {
        assert_eq!(
            write_flows("UPDATE users SET email = ssn, id = DEFAULT, ssn = (email)", dialect),
            [
                ("email".to_string(), Flow::Bare),
                ("id".to_string(), Flow::Default),
                ("ssn".to_string(), Flow::Bare),
            ],
            "{dialect:?}"
        );
    }
    assert_eq!(
        write_flows(
            "UPDATE users SET (email, ssn) = (SELECT o.total, o.total + 1 FROM orders o)",
            Dialect::Postgres
        ),
        [("email".to_string(), Flow::Bare), ("ssn".to_string(), Flow::Derived)]
    );
}

#[test]
fn merge_clauses_merge_flows_per_column() {
    let writes = write_flows(
        "MERGE INTO users u USING orders o ON u.id = o.user_id \
         WHEN MATCHED THEN UPDATE SET email = o.total, ssn = o.total \
         WHEN NOT MATCHED THEN INSERT (id, email, ssn) VALUES (o.user_id, NULL, o.total + 1)",
        Dialect::Postgres,
    );
    assert_eq!(
        writes,
        [
            ("email".to_string(), Flow::Bare),
            ("ssn".to_string(), Flow::Derived),
            ("id".to_string(), Flow::Bare),
        ]
    );
}

#[test]
fn using_keys_merge_both_sides_flows() {
    for dialect in DIALECTS {
        assert_eq!(
            flows_of("SELECT id FROM users JOIN orders USING (id)", dialect),
            [Flow::Bare],
            "{dialect:?}"
        );
    }
    assert_eq!(
        flows_of(
            "SELECT id FROM (SELECT id + 1 AS id FROM users) d FULL JOIN orders USING (id)",
            Dialect::Postgres
        ),
        [Flow::Derived]
    );
}

#[test]
fn on_conflict_excluded_carries_the_inserted_flow() {
    let writes = write_flows(
        "INSERT INTO users (id, ssn) SELECT user_id, total + 1 FROM orders \
         ON CONFLICT (id) DO UPDATE SET email = excluded.email",
        Dialect::Postgres,
    );
    assert_eq!(
        writes,
        [
            ("id".to_string(), Flow::Bare),
            ("ssn".to_string(), Flow::Derived),
            ("email".to_string(), Flow::Derived),
        ]
    );
    // An `excluded` column the `INSERT` writes as `DEFAULT` is server-filled too.
    let writes = write_flows(
        "INSERT INTO users (id, email) VALUES (1, DEFAULT) \
         ON CONFLICT (id) DO UPDATE SET email = excluded.email",
        Dialect::Postgres,
    );
    assert_eq!(
        writes,
        [("id".to_string(), Flow::Derived), ("email".to_string(), Flow::Derived)]
    );
    let writes = write_flows(
        "INSERT INTO users (id, ssn) SELECT user_id, total + 1 FROM orders \
         ON CONFLICT (id) DO UPDATE SET email = excluded.id, id = excluded.ssn",
        Dialect::Postgres,
    );
    assert_eq!(
        writes,
        [
            ("id".to_string(), Flow::Derived),
            ("ssn".to_string(), Flow::Derived),
            ("email".to_string(), Flow::Bare),
        ]
    );
}

#[test]
fn on_duplicate_key_update_merges_into_the_inserted_flow() {
    for dialect in [Dialect::Mysql, Dialect::Mariadb] {
        assert_eq!(
            write_flows(
                "INSERT INTO users (email) VALUES (NULL) ON DUPLICATE KEY UPDATE email = ssn",
                dialect
            ),
            [("email".to_string(), Flow::Bare)],
            "{dialect:?}"
        );
        assert_eq!(
            write_flows(
                "INSERT INTO users SET email = 'x' ON DUPLICATE KEY UPDATE ssn = email",
                dialect
            ),
            [("email".to_string(), Flow::Derived), ("ssn".to_string(), Flow::Bare)],
            "{dialect:?}"
        );
    }
}

#[test]
fn created_columns_carry_the_query_flow() {
    for dialect in DIALECTS {
        assert_eq!(
            write_flows("CREATE TABLE t2 AS SELECT email, lower(ssn) AS s FROM users", dialect),
            [("email".to_string(), Flow::Bare), ("s".to_string(), Flow::Derived)],
            "{dialect:?}"
        );
    }
}

#[test]
fn outer_join_padding_stays_bare() {
    for dialect in DIALECTS {
        assert_eq!(
            flows_of(
                "SELECT u.email FROM orders o LEFT JOIN users u ON u.id = o.user_id",
                dialect
            ),
            [Flow::Bare],
            "{dialect:?}"
        );
    }
}

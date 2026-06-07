//! Acceptance corpus for lineage extraction, expressed as data-driven cases.

use std::collections::BTreeSet;

use dbmcp_sql_lineage::{Lineage, LineageError, SourceRef, TableRef, extract};
use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};

/// One expected output column: its name and exact dotted source columns.
#[derive(Debug, Clone, Copy)]
struct Col {
    /// Expected output name (`None` for an unnamed expression).
    name: Option<&'static str>,
    /// Expected dotted `[db.][schema.]table.column` sources.
    sources: &'static [&'static str],
}

/// One successful-extraction expectation.
#[derive(Clone, Copy)]
struct Case {
    /// Human label, used in assertion messages.
    name: &'static str,
    /// SQL to extract.
    sql: &'static str,
    /// Dialect to parse with.
    dialect: &'static dyn Dialect,
    /// Expected write target (`None` for a read query).
    target: Option<&'static str>,
    /// Expected output columns, in order.
    cols: &'static [Col],
    /// Dotted table paths that must appear in the source `tables` set.
    tables: &'static [&'static str],
}

/// One fail-closed expectation.
struct ErrCase {
    /// Human label, used in assertion messages.
    name: &'static str,
    /// SQL to extract.
    sql: &'static str,
    /// Dialect to parse with.
    dialect: &'static dyn Dialect,
    /// The error variant the input must produce (payload ignored).
    expected: LineageError,
}

/// Asserts the lineage's write target matches the expectation.
fn assert_target(case: &str, lineage: &Lineage, expected: Option<&str>) {
    let got = lineage.target.as_ref().map(table_str);
    assert_eq!(got.as_deref(), expected, "{case}: target mismatch");
}

/// Asserts the output column at `pos` has the expected name and sources.
fn assert_col(case: &str, lineage: &Lineage, pos: usize, expected: &Col) {
    let col = &lineage.columns[pos];
    assert_eq!(col.name.as_deref(), expected.name, "{case}: name mismatch at {pos}");
    let want: BTreeSet<String> = expected.sources.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(srcs(&col.sources), want, "{case}: sources mismatch at {pos}");
}

/// Asserts each expected dotted table path is present in the source set.
///
/// Membership uses [`TableRef`]'s case-insensitive equality.
fn assert_tables(case: &str, lineage: &Lineage, expected: &[&str]) {
    for t in expected {
        let want = parse_table(t);
        assert!(
            lineage.tables.contains(&want),
            "{case}: missing source table `{t}` in {:?}",
            lineage.tables
        );
    }
}

/// Parses a dotted `[db.][schema.]name` path into a [`TableRef`].
fn parse_table(dotted: &str) -> TableRef {
    let parts: Vec<&str> = dotted.split('.').collect();
    match parts.as_slice() {
        [name] => TableRef::new(*name),
        [schema, name] => TableRef::qualified(*schema, *name),
        [.., database, schema, name] => TableRef::builder(*name)
            .with_schema(*schema)
            .with_database(*database)
            .build(),
        [] => TableRef::new(""),
    }
}

/// Builds the set of dotted `[db.][schema.]table.column` strings for sources.
fn srcs(sources: &BTreeSet<SourceRef>) -> BTreeSet<String> {
    sources
        .iter()
        .map(|s| format!("{}.{}", table_str(&s.table), s.column))
        .collect()
}

/// Renders a table reference as its dotted `[db.][schema.]name` path.
fn table_str(t: &TableRef) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(d) = &t.database {
        parts.push(d);
    }
    if let Some(s) = &t.schema {
        parts.push(s);
    }
    parts.push(&t.name);
    parts.join(".")
}

#[test]
#[allow(clippy::too_many_lines)]
fn extract_succeeds() {
    let cases: &'static [Case] = &[
        Case {
            name: "single_table_qualified",
            sql: "SELECT u.id, u.email FROM users u",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[
                Col {
                    name: Some("id"),
                    sources: &["users.id"],
                },
                Col {
                    name: Some("email"),
                    sources: &["users.email"],
                },
            ],
            tables: &["users"],
        },
        Case {
            name: "alias_output_name",
            sql: "SELECT email AS contact FROM users",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("contact"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "literal_has_no_sources",
            sql: "SELECT 1 AS n",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("n"),
                sources: &[],
            }],
            tables: &[],
        },
        Case {
            name: "join_with_aliases",
            sql: "SELECT u.id, o.total FROM users u JOIN orders o ON o.user_id = u.id",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[
                Col {
                    name: Some("id"),
                    sources: &["users.id"],
                },
                Col {
                    name: Some("total"),
                    sources: &["orders.total"],
                },
            ],
            tables: &["users", "orders"],
        },
        Case {
            name: "unqualified_single_table",
            sql: "SELECT email FROM users",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("email"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "schema_qualified_table",
            sql: "SELECT analytics.events.id FROM analytics.events",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("id"),
                sources: &["analytics.events.id"],
            }],
            tables: &["analytics.events"],
        },
        Case {
            name: "multipart_three_level_table",
            sql: "SELECT db.sch.t.id FROM db.sch.t",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("id"),
                sources: &["db.sch.t.id"],
            }],
            tables: &["db.sch.t"],
        },
        Case {
            name: "case_insensitive_table",
            sql: "SELECT U.Email FROM Users U",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("Email"),
                sources: &["Users.Email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "dialect_matrix_pg",
            sql: "SELECT u.email FROM users u",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("email"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "dialect_matrix_mysql",
            sql: "SELECT u.email FROM users u",
            dialect: &MySqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("email"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "dialect_matrix_sqlite",
            sql: "SELECT u.email FROM users u",
            dialect: &SQLiteDialect {},
            target: None,
            cols: &[Col {
                name: Some("email"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "aggregate",
            sql: "SELECT department, MAX(salary) AS top FROM employees GROUP BY department",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[
                Col {
                    name: Some("department"),
                    sources: &["employees.department"],
                },
                Col {
                    name: Some("top"),
                    sources: &["employees.salary"],
                },
            ],
            tables: &["employees"],
        },
        Case {
            name: "concat_expression",
            sql: "SELECT first_name || ' ' || last_name AS full FROM users",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("full"),
                sources: &["users.first_name", "users.last_name"],
            }],
            tables: &["users"],
        },
        Case {
            name: "case_expression",
            sql: "SELECT CASE WHEN active THEN email ELSE name END AS c FROM users",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("c"),
                sources: &["users.active", "users.email", "users.name"],
            }],
            tables: &["users"],
        },
        Case {
            name: "window_function",
            sql: "SELECT ROW_NUMBER() OVER (PARTITION BY dept ORDER BY hired) AS rn FROM staff",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("rn"),
                sources: &["staff.dept", "staff.hired"],
            }],
            tables: &["staff"],
        },
        Case {
            name: "json_arrow_operator",
            sql: "SELECT data->>'ssn' AS ssn FROM accounts",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("ssn"),
                sources: &["accounts.data"],
            }],
            tables: &["accounts"],
        },
        Case {
            name: "json_extract_function",
            sql: "SELECT JSON_EXTRACT(data, '$.ssn') AS ssn FROM accounts",
            dialect: &MySqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("ssn"),
                sources: &["accounts.data"],
            }],
            tables: &["accounts"],
        },
        Case {
            name: "json_hash_arrow",
            sql: "SELECT data#>>'{a,b}' AS v FROM accounts",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("v"),
                sources: &["accounts.data"],
            }],
            tables: &["accounts"],
        },
        Case {
            name: "union_per_position",
            sql: "SELECT a.x FROM t a UNION SELECT b.x FROM s b",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("x"),
                sources: &["t.x", "s.x"],
            }],
            tables: &["t", "s"],
        },
        Case {
            name: "derived_table",
            sql: "SELECT t.email FROM (SELECT email FROM users) t",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("email"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "scalar_subquery_in_projection",
            sql: "SELECT (SELECT MAX(amount) FROM orders) AS top FROM users",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("top"),
                sources: &["orders.amount"],
            }],
            tables: &["orders"],
        },
        Case {
            name: "cte_traces_to_base_table",
            sql: "WITH a AS (SELECT email FROM users) SELECT email FROM a",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("email"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "nested_cte",
            sql: "WITH a AS (SELECT email FROM users), b AS (SELECT email FROM a) SELECT email FROM b",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[Col {
                name: Some("email"),
                sources: &["users.email"],
            }],
            tables: &["users"],
        },
        Case {
            name: "wildcard_over_derived_table",
            sql: "SELECT * FROM (SELECT id, email FROM users) t",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[
                Col {
                    name: Some("id"),
                    sources: &["users.id"],
                },
                Col {
                    name: Some("email"),
                    sources: &["users.email"],
                },
            ],
            tables: &["users"],
        },
        Case {
            name: "qualified_wildcard_over_cte",
            sql: "WITH a AS (SELECT id, email FROM users) SELECT a.* FROM a",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[
                Col {
                    name: Some("id"),
                    sources: &["users.id"],
                },
                Col {
                    name: Some("email"),
                    sources: &["users.email"],
                },
            ],
            tables: &["users"],
        },
        Case {
            name: "insert_select_explicit_columns",
            sql: "INSERT INTO tgt (a, b) SELECT x, y FROM s",
            dialect: &PostgreSqlDialect {},
            target: Some("tgt"),
            cols: &[
                Col {
                    name: Some("a"),
                    sources: &["s.x"],
                },
                Col {
                    name: Some("b"),
                    sources: &["s.y"],
                },
            ],
            tables: &["s"],
        },
        Case {
            name: "insert_select_no_columns_keeps_names",
            sql: "INSERT INTO tgt SELECT id, email FROM users",
            dialect: &PostgreSqlDialect {},
            target: Some("tgt"),
            cols: &[
                Col {
                    name: Some("id"),
                    sources: &["users.id"],
                },
                Col {
                    name: Some("email"),
                    sources: &["users.email"],
                },
            ],
            tables: &["users"],
        },
        Case {
            name: "insert_values_literals_empty_sources",
            sql: "INSERT INTO tgt (a, b) VALUES (1, 'x')",
            dialect: &PostgreSqlDialect {},
            target: Some("tgt"),
            cols: &[
                Col {
                    name: Some("a"),
                    sources: &[],
                },
                Col {
                    name: Some("b"),
                    sources: &[],
                },
            ],
            tables: &[],
        },
        Case {
            name: "insert_set_mysql",
            sql: "INSERT INTO tgt SET a = 1, b = 2",
            dialect: &MySqlDialect {},
            target: Some("tgt"),
            cols: &[
                Col {
                    name: Some("a"),
                    sources: &[],
                },
                Col {
                    name: Some("b"),
                    sources: &[],
                },
            ],
            tables: &[],
        },
        Case {
            name: "ctas_basic",
            sql: "CREATE TABLE rollup AS SELECT u.id, u.email FROM users u",
            dialect: &PostgreSqlDialect {},
            target: Some("rollup"),
            cols: &[
                Col {
                    name: Some("id"),
                    sources: &["users.id"],
                },
                Col {
                    name: Some("email"),
                    sources: &["users.email"],
                },
            ],
            tables: &["users"],
        },
        Case {
            name: "ctas_explicit_columns_override",
            sql: "CREATE TABLE rollup (uid INT, mail TEXT) AS SELECT id, email FROM users",
            dialect: &PostgreSqlDialect {},
            target: Some("rollup"),
            cols: &[
                Col {
                    name: Some("uid"),
                    sources: &["users.id"],
                },
                Col {
                    name: Some("mail"),
                    sources: &["users.email"],
                },
            ],
            tables: &["users"],
        },
        Case {
            name: "create_table_no_query_is_empty",
            sql: "CREATE TABLE t (a int)",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[],
            tables: &[],
        },
        Case {
            name: "update_from_source",
            sql: "UPDATE accounts a SET balance = t.delta FROM txns t WHERE t.acct = a.id",
            dialect: &PostgreSqlDialect {},
            target: Some("accounts"),
            cols: &[Col {
                name: Some("balance"),
                sources: &["txns.delta"],
            }],
            tables: &["txns"],
        },
        Case {
            name: "update_self_column",
            sql: "UPDATE users SET active = active",
            dialect: &PostgreSqlDialect {},
            target: Some("users"),
            cols: &[Col {
                name: Some("active"),
                sources: &["users.active"],
            }],
            tables: &["users"],
        },
        Case {
            name: "delete_using",
            sql: "DELETE FROM stale s USING archive a WHERE s.id = a.id",
            dialect: &PostgreSqlDialect {},
            target: Some("stale"),
            cols: &[],
            tables: &["archive"],
        },
        Case {
            name: "merge_update_and_insert",
            sql: "MERGE INTO tgt t USING src s ON t.id = s.id \
                  WHEN MATCHED THEN UPDATE SET val = s.val \
                  WHEN NOT MATCHED THEN INSERT (id, val) VALUES (s.id, s.val)",
            dialect: &PostgreSqlDialect {},
            target: Some("tgt"),
            cols: &[
                Col {
                    name: Some("val"),
                    sources: &["src.val"],
                },
                Col {
                    name: Some("id"),
                    sources: &["src.id"],
                },
            ],
            tables: &["src"],
        },
        Case {
            name: "explain_is_empty",
            sql: "EXPLAIN SELECT 1",
            dialect: &PostgreSqlDialect {},
            target: None,
            cols: &[],
            tables: &[],
        },
        Case {
            name: "show_is_empty",
            sql: "SHOW TABLES",
            dialect: &MySqlDialect {},
            target: None,
            cols: &[],
            tables: &[],
        },
    ];
    for case in cases {
        let l = extract(case.sql, case.dialect).unwrap_or_else(|e| panic!("{}: unexpected error {e:?}", case.name));
        assert_target(case.name, &l, case.target);
        assert_eq!(l.columns.len(), case.cols.len(), "{}: column count", case.name);
        for (i, expected) in case.cols.iter().enumerate() {
            assert_col(case.name, &l, i, expected);
        }
        assert_tables(case.name, &l, case.tables);
    }
}

#[test]
fn extract_fails() {
    let cases = [
        ErrCase {
            name: "circular_cte",
            sql: "WITH a AS (SELECT x FROM b), b AS (SELECT x FROM a) SELECT x FROM a",
            dialect: &PostgreSqlDialect {},
            expected: LineageError::CircularReference { cte: String::new() },
        },
        ErrCase {
            name: "recursive_cte",
            sql: "WITH RECURSIVE r AS (SELECT id FROM seed UNION ALL SELECT id FROM r) SELECT id FROM r",
            dialect: &PostgreSqlDialect {},
            expected: LineageError::CircularReference { cte: String::new() },
        },
        ErrCase {
            name: "wildcard_over_base_table",
            sql: "SELECT * FROM users",
            dialect: &SQLiteDialect {},
            expected: LineageError::UnresolvedColumn {
                column: String::new(),
                reason: String::new(),
            },
        },
        ErrCase {
            name: "ambiguous_unqualified",
            sql: "SELECT x FROM t1 JOIN t2 ON t1.id = t2.id",
            dialect: &PostgreSqlDialect {},
            expected: LineageError::UnresolvedColumn {
                column: String::new(),
                reason: String::new(),
            },
        },
        ErrCase {
            name: "insert_count_mismatch",
            sql: "INSERT INTO tgt (a) SELECT x, y FROM s",
            dialect: &PostgreSqlDialect {},
            expected: LineageError::ColumnCountMismatch {
                target: String::new(),
                expected: 0,
                found: 0,
            },
        },
        ErrCase {
            name: "parse_error",
            sql: "garbage sql ;;",
            dialect: &PostgreSqlDialect {},
            expected: LineageError::Parse(String::new()),
        },
        ErrCase {
            name: "multi_statement",
            sql: "SELECT 1; SELECT 2",
            dialect: &PostgreSqlDialect {},
            expected: LineageError::MultiStatement,
        },
    ];
    for case in &cases {
        let err = extract(case.sql, case.dialect).expect_err(case.name);
        assert_eq!(
            std::mem::discriminant(&err),
            std::mem::discriminant(&case.expected),
            "{}: expected {:?}, got {err:?}",
            case.name,
            case.expected
        );
    }
}

#[test]
fn deep_nesting_does_not_panic() {
    let mut sql = String::from("SELECT x FROM t");
    for _ in 0..200 {
        sql = format!("SELECT x FROM ({sql}) s");
    }
    let _ = extract(&sql, &PostgreSqlDialect {});
}

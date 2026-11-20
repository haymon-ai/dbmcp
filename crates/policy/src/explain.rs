//! Prunes `explainQuery` plans of ruled relations to value-free keys.
//!
//! Each dialect has one flat allowlist: at any depth a key survives only when
//! listed, an unlisted key drops with its whole subtree. The outer shape of
//! the result must match the dialect's, else the result is refused.

use dbmcp_pii::RedactionError;
use dbmcp_sql_lineage::Dialect;
use serde_json::Value;

/// `MySQL` format v1 plan keys that carry no values.
const MYSQL: &[&str] = &[
    "query_block",
    "nested_loop",
    "table",
    "ordering_operation",
    "grouping_operation",
    "duplicates_removal",
    "windowing",
    "windows",
    "frame_buffer",
    "union_result",
    "intersect_result",
    "except_result",
    "unary_result",
    "query_specifications",
    "materialized_from_subquery",
    "attached_subqueries",
    "select_list_subqueries",
    "optimized_away_subqueries",
    "having_subqueries",
    "order_by_subqueries",
    "group_by_subqueries",
    "update_value_subqueries",
    "insert_values_subqueries",
    "insert_update_subqueries",
    "insert_from",
    "buffer_result",
    "cost_info",
    "select_id",
    "table_name",
    "access_type",
    "possible_keys",
    "key",
    "used_key_parts",
    "key_length",
    "ref",
    "used_columns",
    "partitions",
    "partial_update_columns",
    "sharing_temporary_table_with",
    "name",
    "definition_position",
    "last_executed_window",
    "functions",
    "first_match",
    "rematerialize",
    "using_join_buffer",
    "range_checked_for_each_record",
    "table_function",
    "using_secondary_engine",
    "cacheable",
    "dependent",
    "using_filesort",
    "using_temporary_table",
    "using_index",
    "using_where",
    "not_exists",
    "using_MRR",
    "full_scan_on_NULL_key",
    "using_index_for_group_by",
    "using_index_for_skip_scan",
    "distinct",
    "loosescan",
    "backward_index_scan",
    "recursive",
    "skip_records_in_range_due_to_force",
    "const_row_not_found",
    "unique_row_not_found",
    "impossible_on_condition",
    "insert",
    "update",
    "delete",
    "replace",
    "artificial_window",
    "optimized_frame_evaluation",
    "rows_examined_per_scan",
    "rows_produced_per_join",
    "filtered",
    "read_cost",
    "eval_cost",
    "prefix_cost",
    "sort_cost",
    "query_cost",
    "data_read_per_join",
    "message",
];

/// `MariaDB` plan keys that carry no values.
const MARIADB: &[&str] = &[
    "query_block",
    "nested_loop",
    "table",
    "union_result",
    "query_specifications",
    "subqueries",
    "materialized",
    "materialization",
    "temporary_table",
    "filesort",
    "read_sorted_file",
    "window_functions_computation",
    "sorts",
    "duplicate_removal",
    "duplicates_removal",
    "block-nl-join",
    "rowid_filter",
    "range",
    "index_merge",
    "expression_cache",
    "subquery_cache",
    "range-checked-for-each-record",
    "table_function",
    "recursive_union",
    "select_id",
    "table_name",
    "access_type",
    "possible_keys",
    "key",
    "key_length",
    "used_key_parts",
    "ref",
    "partitions",
    "rows",
    "filtered",
    "cost",
    "loops",
    "selectivity_pct",
    "buffer_type",
    "buffer_size",
    "join_type",
    "mrr_type",
    "state",
    "lateral",
    "unique",
    "operation",
    "using_index",
    "using_index_for_group_by",
    "not_exists",
    "first_match",
    "loose_scan",
    "distinct",
    "full_scan",
    "full-scan-on-null_key",
    "cacheable",
    "dependent",
    "delete",
    "update",
    "const_row_not_found",
    "unique_row_not_found",
    "impossible_on_condition",
    "open_frm_only",
    "open_full_table",
    "skip_open_table",
    "scanned_databases",
    "pushed_condition",
    "message",
];

/// `PostgreSQL` plan keys that carry no values.
const POSTGRES: &[&str] = &[
    "Plan",
    "JIT",
    "Functions",
    "Options",
    "Inlining",
    "Optimization",
    "Expressions",
    "Deforming",
    "Node Type",
    "Parent Relationship",
    "Subplan Name",
    "Plans",
    "Strategy",
    "Partial Mode",
    "Operation",
    "Command",
    "Join Type",
    "Scan Direction",
    "Cache Mode",
    "Sampling Method",
    "Conflict Resolution",
    "Parallel Aware",
    "Async Capable",
    "Inner Unique",
    "Single Copy",
    "Disabled",
    "Relation Name",
    "Alias",
    "Index Name",
    "Function Name",
    "CTE Name",
    "Tuplestore Name",
    "Table Function Name",
    "Custom Plan Provider",
    "Conflict Arbiter Indexes",
    "Target Tables",
    "Workers Planned",
    "Subplans Removed",
    "Planned Partitions",
    "Startup Cost",
    "Total Cost",
    "Plan Rows",
    "Plan Width",
];

/// `SQLite` `EXPLAIN QUERY PLAN` columns.
const SQLITE_COLUMNS: [&str; 4] = ["id", "parent", "notused", "detail"];

/// Prunes an `EXPLAIN` result to the dialect's value-free plan keys.
///
/// `SQLite` rows only have their shape checked.
///
/// # Errors
///
/// Returns [`RedactionError::Internal`] when the result's outer shape is not the dialect's.
pub(crate) fn prune(dialect: Dialect, names: &[String], rows: &mut [Vec<Value>]) -> Result<(), RedactionError> {
    match dialect {
        Dialect::Mysql => mysql(MYSQL, names, rows),
        Dialect::Mariadb => mysql(MARIADB, names, rows),
        Dialect::Postgres => postgres(names, rows),
        Dialect::Sqlite => sqlite(names, rows),
    }
}

/// Checks the four `EXPLAIN QUERY PLAN` columns; rows stay unchanged.
fn sqlite(names: &[String], rows: &[Vec<Value>]) -> Result<(), RedactionError> {
    if names == SQLITE_COLUMNS && rows.iter().all(|row| row.len() == SQLITE_COLUMNS.len()) {
        Ok(())
    } else {
        Err(unexpected())
    }
}

/// Prunes the one `EXPLAIN` text cell of a format v1 plan, keeping it text.
fn mysql(keys: &[&str], names: &[String], rows: &mut [Vec<Value>]) -> Result<(), RedactionError> {
    let ([name], [row]) = (names, rows) else {
        return Err(unexpected());
    };
    let (true, [Value::String(text)]) = (name == "EXPLAIN", row.as_mut_slice()) else {
        return Err(unexpected());
    };
    let mut plan: Value = serde_json::from_str(text).map_err(|_| unexpected())?;
    if !plan
        .as_object()
        .is_some_and(|root| root.len() == 1 && root.contains_key("query_block"))
    {
        return Err(unexpected());
    }
    walk(&mut plan, keys);
    *text = plan.to_string();
    Ok(())
}

/// Prunes each element of the one `QUERY PLAN` array cell.
fn postgres(names: &[String], rows: &mut [Vec<Value>]) -> Result<(), RedactionError> {
    let ([name], [row]) = (names, rows) else {
        return Err(unexpected());
    };
    let (true, [Value::Array(plans)]) = (name == "QUERY PLAN", row.as_mut_slice()) else {
        return Err(unexpected());
    };
    let planned = |plan: &Value| plan.get("Plan").is_some_and(Value::is_object);
    if plans.is_empty() || !plans.iter().all(planned) {
        return Err(unexpected());
    }
    for plan in plans {
        walk(plan, POSTGRES);
    }
    Ok(())
}

/// Drops every key not in `keys` at any depth; arrays are walked element by element.
fn walk(value: &mut Value, keys: &[&str]) {
    match value {
        Value::Object(map) => map.retain(|key, value| {
            let kept = keys.contains(&key.as_str());
            if kept {
                walk(value, keys);
            }
            kept
        }),
        Value::Array(items) => {
            for item in items {
                walk(item, keys);
            }
        }
        _ => {}
    }
}

/// The refusal for a result whose outer shape is not the dialect's.
fn unexpected() -> RedactionError {
    RedactionError::Internal("column policy: EXPLAIN plan has an unexpected shape".into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Prunes one `MySQL`-family cell holding `plan` as text and returns the parsed result.
    fn pruned_text(dialect: Dialect, plan: &Value) -> Value {
        let mut rows = vec![vec![Value::String(plan.to_string())]];
        prune(dialect, &["EXPLAIN".into()], &mut rows).expect("pruned");
        let Value::String(text) = &rows[0][0] else {
            panic!("cell stays a string: {rows:?}");
        };
        serde_json::from_str(text).expect("valid JSON")
    }

    #[test]
    fn mysql_drops_expression_keys_at_depth() {
        let plan = json!({"query_block": {"select_id": 1, "nested_loop": [{"table": {
            "table_name": "users",
            "access_type": "ALL",
            "possible_keys": ["PRIMARY", "email"],
            "used_columns": ["id", "email"],
            "attached_condition": "(`shop`.`users`.`id` = 1)",
            "cost_info": {"read_cost": "0.25", "prefix_cost": "0.35"},
        }}]}});
        let expected = json!({"query_block": {"select_id": 1, "nested_loop": [{"table": {
            "table_name": "users",
            "access_type": "ALL",
            "possible_keys": ["PRIMARY", "email"],
            "used_columns": ["id", "email"],
            "cost_info": {"read_cost": "0.25", "prefix_cost": "0.35"},
        }}]}});
        assert_eq!(pruned_text(Dialect::Mysql, &plan), expected);
    }

    #[test]
    fn unknown_keys_drop_with_their_subtree_and_order_stays() {
        let plan = json!({"query_block": {
            "using_where": true,
            "future_key": {"table": {"table_name": "hidden"}},
            "select_id": 1,
            "message": "No tables used",
        }});
        let mut rows = vec![vec![Value::String(plan.to_string())]];
        prune(Dialect::Mysql, &["EXPLAIN".into()], &mut rows).expect("pruned");
        assert_eq!(
            rows,
            [[json!(
                r#"{"query_block":{"using_where":true,"select_id":1,"message":"No tables used"}}"#
            )]]
        );
    }

    #[test]
    fn pushed_condition_is_kept_only_on_mariadb() {
        let plan = |pushed: Value| json!({"query_block": {"table": {"table_name": "t", "pushed_condition": pushed}}});
        assert_eq!(pruned_text(Dialect::Mariadb, &plan(json!(true))), plan(json!(true)));
        assert_eq!(
            pruned_text(Dialect::Mysql, &plan(json!("(`t`.`a` = 'x')"))),
            json!({"query_block": {"table": {"table_name": "t"}}})
        );
    }

    /// Asserts that pruning `rows` named `names` on `dialect` fails on its shape.
    fn refused(dialect: Dialect, names: &[&str], mut rows: Vec<Vec<Value>>) {
        let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        match prune(dialect, &names, &mut rows) {
            Err(RedactionError::Internal(message)) => {
                assert_eq!(message, "column policy: EXPLAIN plan has an unexpected shape");
            }
            other => panic!("{dialect:?} {names:?} {rows:?}: {other:?}"),
        }
    }

    #[test]
    fn mysql_family_refuses_other_shapes() {
        let v1 = json!(r#"{"query_block":{"select_id":1}}"#);
        for dialect in [Dialect::Mysql, Dialect::Mariadb] {
            refused(dialect, &["EXPLAIN"], vec![]);
            refused(dialect, &["EXPLAIN"], vec![vec![v1.clone()], vec![v1.clone()]]);
            refused(dialect, &["QUERY PLAN"], vec![vec![v1.clone()]]);
            refused(dialect, &["EXPLAIN"], vec![vec![json!({"query_block": {}})]]);
            refused(dialect, &["EXPLAIN"], vec![vec![json!("{not json")]]);
            refused(
                dialect,
                &["EXPLAIN"],
                vec![vec![json!(r#"{"query":"/* select#1 */ select 'x'","inputs":[]}"#)]],
            );
            refused(
                dialect,
                &["EXPLAIN"],
                vec![vec![json!(r#"{"query_block":{},"extra":1}"#)]],
            );
        }
    }

    #[test]
    fn postgres_prunes_each_plan_element() {
        let child = json!({
            "Node Type": "Seq Scan",
            "Parent Relationship": "Outer",
            "Relation Name": "users",
            "Alias": "users",
            "Plan Rows": 1,
            "Filter": "(email = 'a@b.c'::text)",
            "Output": ["email"],
        });
        let plan = json!({"Plan": {"Node Type": "Hash Join", "Hash Cond": "(a.id = b.id)", "Plans": [child]}, "Planning": {"x": 1}});
        let mut rows = vec![vec![json!([plan, plan])]];
        prune(Dialect::Postgres, &["QUERY PLAN".into()], &mut rows).expect("pruned");
        let child = json!({
            "Node Type": "Seq Scan",
            "Parent Relationship": "Outer",
            "Relation Name": "users",
            "Alias": "users",
            "Plan Rows": 1,
        });
        let element = json!({"Plan": {"Node Type": "Hash Join", "Plans": [child]}});
        assert_eq!(rows, [[json!([element, element])]]);
    }

    #[test]
    fn postgres_refuses_other_shapes() {
        let ok = json!([{"Plan": {"Node Type": "Result"}}]);
        refused(Dialect::Postgres, &["QUERY PLAN"], vec![]);
        refused(
            Dialect::Postgres,
            &["QUERY PLAN"],
            vec![vec![ok.clone()], vec![ok.clone()]],
        );
        refused(Dialect::Postgres, &["EXPLAIN"], vec![vec![ok]]);
        refused(
            Dialect::Postgres,
            &["QUERY PLAN"],
            vec![vec![json!("Seq Scan on users")]],
        );
        refused(Dialect::Postgres, &["QUERY PLAN"], vec![vec![json!([])]]);
        refused(Dialect::Postgres, &["QUERY PLAN"], vec![vec![json!([{"Plans": []}])]]);
        refused(Dialect::Postgres, &["QUERY PLAN"], vec![vec![json!([{"Plan": "x"}])]]);
        refused(Dialect::Postgres, &["QUERY PLAN"], vec![vec![json!(["x"])]]);
    }

    #[test]
    fn sqlite_rows_pass_unchanged_under_their_columns() {
        let names: Vec<String> = SQLITE_COLUMNS.map(String::from).to_vec();
        let row = vec![
            json!(2),
            json!(0),
            json!(0),
            json!("SEARCH users USING INTEGER PRIMARY KEY (rowid=?)"),
        ];
        let mut rows = vec![row.clone()];
        prune(Dialect::Sqlite, &names, &mut rows).expect("pruned");
        assert_eq!(rows, [row]);
        prune(Dialect::Sqlite, &names, &mut []).expect("no rows");
    }

    #[test]
    fn sqlite_refuses_other_shapes() {
        let row = || vec![json!(2), json!(0), json!(0), json!("SCAN users")];
        refused(Dialect::Sqlite, &["id", "parent", "notused"], vec![row()]);
        refused(Dialect::Sqlite, &["addr", "opcode", "p1", "p2"], vec![row()]);
        refused(Dialect::Sqlite, &SQLITE_COLUMNS, vec![row(), vec![json!(2)]]);
    }
}

//! Positional redaction of result cells under a column plan.
//!
//! Cell `i` of every row is redacted by the plan's position `i`:
//!
//! - `Global`: global detection over every string leaf, with the column name
//!   as context, exactly like [`Redactor::apply`] on the row object.
//! - `Action`: every non-null value is replaced whole by the rule's operator,
//!   labelled with its entity. A string is rewritten from its text, anything
//!   else (numbers, booleans, `json`/`jsonb`, keys included) from its compact
//!   JSON text, object keys sorted. `NULL` stays `null`.
//! - `Placeholder`: every value, `NULL` included, becomes the entity's
//!   placeholder (`<EMAIL_ADDRESS>`); no operator runs.
//!
//! Rows of `EXPLAIN`, `SHOW`, and `DESCRIBE` are not positional; redact them
//! with no plan, which runs global detection on every cell.

use std::panic::{AssertUnwindSafe, catch_unwind};

use dbmcp_config::{ColumnRule, PiiOperator};
use serde_json::Value;

use crate::redact::{Frame, log_redactions};
use crate::{Entity, Operator, RedactionError, RedactionStats, Redactor};

/// How the values of each output column are redacted, in result order.
///
/// Positions follow the result set of a query or a write's `RETURNING`
/// only. For `EXPLAIN`, `SHOW`, `DESCRIBE`, and DDL the server returns
/// other columns, so callers detect globally on those rows instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// One entry per output column, by 0-based position.
    pub columns: Vec<PlannedColumn>,
}

/// One output column's redaction and the name it must come back under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedColumn {
    /// How the column's values are redacted.
    pub position: Position,
    /// The folded name the column must come back under, where known.
    ///
    /// `Some` only for a single source under its own name (`*`, a bare
    /// column, or a same-name alias).
    pub name: Option<String>,
}

/// How the values of one output column are redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// Replace every non-null value whole with the operator; the entity labels it.
    Action(PiiOperator, Entity),
    /// Replace every value, `NULL` included, with the entity's placeholder.
    Placeholder(Entity),
    /// Run global detection, as without column rules.
    Global,
}

/// Why result cells do not fit a column plan.
///
/// Messages never name an output column: an unaliased name is expression
/// text and may hold literals.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShapeError {
    /// The result has a different number of columns than the plan.
    #[error("column policy: result has {actual} columns but the statement plans {expected}")]
    ColumnCount {
        /// Positions in the plan.
        expected: usize,
        /// Columns in the result.
        actual: usize,
    },
    /// A result column came back under another name than the plan expects.
    #[error("column policy: result column {position} does not match the planned column")]
    NameMismatch {
        /// 0-based position in the result set.
        position: usize,
    },
}

/// One statement's result: column names once, then value rows.
///
/// Fill it with [`Cells::push`] in read order, then redact it with
/// [`Redactor::redact_cells`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cells {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

impl Cells {
    /// Creates cells from column names and value rows, each in result order.
    #[must_use]
    pub fn new(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        Self { columns, rows }
    }

    /// Appends one row of ordered `(name, value)` entries.
    ///
    /// The first row sets the column names; later rows add only values.
    pub fn push(&mut self, entries: Vec<(String, Value)>) {
        if self.rows.is_empty() {
            let (columns, values): (Vec<String>, Vec<Value>) = entries.into_iter().unzip();
            self.columns = columns;
            self.rows.push(values);
        } else {
            self.rows.push(entries.into_iter().map(|(_, value)| value).collect());
        }
    }
}

/// Checks a result's column names against `plan` before any row is used.
///
/// Lets a backend refuse a result while its transaction can still roll
/// back; [`Redactor::redact_cells`] runs the same checks.
///
/// # Errors
///
/// Returns [`ShapeError::ColumnCount`] when `names` and `plan` differ in
/// length, and [`ShapeError::NameMismatch`] for the first known planned
/// name that does not match its column, folded.
pub fn check_columns(plan: &Plan, names: &[String]) -> Result<(), ShapeError> {
    if names.len() != plan.columns.len() {
        return Err(ShapeError::ColumnCount {
            expected: plan.columns.len(),
            actual: names.len(),
        });
    }
    check_names(plan, names)
}

/// Checks that every known planned name matches its result column, folded.
fn check_names(plan: &Plan, names: &[String]) -> Result<(), ShapeError> {
    for (position, (name, column)) in names.iter().zip(&plan.columns).enumerate() {
        if let Some(expected) = &column.name
            && ColumnRule::fold(name) != *expected
        {
            return Err(ShapeError::NameMismatch { position });
        }
    }
    Ok(())
}

impl Redactor {
    /// Redacts `cells` in place, cell `i` of every row by `plan` position `i`.
    ///
    /// With no plan every cell is `Global` and the shape is not checked.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Shape`] before any cell changes, and
    /// [`RedactionError::Internal`] on a panic or NER failure.
    pub(crate) fn apply_cells(&self, cells: &mut Cells, plan: Option<&Plan>) -> Result<RedactionStats, RedactionError> {
        let Cells { columns, rows } = cells;
        if rows.is_empty() {
            return Ok(RedactionStats::default());
        }
        let actions = match plan {
            Some(plan) => {
                if let Some(row) = rows.iter().find(|row| row.len() != plan.columns.len()) {
                    return Err(ShapeError::ColumnCount {
                        expected: plan.columns.len(),
                        actual: row.len(),
                    }
                    .into());
                }
                check_names(plan, columns)?;
                plan.columns
                    .iter()
                    .map(|column| match column.position {
                        Position::Global => Action::Global,
                        Position::Action(operator, entity) => Action::Whole(self.operator(operator), entity),
                        Position::Placeholder(entity) => Action::Placeholder(entity),
                    })
                    .collect::<Vec<_>>()
            }
            None => columns.iter().map(|_| Action::Global).collect(),
        };

        let row_count = rows.len();
        let columns = &*columns;
        let (mut roots, mut stats) = catch_unwind(AssertUnwindSafe(move || {
            let mut stats = RedactionStats::default();
            let mut roots = Vec::new();
            for row in rows {
                for ((value, name), action) in row.iter_mut().zip(columns).zip(&actions) {
                    let entity = match action {
                        Action::Whole(operator, entity) if rewrite_whole(value, operator, *entity) => *entity,
                        Action::Whole(..) => continue,
                        Action::Placeholder(entity) => {
                            *value = Value::String(entity.placeholder().to_owned());
                            *entity
                        }
                        Action::Global => {
                            roots.push(Frame::KeyedChild(value, name.as_str()));
                            continue;
                        }
                    };
                    stats.total += 1;
                    *stats.by_entity.entry(entity).or_insert(0) += 1;
                }
            }
            (roots, stats)
        }))
        .map_err(|_| RedactionError::Internal("operator panicked".into()))?;
        roots.reverse();
        self.walk(roots, &mut stats)?;
        log_redactions(&stats, Some(row_count));
        Ok(stats)
    }

    /// Redacts `cells` by `plan` and returns its rows as JSON objects.
    ///
    /// With no plan every cell runs global detection, as for `EXPLAIN`,
    /// `SHOW`, and `DESCRIBE` rows. Offloads to a blocking thread when a
    /// `Global` position needs NER. A repeated name keeps its last value.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Shape`] when the result does not fit `plan`
    /// (an empty result is not checked), and [`RedactionError::Internal`]
    /// when redaction panics or NER fails (fail-closed).
    pub async fn redact_cells(&self, mut cells: Cells, plan: Option<&Plan>) -> Result<Vec<Value>, RedactionError> {
        let global = plan.is_none_or(|plan| plan.columns.iter().any(|column| column.position == Position::Global));
        if self.uses_ner() && global {
            let redactor = self.clone();
            let plan = plan.cloned();
            cells = tokio::task::spawn_blocking(move || redactor.apply_cells(&mut cells, plan.as_ref()).map(|_| cells))
                .await
                .map_err(|e| RedactionError::Internal(format!("redaction task panicked: {e}")))??;
        } else {
            self.apply_cells(&mut cells, plan)?;
        }
        let Cells { columns, rows } = cells;
        Ok(rows
            .into_iter()
            .map(|row| Value::Object(columns.iter().cloned().zip(row).collect()))
            .collect())
    }
}

/// How one output column's cells are redacted, its operator built once.
enum Action {
    /// Global detection under the column name.
    Global,
    /// A whole-value rewrite of every non-null cell by the operator.
    Whole(Operator, Entity),
    /// The entity's placeholder in every cell, `NULL` included.
    Placeholder(Entity),
}

/// Rewrites a non-null `value` whole; returns `false` for `NULL`.
fn rewrite_whole(value: &mut Value, operator: &Operator, entity: Entity) -> bool {
    let text = match &mut *value {
        Value::Null => return false,
        Value::String(text) => std::mem::take(text),
        other => {
            other.sort_all_objects();
            other.to_string()
        }
    };
    *value = Value::String(operator.apply(entity, &text).into_owned());
    true
}

#[cfg(test)]
mod tests {
    use dbmcp_config::PiiConfig;
    use serde_json::json;

    use super::*;

    const EMAIL: &str = "jane.doe@example.com";
    const KEY: &str = "0123456789abcdef0123456789abcdef";

    /// A plan with no known names.
    fn plan(positions: Vec<Position>) -> Plan {
        let columns = positions
            .into_iter()
            .map(|position| PlannedColumn { position, name: None })
            .collect();
        Plan { columns }
    }

    /// A result of value rows, its columns named `c0`, `c1`, ….
    fn cells(rows: Vec<Vec<Value>>) -> Cells {
        let mut cells = Cells::default();
        for values in rows {
            cells.push(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(i, value)| (format!("c{i}"), value))
                    .collect(),
            );
        }
        cells
    }

    /// Redacts one row in place and returns its values.
    fn redact(redactor: &Redactor, positions: Vec<Position>, values: Vec<Value>) -> Vec<Value> {
        let mut cells = cells(vec![values]);
        redactor
            .apply_cells(&mut cells, Some(&plan(positions)))
            .expect("redaction runs");
        cells.rows.remove(0)
    }

    /// A redactor from a config with `operator` and a fixed key.
    fn keyed(operator: PiiOperator) -> Redactor {
        let config = PiiConfig {
            operator,
            key: Some(KEY.to_string().into()),
            ..PiiConfig::default()
        };
        Redactor::from_config(Some(&config)).expect("builds").expect("enabled")
    }

    const MASKED: Position = Position::Action(PiiOperator::Mask, Entity::UsSsn);

    #[test]
    fn push_keeps_the_first_row_names_and_later_values() {
        let mut cells = Cells::default();
        cells.push(vec![("a".into(), json!(1)), ("b".into(), json!(2))]);
        cells.push(vec![("x".into(), json!(3)), ("y".into(), json!(4))]);
        assert_eq!(cells.columns, ["a", "b"]);
        assert_eq!(cells.rows, [vec![json!(1), json!(2)], vec![json!(3), json!(4)]]);
    }

    #[test]
    fn known_names_must_match_folded_and_unknown_ones_are_not_compared() {
        let plan = Plan {
            columns: vec![
                PlannedColumn {
                    position: Position::Global,
                    name: Some("email".into()),
                },
                PlannedColumn {
                    position: Position::Global,
                    name: None,
                },
            ],
        };
        check_names(&plan, &["EMAIL", "?column?"].map(String::from)).expect("folded match, unknown skipped");
        let err = check_names(&plan, &["ssn", "x"].map(String::from)).expect_err("name differs");
        assert_eq!(err, ShapeError::NameMismatch { position: 0 });
    }

    #[test]
    fn duplicate_names_are_allowed() {
        check_names(&plan(vec![MASKED, Position::Global]), &["a", "a"].map(String::from)).expect("duplicates allowed");
    }

    #[test]
    fn check_columns_checks_count_then_names() {
        let two = plan(vec![MASKED, Position::Global]);
        let err = check_columns(&two, &["a".to_owned()]).expect_err("count differs");
        assert_eq!(err, ShapeError::ColumnCount { expected: 2, actual: 1 });
        check_columns(&two, &["a", "b"].map(String::from)).expect("unknown names pass");
    }

    #[test]
    fn misfit_row_fails_before_any_cell_changes() {
        let mut cells = cells(vec![vec![json!(EMAIL), json!("123-45-6789")]]);
        let err = Redactor::with_defaults()
            .apply_cells(&mut cells, Some(&plan(vec![MASKED])))
            .expect_err("count differs");
        assert!(
            matches!(err, RedactionError::Shape(ShapeError::ColumnCount { .. })),
            "{err}"
        );
        assert_eq!(cells.rows[0][1], json!("123-45-6789"));
    }

    #[test]
    fn later_row_of_wrong_length_is_rejected() {
        let mut cells = cells(vec![vec![json!("a")], vec![json!("a"), json!("b")]]);
        let err = Redactor::with_defaults()
            .apply_cells(&mut cells, Some(&plan(vec![MASKED])))
            .expect_err("second row differs");
        assert!(
            matches!(err, RedactionError::Shape(ShapeError::ColumnCount { .. })),
            "{err}"
        );
    }

    #[test]
    fn empty_result_is_not_checked() {
        let mut cells = Cells::default();
        let stats = Redactor::with_defaults()
            .apply_cells(&mut cells, Some(&plan(vec![MASKED, Position::Global])))
            .expect("nothing to check");
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn action_rewrites_any_type_whole_as_string_and_keeps_null() {
        let values = redact(
            &Redactor::with_defaults(),
            vec![MASKED; 5],
            vec![
                json!("123-45-6789"),
                json!(1234),
                json!(true),
                json!({"ssn": "1"}),
                Value::Null,
            ],
        );
        assert_eq!(
            values,
            [
                json!("***********"),
                json!("****"),
                json!("****"),
                json!("***********"),
                Value::Null
            ]
        );
    }

    #[test]
    fn placeholder_rewrites_every_value_null_included_and_counts_each() {
        let mut cells = cells(vec![vec![
            json!("jane@example.com"),
            json!(1234),
            json!(true),
            json!({"email": "x"}),
            Value::Null,
            Value::Null,
        ]]);
        let mut positions = vec![Position::Placeholder(Entity::EmailAddress); 5];
        positions.push(MASKED);
        let stats = Redactor::with_defaults()
            .apply_cells(&mut cells, Some(&plan(positions)))
            .expect("runs");
        assert_eq!(
            cells.rows[0],
            [
                json!("<EMAIL_ADDRESS>"),
                json!("<EMAIL_ADDRESS>"),
                json!("<EMAIL_ADDRESS>"),
                json!("<EMAIL_ADDRESS>"),
                json!("<EMAIL_ADDRESS>"),
                Value::Null
            ]
        );
        assert_eq!(stats.total, 5);
        assert_eq!(stats.by_entity.get(&Entity::EmailAddress), Some(&5));
    }

    #[test]
    fn action_replace_writes_the_entity_placeholder() {
        let values = redact(
            &Redactor::with_defaults(),
            vec![Position::Action(PiiOperator::Replace, Entity::Person)],
            vec![json!("Jane Doe")],
        );
        assert_eq!(values, [json!("<PERSON>")]);
    }

    #[test]
    fn whole_value_rewrites_are_counted_under_the_rule_entity() {
        let mut cells = cells(vec![
            vec![json!("123-45-6789"), Value::Null],
            vec![json!("987-65-4321"), json!("x")],
        ]);
        let stats = Redactor::with_defaults()
            .apply_cells(&mut cells, Some(&plan(vec![MASKED, MASKED])))
            .expect("runs");
        assert_eq!(stats.total, 3);
        assert_eq!(stats.by_entity.get(&Entity::UsSsn), Some(&3));
    }

    #[test]
    fn global_matches_row_redaction_with_the_column_name_as_context() {
        let redactor = Redactor::with_defaults();
        let mut cells = Cells::default();
        cells.push(vec![("contact".to_string(), json!(EMAIL))]);
        redactor
            .apply_cells(&mut cells, Some(&plan(vec![Position::Global])))
            .expect("runs");
        let mut objects = vec![json!({ "contact": EMAIL })];
        redactor.apply(&mut objects).expect("runs");
        assert_eq!(cells.rows[0][0], objects[0]["contact"]);
        assert_ne!(cells.rows[0][0], json!(EMAIL));
    }

    #[test]
    fn a_hashing_rule_token_equals_global_detection_token() {
        let redactor = keyed(PiiOperator::Hash);
        let values = redact(
            &redactor,
            vec![
                Position::Action(PiiOperator::Hash, Entity::EmailAddress),
                Position::Global,
            ],
            vec![json!(EMAIL), json!(EMAIL)],
        );
        assert_eq!(values[0], values[1]);
        assert!(
            values[0].as_str().is_some_and(|t| t.starts_with("<EMAIL_ADDRESS_")),
            "{values:?}"
        );
    }

    #[test]
    fn a_hashing_rule_under_another_global_operator_hashes() {
        let redactor = keyed(PiiOperator::Mask);
        let values = redact(
            &redactor,
            vec![Position::Action(PiiOperator::Hash, Entity::EmailAddress)],
            vec![json!(EMAIL)],
        );
        assert!(
            values[0].as_str().is_some_and(|t| t.starts_with("<EMAIL_ADDRESS_")),
            "{values:?}"
        );
    }

    const SORTED: &str = r#"{"a":{"c":3,"d":2},"b":1}"#;

    #[test]
    fn a_ruled_json_cell_hashes_its_sorted_key_text() {
        let redactor = keyed(PiiOperator::Hash);
        let hashed = Position::Action(PiiOperator::Hash, Entity::UsSsn);
        let values = redact(
            &redactor,
            vec![hashed, hashed, hashed],
            vec![
                json!({"b": 1, "a": {"d": 2, "c": 3}}),
                json!({"a": {"c": 3, "d": 2}, "b": 1}),
                json!(SORTED),
            ],
        );
        assert_eq!(values[0], values[1]);
        assert_eq!(values[0], values[2]);
    }

    #[test]
    fn a_ruled_json_cell_masks_to_its_sorted_key_length() {
        let redactor = keyed(PiiOperator::Mask);
        let values = redact(&redactor, vec![MASKED], vec![json!({"b": 1, "a": {"d": 2, "c": 3}})]);
        assert_eq!(values[0], json!("*".repeat(SORTED.len())));
    }

    #[tokio::test]
    async fn redact_cells_returns_objects_and_the_last_duplicate_wins() {
        let mut cells = Cells::default();
        cells.push(vec![
            ("x".to_string(), json!("123-45-6789")),
            ("note".to_string(), json!("plain")),
            ("x".to_string(), json!("987-65-4321")),
        ]);
        let out = Redactor::with_defaults()
            .redact_cells(cells, Some(&plan(vec![MASKED, Position::Global, MASKED])))
            .await
            .expect("runs");
        assert_eq!(out, [json!({ "x": "***********", "note": "plain" })]);
    }

    #[tokio::test]
    async fn no_plan_matches_row_redaction_of_the_row_objects() {
        let redactor = keyed(PiiOperator::Hash);
        let columns = ["contact", "ssn", "doc", "n", "contact"].map(String::from).to_vec();
        let rows = vec![
            vec![
                json!(EMAIL),
                json!("123-45-6789"),
                json!({ "email": EMAIL, "tags": ["call 555-123-4567", 7] }),
                json!(42),
                json!("second jane@example.org"),
            ],
            vec![Value::Null, json!("plain"), json!([EMAIL]), json!(true), json!(EMAIL)],
        ];
        let objects = rows
            .iter()
            .map(|values| Value::Object(columns.iter().cloned().zip(values.iter().cloned()).collect()))
            .collect();
        let expected = redactor.redact_rows(objects).await.expect("runs");
        let out = redactor
            .redact_cells(Cells::new(columns, rows), None)
            .await
            .expect("runs");
        assert_eq!(out, expected);
        assert_ne!(out[0]["contact"], json!("second jane@example.org"));
    }

    #[tokio::test]
    async fn redact_cells_keys_follow_column_order_under_rules_and_global() {
        let redactor = Redactor::with_defaults();
        let cells = || {
            let columns = ["c", "a", "b"].map(String::from).to_vec();
            Cells::new(columns, vec![vec![json!("123-45-6789"), json!(EMAIL), json!(1)]])
        };
        let keys = |rows: Vec<Value>| {
            rows[0]
                .as_object()
                .expect("row object")
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        let ruled = redactor
            .redact_cells(cells(), Some(&plan(vec![MASKED, Position::Global, Position::Global])))
            .await
            .expect("runs");
        let global = redactor.redact_cells(cells(), None).await.expect("runs");

        assert_eq!(keys(ruled), ["c", "a", "b"]);
        assert_eq!(keys(global), ["c", "a", "b"]);
    }
}

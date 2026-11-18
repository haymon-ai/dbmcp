//! Positional redaction of result cells under a column plan.
//!
//! With column rules active, a backend collects a result as [`Cells`]
//! (column names once, then value rows) and redacts cell `i` of every row by
//! the plan's position `i`:
//!
//! - `Global`: global detection over every string leaf, with the column name
//!   as context, exactly like [`Redactor::apply`] on the row object.
//! - `Action`: every non-null value is replaced whole by the rule's operator,
//!   labelled with its entity. A string is rewritten from its text, anything
//!   else (numbers, booleans, `json`/`jsonb`, keys included) from its compact
//!   JSON text. `NULL` stays `null`.
//!
//! Before any of it runs, the cells must fit the plan; an empty result has
//! nothing to check. Rows of `EXPLAIN`, `SHOW`, and `DESCRIBE` are not
//! positional; redact them with [`Redactor::redact_rows`].

use std::panic::{AssertUnwindSafe, catch_unwind};

use dbmcp_config::{ColumnRule, PiiOperator};
use serde_json::Value;

use crate::policy::{Plan, Position};
use crate::redact::{Frame, log_redactions};
use crate::{Entity, Operator, RedactionError, RedactionStats, Redactor};

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
    /// Column names, from the first pushed row.
    columns: Vec<String>,
    /// Row values, in column order.
    rows: Vec<Vec<Value>>,
}

impl Cells {
    /// Creates an empty result.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one row of ordered `(name, value)` entries.
    ///
    /// The first row sets the column names; later rows keep only their
    /// values, since every row of one result shares its columns.
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

/// Checks that result columns named `names` fit `plan`.
///
/// The column count must equal the plan's length, and every position with a
/// known name ([`Plan::names`]) must come back under it, compared folded by
/// [`ColumnRule::fold`]. Duplicate names are allowed.
///
/// # Errors
///
/// Returns [`ShapeError::ColumnCount`] on a count mismatch and
/// [`ShapeError::NameMismatch`] for the first position whose name differs.
fn check_columns(plan: &Plan, names: &[String]) -> Result<(), ShapeError> {
    if names.len() != plan.positions.len() {
        return Err(ShapeError::ColumnCount {
            expected: plan.positions.len(),
            actual: names.len(),
        });
    }
    for (position, (name, expected)) in names.iter().zip(&plan.names).enumerate() {
        if let Some(expected) = expected
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
    /// Whole-value rewrites count into the stats under the rule's entity.
    /// Emits one `tracing::info!` event when anything was rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Shape`] when the columns or a row do not fit
    /// `plan` (see [`check_columns`]), before any cell is touched, and
    /// [`RedactionError::Internal`] when the pipeline panics, the NER pass
    /// fails, or a rule hashes without a `hash` operator. The request must
    /// fail without returning any row (fail-closed).
    pub(crate) fn apply_cells(&self, cells: &mut Cells, plan: &Plan) -> Result<RedactionStats, RedactionError> {
        let Cells { columns, rows } = cells;
        if !rows.is_empty() {
            check_columns(plan, columns)?;
        }
        if let Some(row) = rows.iter().find(|row| row.len() != plan.positions.len()) {
            return Err(ShapeError::ColumnCount {
                expected: plan.positions.len(),
                actual: row.len(),
            }
            .into());
        }
        let actions = plan
            .positions
            .iter()
            .map(|position| match position {
                Position::Global => Ok(None),
                Position::Action(operator, entity) => Ok(Some((self.rule_operator(*operator)?, *entity))),
            })
            .collect::<Result<Vec<_>, RedactionError>>()?;

        let row_count = rows.len();
        let mut stats = RedactionStats::default();
        let mut roots = Vec::new();
        let mut rewrites = Vec::new();
        for row in rows.iter_mut() {
            for ((value, name), action) in row.iter_mut().zip(columns.iter()).zip(&actions) {
                match action {
                    Some((operator, entity)) => rewrites.push((value, operator, *entity)),
                    None => roots.push(Frame::KeyedChild(value, name.as_str())),
                }
            }
        }
        catch_unwind(AssertUnwindSafe(|| {
            for (value, operator, entity) in rewrites {
                if rewrite_whole(value, operator, entity) {
                    stats.total += 1;
                    *stats.by_entity.entry(entity).or_insert(0) += 1;
                }
            }
        }))
        .map_err(|_| RedactionError::Internal("operator panicked".into()))?;
        roots.reverse();
        self.walk(roots, &mut stats)?;
        log_redactions(&stats, Some(row_count));
        Ok(stats)
    }

    /// Redacts `cells` by `plan` and returns its rows as JSON objects.
    ///
    /// Runs inline, or on a blocking thread when a `Global` position runs the
    /// NER pass. Each row becomes an object, as the unruled path builds it; a
    /// repeated name keeps its last value, already redacted by its own
    /// position.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Shape`] when the columns or a row do not fit
    /// `plan`: the column count must equal the plan's length, and a column
    /// whose name the plan knows ([`Plan::names`]) must come back under it,
    /// compared folded by [`ColumnRule::fold`]. An empty result is not
    /// checked. Returns [`RedactionError::Internal`] when the pipeline or the
    /// offloaded task panics, the NER pass fails, or a rule hashes without a
    /// `hash` operator. The request must fail without returning any row
    /// (fail-closed).
    pub async fn redact_cells(&self, mut cells: Cells, plan: &Plan) -> Result<Vec<Value>, RedactionError> {
        if self.uses_ner() && plan.positions.contains(&Position::Global) {
            let redactor = self.clone();
            let plan = plan.clone();
            cells = tokio::task::spawn_blocking(move || redactor.apply_cells(&mut cells, &plan).map(|_| cells))
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

    /// Returns the operator a column rule names, sharing this redactor's `hash` key.
    fn rule_operator(&self, operator: PiiOperator) -> Result<Operator, RedactionError> {
        crate::redact::operator(operator, self.hash.as_ref())
            .ok_or_else(|| RedactionError::Internal("column rule hashes without a hash key".into()))
    }
}

/// Replaces a non-null `value` whole with `operator`'s output for `entity`.
///
/// A string is rewritten from its text; any other value from its compact
/// JSON text. Returns whether the value was rewritten (`NULL` is not).
fn rewrite_whole(value: &mut Value, operator: &Operator, entity: Entity) -> bool {
    let text = match &mut *value {
        Value::Null => return false,
        Value::String(text) => std::mem::take(text),
        other => other.to_string(),
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
        let names = vec![None; positions.len()];
        Plan { positions, names }
    }

    /// Column names as the result reports them.
    fn names<const N: usize>(names: [&str; N]) -> Vec<String> {
        names.map(String::from).to_vec()
    }

    /// A result of value rows, its columns named `c0`, `c1`, ….
    fn cells(rows: Vec<Vec<Value>>) -> Cells {
        let mut cells = Cells::new();
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
            .apply_cells(&mut cells, &plan(positions))
            .expect("redaction runs");
        cells.rows.remove(0)
    }

    /// A redactor from a config with `operator` and a fixed key.
    fn keyed(operator: PiiOperator, rules: Vec<ColumnRule>) -> Redactor {
        let config = PiiConfig {
            operator,
            key: Some(KEY.to_string().into()),
            rules,
            ..PiiConfig::default()
        };
        Redactor::from_config(Some(&config)).expect("builds").expect("enabled")
    }

    /// A hashing rule, as the loader resolves it.
    fn hash_rule() -> ColumnRule {
        ColumnRule {
            name: "emails".into(),
            item: 0,
            key: ["shop", "public", "users", "email"].map(String::from),
            entity: "EMAIL_ADDRESS",
            operator: PiiOperator::Hash,
        }
    }

    const MASKED: Position = Position::Action(PiiOperator::Mask, Entity::UsSsn);

    #[test]
    fn push_keeps_the_first_row_names_and_later_values() {
        let mut cells = Cells::new();
        cells.push(vec![("a".into(), json!(1)), ("b".into(), json!(2))]);
        cells.push(vec![("x".into(), json!(3)), ("y".into(), json!(4))]);
        assert_eq!(cells.columns, names(["a", "b"]));
        assert_eq!(cells.rows, [vec![json!(1), json!(2)], vec![json!(3), json!(4)]]);
    }

    #[test]
    fn count_mismatch_is_rejected() {
        let err = check_columns(&plan(vec![Position::Global]), &names(["a", "b"])).expect_err("count differs");
        assert_eq!(err, ShapeError::ColumnCount { expected: 1, actual: 2 });
    }

    #[test]
    fn known_names_must_match_folded_and_unknown_ones_are_not_compared() {
        let plan = Plan {
            positions: vec![Position::Global, Position::Global],
            names: vec![Some("email".into()), None],
        };
        check_columns(&plan, &names(["EMAIL", "?column?"])).expect("folded match, unknown skipped");
        let err = check_columns(&plan, &names(["ssn", "x"])).expect_err("name differs");
        assert_eq!(err, ShapeError::NameMismatch { position: 0 });
    }

    #[test]
    fn duplicate_names_are_allowed() {
        check_columns(&plan(vec![MASKED, Position::Global]), &names(["a", "a"])).expect("duplicates allowed");
    }

    #[test]
    fn misfit_row_fails_before_any_cell_changes() {
        let mut cells = cells(vec![vec![json!(EMAIL), json!("123-45-6789")]]);
        let err = Redactor::with_defaults()
            .apply_cells(&mut cells, &plan(vec![MASKED]))
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
            .apply_cells(&mut cells, &plan(vec![MASKED]))
            .expect_err("second row differs");
        assert!(
            matches!(err, RedactionError::Shape(ShapeError::ColumnCount { .. })),
            "{err}"
        );
    }

    #[test]
    fn empty_result_is_not_checked() {
        let mut cells = Cells::new();
        let stats = Redactor::with_defaults()
            .apply_cells(&mut cells, &plan(vec![MASKED, Position::Global]))
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
            .apply_cells(&mut cells, &plan(vec![MASKED, MASKED]))
            .expect("runs");
        assert_eq!(stats.total, 3);
        assert_eq!(stats.by_entity.get(&Entity::UsSsn), Some(&3));
    }

    #[test]
    fn global_matches_row_redaction_with_the_column_name_as_context() {
        let redactor = Redactor::with_defaults();
        let mut cells = Cells::new();
        cells.push(vec![("contact".to_string(), json!(EMAIL))]);
        redactor
            .apply_cells(&mut cells, &plan(vec![Position::Global]))
            .expect("runs");
        let mut objects = vec![json!({ "contact": EMAIL })];
        redactor.apply(&mut objects).expect("runs");
        assert_eq!(cells.rows[0][0], objects[0]["contact"]);
        assert_ne!(cells.rows[0][0], json!(EMAIL));
    }

    #[test]
    fn a_hashing_rule_token_equals_global_detection_token() {
        let redactor = keyed(PiiOperator::Hash, vec![hash_rule()]);
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
    fn a_hashing_rule_under_another_global_operator_still_gets_a_key() {
        let redactor = keyed(PiiOperator::Mask, vec![hash_rule()]);
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

    #[test]
    fn a_hashing_position_without_a_key_fails_closed() {
        let mut cells = cells(vec![vec![json!(EMAIL)]]);
        let err = Redactor::with_defaults()
            .apply_cells(
                &mut cells,
                &plan(vec![Position::Action(PiiOperator::Hash, Entity::EmailAddress)]),
            )
            .expect_err("no hash key");
        assert!(matches!(err, RedactionError::Internal(_)), "{err}");
    }

    #[tokio::test]
    async fn redact_cells_returns_objects_and_the_last_duplicate_wins() {
        let mut cells = Cells::new();
        cells.push(vec![
            ("x".to_string(), json!("123-45-6789")),
            ("note".to_string(), json!("plain")),
            ("x".to_string(), json!("987-65-4321")),
        ]);
        let out = Redactor::with_defaults()
            .redact_cells(cells, &plan(vec![MASKED, Position::Global, MASKED]))
            .await
            .expect("runs");
        assert_eq!(out, [json!({ "x": "***********", "note": "plain" })]);
    }
}

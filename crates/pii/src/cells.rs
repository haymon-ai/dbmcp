//! Positional redaction of result cells under a column plan.
//!
//! With column rules active, a backend reads each row as ordered
//! `(name, value)` cells and redacts cell `i` by the plan's position `i`:
//!
//! - `Global`: global detection over every string leaf, with the column name
//!   as context, exactly like [`Redactor::apply`] on the row object.
//! - `Action`: every non-null value is replaced whole by the rule's operator,
//!   labelled with its entity. A string is rewritten from its text, anything
//!   else (numbers, booleans, `json`/`jsonb`, keys included) from its compact
//!   JSON text. `NULL` stays `null`.
//!
//! [`check_columns`] guards that the cells fit the plan before any of it runs.
//! Rows of `EXPLAIN`, `SHOW`, and `DESCRIBE` are not positional; redact them
//! with [`Redactor::redact_rows`].

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
pub fn check_columns<'a>(plan: &Plan, names: impl IntoIterator<Item = &'a str>) -> Result<(), ShapeError> {
    let names: Vec<&str> = names.into_iter().collect();
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
    /// Redacts positional `rows` in place, cell `i` by `plan` position `i`.
    ///
    /// Whole-value rewrites count into the stats under the rule's entity.
    /// Emits one `tracing::info!` event when anything was rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Shape`] when a row does not fit `plan` (see
    /// [`check_columns`]), before any cell is touched, and
    /// [`RedactionError::Internal`] when the pipeline panics, the NER pass
    /// fails, or a rule hashes without a `hash` operator. The request must
    /// fail without returning any row (fail-closed).
    pub fn apply_cells(
        &self,
        rows: &mut [Vec<(String, Value)>],
        plan: &Plan,
    ) -> Result<RedactionStats, RedactionError> {
        // Every row of one result shares its columns: check names once, lengths per row.
        if let Some(first) = rows.first() {
            check_columns(plan, first.iter().map(|(name, _)| name.as_str()))?;
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
            for ((name, value), action) in row.iter_mut().zip(&actions) {
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

    /// Redacts positional `rows` by `plan` and returns them as JSON objects.
    ///
    /// Runs [`Self::apply_cells`] inline, or on a blocking thread when a
    /// `Global` position runs the NER pass. Each row becomes an object, as
    /// the unruled path builds it; a repeated name keeps its last value,
    /// already redacted by its own position.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::apply_cells`]'s errors, and returns
    /// [`RedactionError::Internal`] when the offloaded task panics.
    pub async fn redact_cells(
        &self,
        mut rows: Vec<Vec<(String, Value)>>,
        plan: &Plan,
    ) -> Result<Vec<Value>, RedactionError> {
        if self.uses_ner() && plan.positions.contains(&Position::Global) {
            let redactor = self.clone();
            let plan = plan.clone();
            rows = tokio::task::spawn_blocking(move || redactor.apply_cells(&mut rows, &plan).map(|_| rows))
                .await
                .map_err(|e| RedactionError::Internal(format!("redaction task panicked: {e}")))??;
        } else {
            self.apply_cells(&mut rows, plan)?;
        }
        Ok(rows
            .into_iter()
            .map(|row| Value::Object(row.into_iter().collect()))
            .collect())
    }

    /// Returns the operator a column rule names, sharing this redactor's `hash` key.
    fn rule_operator(&self, operator: PiiOperator) -> Result<Operator, RedactionError> {
        Ok(match operator {
            PiiOperator::Replace => Operator::Replace,
            PiiOperator::Mask => Operator::default_mask(),
            PiiOperator::Redact => Operator::Redact,
            PiiOperator::Hash => self
                .hash
                .clone()
                .ok_or_else(|| RedactionError::Internal("column rule hashes without a hash key".into()))?,
        })
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

    /// One row of cells named `c0`, `c1`, ….
    fn row(values: Vec<Value>) -> Vec<(String, Value)> {
        values
            .into_iter()
            .enumerate()
            .map(|(i, value)| (format!("c{i}"), value))
            .collect()
    }

    /// Redacts one row in place and returns its values.
    fn redact(redactor: &Redactor, positions: Vec<Position>, cells: Vec<Value>) -> Vec<Value> {
        let mut rows = vec![row(cells)];
        redactor
            .apply_cells(&mut rows, &plan(positions))
            .expect("redaction runs");
        rows.remove(0).into_iter().map(|(_, value)| value).collect()
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
    fn count_mismatch_is_rejected() {
        let err = check_columns(&plan(vec![Position::Global]), ["a", "b"]).expect_err("count differs");
        assert_eq!(err, ShapeError::ColumnCount { expected: 1, actual: 2 });
    }

    #[test]
    fn known_names_must_match_folded_and_unknown_ones_are_not_compared() {
        let plan = Plan {
            positions: vec![Position::Global, Position::Global],
            names: vec![Some("email".into()), None],
        };
        check_columns(&plan, ["EMAIL", "?column?"]).expect("folded match, unknown skipped");
        let err = check_columns(&plan, ["ssn", "x"]).expect_err("name differs");
        assert_eq!(err, ShapeError::NameMismatch { position: 0 });
    }

    #[test]
    fn duplicate_names_are_allowed() {
        check_columns(&plan(vec![MASKED, Position::Global]), ["a", "a"]).expect("duplicates allowed");
    }

    #[test]
    fn misfit_row_fails_before_any_cell_changes() {
        let mut rows = vec![row(vec![json!(EMAIL), json!("123-45-6789")])];
        let err = Redactor::with_defaults()
            .apply_cells(&mut rows, &plan(vec![MASKED]))
            .expect_err("count differs");
        assert!(
            matches!(err, RedactionError::Shape(ShapeError::ColumnCount { .. })),
            "{err}"
        );
        assert_eq!(rows[0][1].1, json!("123-45-6789"));
    }

    #[test]
    fn later_row_of_wrong_length_is_rejected() {
        let mut rows = vec![row(vec![json!("a")]), row(vec![json!("a"), json!("b")])];
        let err = Redactor::with_defaults()
            .apply_cells(&mut rows, &plan(vec![MASKED]))
            .expect_err("second row differs");
        assert!(
            matches!(err, RedactionError::Shape(ShapeError::ColumnCount { .. })),
            "{err}"
        );
    }

    #[test]
    fn action_rewrites_any_type_whole_as_string_and_keeps_null() {
        let cells = redact(
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
            cells,
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
        let cells = redact(
            &Redactor::with_defaults(),
            vec![Position::Action(PiiOperator::Replace, Entity::Person)],
            vec![json!("Jane Doe")],
        );
        assert_eq!(cells, [json!("<PERSON>")]);
    }

    #[test]
    fn whole_value_rewrites_are_counted_under_the_rule_entity() {
        let mut rows = vec![
            row(vec![json!("123-45-6789"), Value::Null]),
            row(vec![json!("987-65-4321"), json!("x")]),
        ];
        let stats = Redactor::with_defaults()
            .apply_cells(&mut rows, &plan(vec![MASKED, MASKED]))
            .expect("runs");
        assert_eq!(stats.total, 3);
        assert_eq!(stats.by_entity.get(&Entity::UsSsn), Some(&3));
    }

    #[test]
    fn global_matches_row_redaction_with_the_column_name_as_context() {
        let redactor = Redactor::with_defaults();
        let mut rows = vec![vec![("contact".to_string(), json!(EMAIL))]];
        redactor
            .apply_cells(&mut rows, &plan(vec![Position::Global]))
            .expect("runs");
        let mut objects = vec![json!({ "contact": EMAIL })];
        redactor.apply(&mut objects).expect("runs");
        assert_eq!(rows[0][0].1, objects[0]["contact"]);
        assert_ne!(rows[0][0].1, json!(EMAIL));
    }

    #[test]
    fn a_hashing_rule_token_equals_global_detection_token() {
        let redactor = keyed(PiiOperator::Hash, vec![hash_rule()]);
        let cells = redact(
            &redactor,
            vec![
                Position::Action(PiiOperator::Hash, Entity::EmailAddress),
                Position::Global,
            ],
            vec![json!(EMAIL), json!(EMAIL)],
        );
        assert_eq!(cells[0], cells[1]);
        assert!(
            cells[0].as_str().is_some_and(|t| t.starts_with("<EMAIL_ADDRESS_")),
            "{cells:?}"
        );
    }

    #[test]
    fn a_hashing_rule_under_another_global_operator_still_gets_a_key() {
        let redactor = keyed(PiiOperator::Mask, vec![hash_rule()]);
        let cells = redact(
            &redactor,
            vec![Position::Action(PiiOperator::Hash, Entity::EmailAddress)],
            vec![json!(EMAIL)],
        );
        assert!(
            cells[0].as_str().is_some_and(|t| t.starts_with("<EMAIL_ADDRESS_")),
            "{cells:?}"
        );
    }

    #[test]
    fn a_hashing_position_without_a_key_fails_closed() {
        let mut rows = vec![row(vec![json!(EMAIL)])];
        let err = Redactor::with_defaults()
            .apply_cells(
                &mut rows,
                &plan(vec![Position::Action(PiiOperator::Hash, Entity::EmailAddress)]),
            )
            .expect_err("no hash key");
        assert!(matches!(err, RedactionError::Internal(_)), "{err}");
    }

    #[tokio::test]
    async fn redact_cells_returns_objects_and_the_last_duplicate_wins() {
        let rows = vec![vec![
            ("x".to_string(), json!("123-45-6789")),
            ("note".to_string(), json!("plain")),
            ("x".to_string(), json!("987-65-4321")),
        ]];
        let out = Redactor::with_defaults()
            .redact_cells(rows, &plan(vec![MASKED, Position::Global, MASKED]))
            .await
            .expect("runs");
        assert_eq!(out, [json!({ "x": "***********", "note": "plain" })]);
    }
}

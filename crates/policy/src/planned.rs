//! A planned statement: its result checks, error withholding, and redaction.

use dbmcp_config::PiiConfig;
use dbmcp_pii::cells::{Cells, Plan, Position, ShapeError, check_columns};
use dbmcp_pii::{RedactionError, Redactor};
use dbmcp_sql_lineage::{Dialect, RelationId};
use serde_json::Value;

use crate::explain;
use crate::plan::{is_ruled, rule};

/// A planned statement, bound to the `pii.rules` it was planned under.
///
/// Only [`Analyzed::plan`](crate::Analyzed::plan) builds one.
#[derive(Debug)]
pub struct Planned<'c> {
    /// `None` for `EXPLAIN`, `SHOW`, and `DESCRIBE`: redacted globally.
    plan: Option<Plan>,
    config: &'c PiiConfig,
    withholds: bool,
    /// The dialect whose value-free plan keys `explainQuery`'s result keeps; `None` keeps all.
    prune: Option<Dialect>,
    /// Whether a first row passed [`Planned::check_first`].
    fit: bool,
}

/// Why a result does not fit its plan; names only a position.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unfit {
    /// A ruled column came back where the plan does not rewrite it.
    #[error("column policy: result column {0} carries a ruled column the plan does not rewrite")]
    Unplanned(usize),
    /// The result columns differ from the plan in count or name.
    #[error(transparent)]
    Shape(#[from] ShapeError),
}

impl<'c> Planned<'c> {
    pub(crate) const fn new(
        plan: Option<Plan>,
        config: &'c PiiConfig,
        withholds: bool,
        prune: Option<Dialect>,
    ) -> Self {
        Self {
            plan,
            config,
            withholds,
            prune,
            fit: false,
        }
    }

    /// Returns `code` when this plan withholds value-echoing errors.
    ///
    /// `code` is `None` for errors that cannot echo a value.
    #[must_use]
    pub fn withheld<C>(&self, code: Option<C>) -> Option<C> {
        code.filter(|_| self.withholds)
    }

    /// Checks the first row's names and origins against the plan.
    ///
    /// `column` is `None` for a whole-row origin. Non-positional plans check nothing.
    ///
    /// # Errors
    ///
    /// Returns [`Unfit::Unplanned`] for a ruled origin the plan does not rewrite, else [`Unfit::Shape`].
    pub fn check_first(
        &mut self,
        names: &[String],
        origins: impl IntoIterator<Item = (usize, RelationId, Option<String>)>,
    ) -> Result<(), Unfit> {
        if let Some(plan) = &self.plan {
            for (position, relation, column) in origins {
                let ruled = match column.as_deref() {
                    Some(column) => rule(self.config, &relation, column).is_some(),
                    None => is_ruled(&relation, self.config),
                };
                let rewritten = matches!(
                    plan.columns.get(position).map(|planned| planned.position),
                    Some(Position::Action(..))
                );
                if ruled && !rewritten {
                    return Err(Unfit::Unplanned(position));
                }
            }
            check_columns(plan, names)?;
        }
        self.fit = true;
        Ok(())
    }

    /// Redacts the rows by the plan and returns them as JSON objects.
    ///
    /// A pruned `EXPLAIN` plan keeps only value-free keys before global detection.
    ///
    /// # Errors
    ///
    /// Returns [`RedactionError::Internal`] for positional rows never checked
    /// or a pruned plan of unexpected shape, else the redactor's error.
    pub async fn redact(
        &self,
        redactor: &Redactor,
        names: Vec<String>,
        mut rows: Vec<Vec<Value>>,
    ) -> Result<Vec<Value>, RedactionError> {
        if let Some(dialect) = self.prune {
            explain::prune(dialect, &names, &mut rows)?;
        }
        if self.plan.is_some() && !self.fit && !rows.is_empty() {
            return Err(RedactionError::Internal(
                "column policy: result rows were not checked against the plan".into(),
            ));
        }
        redactor.redact_cells(Cells::new(names, rows), self.plan.as_ref()).await
    }
}

#[cfg(test)]
mod tests {
    use dbmcp_config::{ColumnRule, PiiOperator};
    use dbmcp_pii::Entity;
    use dbmcp_pii::cells::PlannedColumn;
    use serde_json::json;

    use super::*;

    /// A config ruling `shop.public.t.a`.
    fn ruled() -> PiiConfig {
        PiiConfig {
            rules: vec![ColumnRule {
                name: "r".into(),
                item: 0,
                key: ["shop", "public", "t", "a"].map(String::from),
                entity: "US_SSN",
                operator: PiiOperator::Mask,
            }],
            ..PiiConfig::default()
        }
    }

    /// A positional plan with `positions`, in order, names unknown.
    fn plan(positions: &[Position]) -> Plan {
        let columns = positions
            .iter()
            .map(|&position| PlannedColumn { position, name: None })
            .collect();
        Plan { columns }
    }

    /// `count` result column names.
    fn names(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("c{i}")).collect()
    }

    /// One origin at `position` from `shop.public.<table>.<column>`.
    fn origin(position: usize, table: &str, column: Option<&str>) -> (usize, RelationId, Option<String>) {
        (
            position,
            RelationId::new("shop", "public", table),
            column.map(Into::into),
        )
    }

    /// The redactor `config` builds.
    fn redactor(config: &PiiConfig) -> Redactor {
        Redactor::from_config(Some(config))
            .expect("builds")
            .expect("configured")
    }

    const ACTION: Position = Position::Action(PiiOperator::Mask, Entity::UsSsn);

    #[test]
    fn echoing_errors_are_withheld_only_when_the_plan_withholds() {
        let config = ruled();
        let withholding = Planned::new(None, &config, true, None);
        assert_eq!(withholding.withheld(Some("23505")), Some("23505"));
        assert_eq!(withholding.withheld(None::<&str>), None);
        let showing = Planned::new(None, &config, false, None);
        assert_eq!(showing.withheld(Some("23505")), None);
        assert_eq!(showing.withheld(None::<&str>), None);
    }

    #[test]
    fn ruled_origins_need_an_action_position() {
        let config = ruled();
        let mut planned = Planned::new(Some(plan(&[Position::Global, ACTION])), &config, true, None);
        assert_eq!(
            planned.check_first(&names(2), [origin(0, "t", Some("a"))]),
            Err(Unfit::Unplanned(0))
        );
        assert_eq!(planned.check_first(&names(2), [origin(1, "t", Some("a"))]), Ok(()));
    }

    #[test]
    fn positions_past_the_plan_admit_no_ruled_origin() {
        let config = ruled();
        let mut planned = Planned::new(Some(plan(&[ACTION])), &config, true, None);
        assert_eq!(
            planned.check_first(&names(1), [origin(1, "t", Some("a"))]),
            Err(Unfit::Unplanned(1))
        );
        assert_eq!(planned.check_first(&names(1), [origin(1, "t", Some("b"))]), Ok(()));
    }

    #[test]
    fn unruled_origins_are_admitted_anywhere() {
        let config = ruled();
        let mut planned = Planned::new(Some(plan(&[Position::Global])), &config, false, None);
        let origins = [
            origin(0, "t", Some("b")),
            origin(0, "u", Some("a")),
            origin(0, "u", None),
        ];
        assert_eq!(planned.check_first(&names(1), origins), Ok(()));
    }

    #[test]
    fn whole_rows_are_ruled_when_any_column_is() {
        let config = ruled();
        let mut global = Planned::new(Some(plan(&[Position::Global])), &config, true, None);
        assert_eq!(
            global.check_first(&names(1), [origin(0, "t", None)]),
            Err(Unfit::Unplanned(0))
        );
        let mut action = Planned::new(Some(plan(&[ACTION])), &config, true, None);
        assert_eq!(action.check_first(&names(1), [origin(0, "t", None)]), Ok(()));
    }

    #[test]
    fn origins_match_rules_case_folded() {
        let config = ruled();
        let mut planned = Planned::new(Some(plan(&[Position::Global])), &config, true, None);
        let origin = (0, RelationId::new("SHOP", "Public", "T"), Some("A".to_owned()));
        assert_eq!(planned.check_first(&names(1), [origin]), Err(Unfit::Unplanned(0)));
    }

    #[test]
    fn every_reading_of_a_position_must_be_admitted() {
        let config = ruled();
        let mut planned = Planned::new(Some(plan(&[Position::Global])), &config, true, None);
        let readings = [origin(0, "u", Some("a")), origin(0, "t", Some("a"))];
        assert_eq!(planned.check_first(&names(1), readings), Err(Unfit::Unplanned(0)));
    }

    #[test]
    fn origins_are_checked_before_the_shape() {
        let config = ruled();
        let mut planned = Planned::new(Some(plan(&[Position::Global])), &config, true, None);
        assert_eq!(
            planned.check_first(&names(2), [origin(0, "t", Some("a"))]),
            Err(Unfit::Unplanned(0))
        );
        assert_eq!(
            planned.check_first(&names(2), []),
            Err(Unfit::Shape(ShapeError::ColumnCount { expected: 1, actual: 2 }))
        );
    }

    #[test]
    fn non_positional_statements_check_nothing() {
        let config = ruled();
        let mut planned = Planned::new(None, &config, true, None);
        assert_eq!(planned.check_first(&names(3), [origin(0, "t", Some("a"))]), Ok(()));
    }

    #[tokio::test]
    async fn rows_never_checked_are_refused() {
        let config = ruled();
        let planned = Planned::new(Some(plan(&[ACTION])), &config, true, None);
        let refused = planned
            .redact(&redactor(&config), names(1), vec![vec![json!("123-45-6789")]])
            .await;
        assert!(matches!(refused, Err(RedactionError::Internal(_))), "{refused:?}");
        let empty = planned.redact(&redactor(&config), names(1), Vec::new()).await;
        assert_eq!(empty.expect("no rows to check"), Vec::<Value>::new());
    }

    #[tokio::test]
    async fn checked_rows_are_rewritten_by_position() {
        let config = ruled();
        let mut planned = Planned::new(Some(plan(&[ACTION, Position::Global])), &config, true, None);
        planned.check_first(&names(2), []).expect("fits");
        let rows = planned
            .redact(
                &redactor(&config),
                names(2),
                vec![vec![json!("123-45-6789"), json!("plain")]],
            )
            .await
            .expect("redacts");
        assert_eq!(rows, [json!({ "c0": "***********", "c1": "plain" })]);
    }

    #[tokio::test]
    async fn pruned_plans_are_still_detected_globally() {
        let config = ruled();
        let planned = Planned::new(None, &config, true, Some(Dialect::Postgres));
        let plan = json!([{"Plan": {"Alias": "ssn 123-45-6789", "Filter": "(a = 1)"}}]);
        let rows = planned
            .redact(&redactor(&config), vec!["QUERY PLAN".into()], vec![vec![plan]])
            .await
            .expect("redacts");
        let alias = &rows[0]["QUERY PLAN"][0]["Plan"];
        assert_eq!(alias.as_object().map(serde_json::Map::len), Some(1), "{rows:?}");
        assert_ne!(alias["Alias"], json!("ssn 123-45-6789"));
    }

    #[tokio::test]
    async fn pruning_refuses_unexpected_shapes() {
        let config = ruled();
        let planned = Planned::new(None, &config, true, Some(Dialect::Postgres));
        let refused = planned
            .redact(
                &redactor(&config),
                vec!["QUERY PLAN".into()],
                vec![vec![json!("Seq Scan")]],
            )
            .await;
        assert!(matches!(refused, Err(RedactionError::Internal(_))), "{refused:?}");
    }

    #[tokio::test]
    async fn non_positional_rows_are_detected_globally_unchecked() {
        let config = ruled();
        let planned = Planned::new(None, &config, true, None);
        let rows = planned
            .redact(
                &redactor(&config),
                vec!["Extra".into()],
                vec![vec![json!("ssn 123-45-6789")]],
            )
            .await
            .expect("redacts");
        assert_ne!(rows, [json!({ "Extra": "ssn 123-45-6789" })]);
    }
}

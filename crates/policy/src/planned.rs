//! A planned statement: its column plan bound to the rules it was planned under.

use dbmcp_config::PiiConfig;
use dbmcp_pii::cells::{Plan, Position};
use dbmcp_sql_lineage::RelationId;

use crate::plan::{is_ruled, rule};

/// A statement's column plan, bound to the `pii.rules` it was planned under.
///
/// Only [`Analyzed::plan`](crate::Analyzed::plan) builds one, so every plan
/// comes from an analyzed statement and answers origin checks under its own
/// rules.
#[derive(Debug)]
pub struct Planned<'c> {
    /// How each output column is redacted.
    plan: Plan,
    /// The config whose rules produced `plan`.
    config: &'c PiiConfig,
}

impl<'c> Planned<'c> {
    /// Binds `plan` to the config it was planned under.
    pub(crate) const fn new(plan: Plan, config: &'c PiiConfig) -> Self {
        Self { plan, config }
    }

    /// Returns the column plan, for shape checks and redaction.
    #[must_use]
    pub const fn plan(&self) -> &Plan {
        &self.plan
    }

    /// Reports whether a result column may come from `relation`'s `column` at `position`.
    ///
    /// A ruled origin is admitted only where the plan rewrites the column
    /// (a [`Position::Action`]); a position past the plan's end rewrites
    /// nothing. `column` is `None` for a whole-row reference, which is ruled
    /// when any column of `relation` is. Unruled origins are always admitted.
    #[must_use]
    pub fn admits(&self, position: usize, relation: &RelationId, column: Option<&str>) -> bool {
        let ruled = match column {
            Some(column) => rule(self.config, relation, column).is_some(),
            None => is_ruled(relation, self.config),
        };
        !ruled
            || matches!(
                self.plan.columns.get(position).map(|planned| planned.position),
                Some(Position::Action(..))
            )
    }
}

#[cfg(test)]
mod tests {
    use dbmcp_config::{ColumnRule, PiiOperator};
    use dbmcp_pii::Entity;
    use dbmcp_pii::cells::PlannedColumn;

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

    /// A plan with `positions`, in order.
    fn plan(positions: &[Position]) -> Plan {
        let columns = positions
            .iter()
            .map(|&position| PlannedColumn { position, name: None })
            .collect();
        Plan { columns }
    }

    const ACTION: Position = Position::Action(PiiOperator::Mask, Entity::UsSsn);

    #[test]
    fn ruled_origins_need_an_action_position() {
        let config = ruled();
        let planned = Planned::new(plan(&[Position::Global, ACTION]), &config);
        let t = RelationId::new("shop", "public", "t");
        assert!(!planned.admits(0, &t, Some("a")));
        assert!(planned.admits(1, &t, Some("a")));
    }

    #[test]
    fn positions_past_the_plan_admit_no_ruled_origin() {
        let config = ruled();
        let planned = Planned::new(plan(&[ACTION]), &config);
        let t = RelationId::new("shop", "public", "t");
        assert!(!planned.admits(1, &t, Some("a")));
        assert!(planned.admits(1, &t, Some("b")));
    }

    #[test]
    fn unruled_origins_are_admitted_anywhere() {
        let config = ruled();
        let planned = Planned::new(plan(&[Position::Global]), &config);
        assert!(planned.admits(0, &RelationId::new("shop", "public", "t"), Some("b")));
        assert!(planned.admits(0, &RelationId::new("shop", "public", "u"), Some("a")));
        assert!(planned.admits(0, &RelationId::new("shop", "public", "u"), None));
    }

    #[test]
    fn whole_rows_are_ruled_when_any_column_is() {
        let config = ruled();
        let t = RelationId::new("shop", "public", "t");
        assert!(!Planned::new(plan(&[Position::Global]), &config).admits(0, &t, None));
        assert!(Planned::new(plan(&[ACTION]), &config).admits(0, &t, None));
    }

    #[test]
    fn origins_match_rules_case_folded() {
        let config = ruled();
        let planned = Planned::new(plan(&[Position::Global]), &config);
        assert!(!planned.admits(0, &RelationId::new("SHOP", "Public", "T"), Some("A")));
    }
}

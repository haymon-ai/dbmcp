//! The catalog loop: analyzes a statement, asking the caller for missing relations.

use std::mem;

use dbmcp_config::{DatabaseBackend, PiiConfig};
use dbmcp_sql_lineage::{
    Catalog, Dialect, Entry, ExplainForm, Lexical, Lineage, LineageError, Origin, RelationId, StatementKind,
    WrittenName, analyze,
};

use crate::deny;
use crate::plan::{Facts, Names, plan, touches_ruled};
use crate::planned::{Mode, Planned};
use crate::rejection::{CATALOG_ROUNDS, Rejection, reject_lineage};
use crate::relation::RelationRef;

/// One statement's lineage analysis, driven by the backend's catalog reads.
///
/// Call [`Analysis::step`] with no entries first, then with an entry for
/// each name the previous step reported missing, until it returns
/// [`Step::Done`]. After [`CATALOG_ROUNDS`] rounds that still miss names it
/// rejects with [`Rejection::CatalogRounds`].
#[derive(Debug)]
pub struct Analysis<'a> {
    /// The statement under analysis.
    sql: &'a str,
    /// The dialect the statement is parsed in.
    dialect: Dialect,
    /// Every entry the backend supplied so far.
    catalog: Catalog,
    /// Rounds that reported missing names so far.
    rounds: usize,
    /// The database names are read against; see [`Analysis::connected_to`].
    database: Option<String>,
    /// See [`Names`].
    names: Names,
}

/// What an analysis step needs next.
#[derive(Debug)]
pub enum Step {
    /// The backend must look up these written names and pass an entry for each.
    ///
    /// Each comes with its dialect's [`RelationRef`]. Names that can name no
    /// relation never come: the analysis records them absent itself.
    Missing(Vec<(WrittenName, RelationRef)>),
    /// The analysis finished.
    Done(Analyzed),
}

/// A statement whose lineage analysis succeeded.
///
/// Only [`Analysis::step`] builds one, so every [`Planned`] comes from an
/// analyzed statement.
#[derive(Debug)]
pub struct Analyzed {
    /// The statement's lineage.
    lineage: Box<Lineage>,
    /// The dialect it was analyzed in.
    dialect: Dialect,
    /// See [`Names`].
    names: Names,
}

impl<'a> Analysis<'a> {
    /// Starts analyzing `sql` as `backend` parses it.
    #[must_use]
    pub fn new(sql: &'a str, backend: DatabaseBackend) -> Self {
        let dialect = match backend {
            DatabaseBackend::Mysql => Dialect::Mysql,
            DatabaseBackend::Mariadb => Dialect::Mariadb,
            DatabaseBackend::Postgres => Dialect::Postgres,
            DatabaseBackend::Sqlite => Dialect::Sqlite,
        };
        Self {
            sql,
            dialect,
            catalog: Catalog::new(),
            rounds: 0,
            database: None,
            names: Names::default(),
        }
    }

    /// Records the schema an unqualified `CREATE` lands in; `None` when unknown.
    pub fn creates_in(&mut self, schema: Option<String>) {
        self.names.creation = schema;
    }

    /// Records the database names are read against; `None` when there is none.
    ///
    /// `MySQL` and `MariaDB` pass the default database, which bare names are
    /// in; `PostgreSQL` passes the connected database, the only one a
    /// three-part name may qualify. `SQLite` reads no database.
    pub fn connected_to(&mut self, database: Option<String>) {
        self.database = database;
    }

    /// Records that the server lexes SQL unlike the parser, for `reason`.
    ///
    /// The next step then rejects with [`LineageClass::LexicalMismatch`](crate::LineageClass::LexicalMismatch).
    pub fn mismatch(&mut self, reason: impl Into<String>) {
        self.catalog.set_lexical(Lexical::Mismatch(reason.into()));
    }

    /// Adds `entries` to the catalog and analyzes the statement again.
    ///
    /// # Errors
    ///
    /// Returns [`Rejection::DeniedRelation`] when a missing name is denied,
    /// before any fetch; [`Rejection::CatalogRounds`] when names are still
    /// missing after [`CATALOG_ROUNDS`] rounds; and the lineage failure's
    /// class otherwise.
    /// Each rejection is logged.
    pub fn step(&mut self, entries: Vec<(WrittenName, Entry)>) -> Result<Step, Rejection> {
        for (name, mut entry) in entries {
            if let Entry::Resolved(relation) = &mut entry {
                if let (Origin::Query, [_]) = (&name.context, name.parts.as_slice()) {
                    self.names.bare.insert(relation.id.clone());
                }
                // Analysis ignores the family, so the planner takes it.
                self.names
                    .families
                    .insert(relation.id.clone(), mem::take(&mut relation.family));
                // Analysis still needs the columns, so the planner gets a copy of their facts.
                self.names.facts.insert(
                    relation.id.clone(),
                    Facts {
                        columns: relation.columns.clone(),
                    },
                );
            }
            self.catalog.insert(name, entry);
        }
        loop {
            match analyze(self.sql, self.dialect, &self.catalog) {
                Ok(lineage) => {
                    return Ok(Step::Done(Analyzed {
                        lineage: Box::new(lineage),
                        dialect: self.dialect,
                        names: std::mem::take(&mut self.names),
                    }));
                }
                Err(LineageError::Missing(names)) => {
                    let mut missing = Vec::new();
                    for name in names {
                        match RelationRef::new(self.dialect, self.database.as_deref(), &name) {
                            Some(reference) => missing.push((name, reference)),
                            // It names no relation, so there is nothing to fetch.
                            None => self.catalog.insert(name, Entry::Absent),
                        }
                    }
                    deny::names(self.dialect, &missing).map_err(Rejection::logged)?;
                    if self.rounds == CATALOG_ROUNDS {
                        return Err(Rejection::CatalogRounds.logged());
                    }
                    self.rounds += 1;
                    if !missing.is_empty() {
                        return Ok(Step::Missing(missing));
                    }
                }
                Err(error) => return Err(reject_lineage(&error)),
            }
        }
    }
}

impl Analyzed {
    /// Returns what kind of statement it is.
    #[must_use]
    pub fn statement(&self) -> &StatementKind {
        &self.lineage.statement
    }

    /// Returns the relation a DDL or DML statement targets, if any.
    #[must_use]
    pub fn target(&self) -> Option<&RelationId> {
        self.lineage.target.as_ref()
    }

    /// Plans the statement's output columns under `config`'s column rules.
    ///
    /// Runs every planner gate. `EXPLAIN`, `SHOW`, and `DESCRIBE` redact globally;
    /// a plain [`ExplainForm::Json`] or [`ExplainForm::QueryPlan`] plan touching a
    /// ruled relation is pruned first.
    ///
    /// # Errors
    ///
    /// Returns the first failing gate's [`Rejection`], logged.
    ///
    /// # Panics
    ///
    /// Panics when a rule's entity is not an [`Entity`](dbmcp_pii::Entity)
    /// name; the loader only accepts `ColumnRule::ENTITIES`.
    pub fn plan<'c>(&self, config: &'c PiiConfig) -> Result<Planned<'c>, Rejection> {
        let plan = plan(&self.lineage, self.dialect, config, &self.names)?;
        let statement = &self.lineage.statement;
        let touches = touches_ruled(&self.lineage, config, &self.names.families);
        // `PostgreSQL` runs `CREATE INDEX` outside the planning transaction, where names may rebind.
        let withholds = touches || (self.dialect == Dialect::Postgres && *statement == StatementKind::CreateIndex);
        let mode = match statement {
            StatementKind::Explain(ExplainForm::Json | ExplainForm::QueryPlan, _) if touches => {
                Mode::Pruned(self.dialect)
            }
            StatementKind::Explain(..) | StatementKind::Show(_) | StatementKind::Describe => Mode::Global,
            _ => Mode::Positional(plan),
        };
        Ok(Planned::new(mode, config, self.names.families.clone(), withholds))
    }
}

#[cfg(test)]
mod tests {
    use dbmcp_config::{ColumnRule, PiiOperator};
    use std::collections::BTreeSet;

    use dbmcp_sql_lineage::{CatalogColumn, Family, Member, Relation, RelationKind};

    use super::*;
    use crate::{LineageClass, Unfit};

    /// Answers every missing name with `shop.public.<last part>`, columns `a` and `b`.
    fn table(name: &WrittenName) -> Entry {
        let last = &name.parts.last().expect("a written name has parts").value;
        let id = RelationId::new("shop", "public", last.as_str());
        let columns = vec![CatalogColumn::new("a"), CatalogColumn::new("b")];
        Entry::Resolved(Relation::new(id, RelationKind::Table, columns))
    }

    /// Analyzes `sql`, creating in `creation` and resolving names with `entry`.
    fn analyzed_with(
        backend: DatabaseBackend,
        sql: &str,
        creation: Option<&str>,
        entry: impl Fn(&WrittenName) -> Entry,
    ) -> Analyzed {
        let mut analysis = Analysis::new(sql, backend);
        analysis.connected_to(Some("app".into()));
        analysis.creates_in(creation.map(Into::into));
        let mut entries = Vec::new();
        loop {
            match analysis.step(entries).expect("analysis") {
                Step::Missing(names) => {
                    entries = names
                        .into_iter()
                        .map(|(name, _)| {
                            let entry = entry(&name);
                            (name, entry)
                        })
                        .collect();
                }
                Step::Done(analyzed) => return analyzed,
            }
        }
    }

    /// Analyzes `sql` on `backend`, resolving every missing name with [`table`].
    fn analyzed_on(backend: DatabaseBackend, sql: &str) -> Analyzed {
        analyzed_with(backend, sql, Some("public"), table)
    }

    /// Analyzes `sql` on `PostgreSQL`.
    fn analyzed(sql: &str) -> Analyzed {
        analyzed_on(DatabaseBackend::Postgres, sql)
    }

    /// A config ruling `shop.public.t.a` with `mask` as `US_SSN`.
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

    #[test]
    fn missing_names_reject_after_the_round_cap() {
        let mut analysis = Analysis::new("SELECT a FROM t", DatabaseBackend::Sqlite);
        for _ in 0..CATALOG_ROUNDS {
            assert!(matches!(analysis.step(Vec::new()), Ok(Step::Missing(_))));
        }
        assert_eq!(analysis.step(Vec::new()).expect_err("cap"), Rejection::CatalogRounds);
    }

    #[test]
    fn names_that_name_no_relation_are_absent_without_a_fetch() {
        for (backend, sql) in [
            (DatabaseBackend::Mysql, "SELECT a FROM t"),
            (DatabaseBackend::Sqlite, "SELECT a FROM a.b.t"),
        ] {
            assert_eq!(
                Analysis::new(sql, backend).step(Vec::new()).expect_err("unresolved"),
                Rejection::Lineage(LineageClass::UnresolvedRelation),
                "{sql}"
            );
        }
    }

    #[test]
    fn supplied_entries_finish_the_analysis() {
        let analyzed = analyzed("SELECT a, b FROM t");
        assert_eq!(*analyzed.statement(), StatementKind::Query);
        assert_eq!(analyzed.target(), None);
    }

    #[test]
    fn a_lexical_mismatch_rejects() {
        let mut analysis = Analysis::new("SELECT 1", DatabaseBackend::Mysql);
        analysis.mismatch("NO_BACKSLASH_ESCAPES differs");
        assert_eq!(
            analysis.step(Vec::new()).expect_err("mismatch"),
            Rejection::Lineage(LineageClass::LexicalMismatch)
        );
    }

    /// The origin `shop.public.t.a` at `position`.
    fn ruled_origin(position: usize) -> (usize, RelationId, Option<String>) {
        (position, RelationId::new("shop", "public", "t"), Some("a".into()))
    }

    #[test]
    fn queries_plan_by_position() {
        let config = ruled();
        let mut planned = analyzed("SELECT a, b FROM t").plan(&config).expect("plan");
        let names = ["a", "b"].map(String::from);
        assert_eq!(planned.check_first(&names, [ruled_origin(0)]), Ok(()));
        assert_eq!(planned.check_first(&names, [ruled_origin(1)]), Err(Unfit::Unplanned(1)));
    }

    #[test]
    fn explain_show_and_describe_plan_globally() {
        let config = ruled();
        let names = ["x", "y", "z"].map(String::from);
        for sql in ["EXPLAIN SELECT b FROM u", "SHOW TABLES", "DESCRIBE u"] {
            let mut planned = analyzed_on(DatabaseBackend::Mysql, sql).plan(&config).expect("plan");
            assert_eq!(planned.check_first(&names, [ruled_origin(0)]), Ok(()), "{sql}");
        }
    }

    /// Plans `analyzed` under [`ruled`] and redacts one `PostgreSQL` plan holding a `Filter`.
    async fn explain_output(analyzed: &Analyzed) -> Vec<serde_json::Value> {
        let config = ruled();
        let redactor = dbmcp_pii::Redactor::from_config(Some(&config))
            .expect("builds")
            .expect("configured");
        let plan = serde_json::json!([{"Plan": {"Node Type": "Seq Scan", "Filter": "(b = 'x')"}}]);
        analyzed
            .plan(&config)
            .expect("plan")
            .redact(&redactor, vec!["QUERY PLAN".into()], vec![vec![plan]])
            .await
            .expect("redacts")
    }

    #[tokio::test]
    async fn only_plain_explain_of_ruled_relations_prunes() {
        let config = ruled();
        let pruned = serde_json::json!([{"QUERY PLAN": [{"Plan": {"Node Type": "Seq Scan"}}]}]);
        let kept = serde_json::json!([{"QUERY PLAN": [{"Plan": {"Node Type": "Seq Scan", "Filter": "(b = 'x')"}}]}]);
        let ruled = analyzed("EXPLAIN (FORMAT JSON) SELECT b FROM t");
        assert!(ruled.plan(&config).expect("plan").prunes());
        assert_eq!(explain_output(&ruled).await, pruned.as_array().expect("array").clone());
        for sql in [
            "EXPLAIN (FORMAT JSON) SELECT b FROM u",
            "EXPLAIN (ANALYZE, FORMAT JSON) SELECT b FROM u",
        ] {
            let unruled = analyzed(sql);
            assert!(!unruled.plan(&config).expect("plan").prunes(), "{sql}");
            assert_eq!(
                explain_output(&unruled).await,
                kept.as_array().expect("array").clone(),
                "{sql}"
            );
        }
    }

    #[test]
    fn statements_touching_a_ruled_relation_withhold_errors() {
        let config = ruled();
        let ruled = analyzed("SELECT b FROM t").plan(&config).expect("plan");
        assert_eq!(ruled.withheld(Some("23505")), Some("23505"));
        let other = analyzed("SELECT b FROM u").plan(&config).expect("plan");
        assert_eq!(other.withheld(Some("23505")), None);
    }

    #[test]
    fn every_postgres_create_index_withholds_errors() {
        let config = ruled();
        let index = "CREATE INDEX i ON u (a)";
        let postgres = analyzed(index).plan(&config).expect("plan");
        assert_eq!(postgres.withheld(Some("23505")), Some("23505"));
        let other = analyzed_on(DatabaseBackend::Sqlite, index);
        assert_eq!(other.plan(&config).expect("plan").withheld(Some("23505")), None);
    }

    #[test]
    fn bare_query_names_feed_the_create_target_gate() {
        let config = PiiConfig::default();
        let plans = |sql, creation, entry: fn(&WrittenName) -> Entry| {
            analyzed_with(DatabaseBackend::Postgres, sql, creation, entry)
                .plan(&config)
                .is_ok()
        };
        let vacant = |name: &WrittenName| match name.parts.last().map(|part| part.value.as_str()) {
            Some("c") => Entry::Vacant(RelationId::new("shop", "app", "c")),
            _ => table(name),
        };
        let bare = "CREATE TABLE c AS SELECT a FROM t";
        assert!(!plans(bare, Some("app"), table));
        assert!(plans(bare, Some("app"), vacant));
        assert!(plans("CREATE TABLE public.c AS SELECT a FROM t", Some("app"), table));
    }

    /// The relation the first step of `sql` on `backend` denies, if any.
    fn denied_first(backend: DatabaseBackend, sql: &str) -> Option<String> {
        let mut analysis = Analysis::new(sql, backend);
        analysis.connected_to(Some("app".into()));
        match analysis.step(Vec::new()) {
            Err(Rejection::DeniedRelation { relation }) => Some(relation),
            Ok(Step::Missing(_)) => None,
            other => panic!("{sql}: unexpected step {other:?}"),
        }
    }

    #[test]
    fn denied_written_names_reject_before_the_fetch() {
        let cases = [
            (
                DatabaseBackend::Mysql,
                "SELECT * FROM mysql.help_topic",
                "mysql.help_topic",
            ),
            (
                DatabaseBackend::Mariadb,
                "SELECT * FROM MySQL.Help_Topic",
                "MySQL.Help_Topic",
            ),
            (
                DatabaseBackend::Mysql,
                "SELECT * FROM `mysql`.`help_topic`",
                "mysql.help_topic",
            ),
            (
                DatabaseBackend::Mysql,
                "SELECT * FROM information_schema.innodb_locks",
                "information_schema.innodb_locks",
            ),
            (DatabaseBackend::Postgres, "SELECT * FROM pg_stats", "pg_stats"),
            (
                DatabaseBackend::Postgres,
                "SELECT * FROM pg_catalog.pg_stats",
                "pg_catalog.pg_stats",
            ),
            (
                DatabaseBackend::Postgres,
                "SELECT * FROM \"x\ny\".pg_stats",
                "x\ny.pg_stats",
            ),
            (
                DatabaseBackend::Sqlite,
                "SELECT * FROM main.sqlite_stat1",
                "main.sqlite_stat1",
            ),
        ];
        for (backend, sql, relation) in cases {
            assert_eq!(denied_first(backend, sql).as_deref(), Some(relation), "{sql}");
        }
    }

    #[test]
    fn bare_mysql_names_reject_in_the_default_database() {
        for backend in [DatabaseBackend::Mysql, DatabaseBackend::Mariadb] {
            let mut analysis = Analysis::new("SELECT * FROM help_topic", backend);
            analysis.connected_to(Some("mysql".into()));
            assert_eq!(
                analysis.step(Vec::new()).expect_err("denied"),
                Rejection::DeniedRelation {
                    relation: "help_topic".into()
                }
            );
        }
    }

    #[test]
    fn bare_names_in_a_view_body_reject_in_the_view_schema() {
        let mut analysis = Analysis::new("SELECT name FROM v", DatabaseBackend::Mysql);
        analysis.connected_to(Some("app".into()));
        let Ok(Step::Missing(names)) = analysis.step(Vec::new()) else {
            panic!("the view is missing first");
        };
        let view = Relation::new(
            RelationId::new("def", "mysql", "v"),
            RelationKind::View,
            vec![CatalogColumn::new("name")],
        )
        .with_view_sql("SELECT name FROM help_topic");
        let entries = names
            .into_iter()
            .map(|(name, _)| (name, Entry::Resolved(view.clone())))
            .collect();
        assert_eq!(
            analysis.step(entries).expect_err("denied"),
            Rejection::DeniedRelation {
                relation: "help_topic".into()
            }
        );
    }

    #[test]
    fn names_the_deny_tables_cannot_match_unresolved_are_fetched() {
        for sql in ["SELECT * FROM help_topic", "SELECT * FROM app.mysql_notes"] {
            assert_eq!(denied_first(DatabaseBackend::Mysql, sql), None, "{sql}");
        }
    }

    #[test]
    fn denied_names_in_a_view_body_reject_in_the_next_round() {
        let mut analysis = Analysis::new("SELECT host FROM v", DatabaseBackend::Mysql);
        analysis.connected_to(Some("app".into()));
        let Ok(Step::Missing(names)) = analysis.step(Vec::new()) else {
            panic!("the view is missing first");
        };
        let view = Relation::new(
            RelationId::new("def", "app", "v"),
            RelationKind::View,
            vec![CatalogColumn::new("host")],
        )
        .with_view_sql("SELECT host FROM mysql.user");
        let entries = names
            .into_iter()
            .map(|(name, _)| (name, Entry::Resolved(view.clone())))
            .collect();
        assert_eq!(
            analysis.step(entries).expect_err("denied"),
            Rejection::DeniedRelation {
                relation: "mysql.user".into()
            }
        );
    }

    /// Answers `t` with a parent of `t_1` and every other name with [`table`].
    fn parent(name: &WrittenName) -> Entry {
        let Entry::Resolved(relation) = table(name) else {
            unreachable!("table resolves");
        };
        if relation.id.name != "t" {
            return Entry::Resolved(relation);
        }
        let child = RelationId::new("shop", "public", "t_1");
        let family = Family {
            partitioned: true,
            ancestors: BTreeSet::new(),
            descendants: vec![Member {
                id: child,
                partitioned: false,
                ancestors: BTreeSet::from([relation.id.clone()]),
            }],
        };
        Entry::Resolved(relation.with_family(family))
    }

    /// A config ruling `shop.public.t_1.a` with `mask` as `US_SSN`.
    fn child_ruled() -> PiiConfig {
        let mut config = ruled();
        config.rules[0].key[2] = "t_1".into();
        config
    }

    #[test]
    fn resolved_families_reach_the_plan() {
        let config = child_ruled();
        let analyzed = analyzed_with(DatabaseBackend::Postgres, "SELECT a, b FROM t", Some("public"), parent);
        let mut planned = analyzed.plan(&config).expect("plan");
        let names = ["a", "b"].map(String::from);
        let parent = RelationId::new("shop", "public", "t");
        assert_eq!(
            planned.check_first(&names, [(1, parent.clone(), Some("a".into()))]),
            Err(Unfit::Unplanned(1))
        );
        assert_eq!(
            planned.check_first(&names, [(1, parent.clone(), None)]),
            Err(Unfit::Unplanned(1))
        );
        assert_eq!(planned.check_first(&names, [(0, parent, Some("a".into()))]), Ok(()));
        assert_eq!(planned.withheld(Some("23505")), Some("23505"));
    }

    #[test]
    fn backends_map_to_their_dialects() {
        let mut analysis = Analysis::new("SELECT 1", DatabaseBackend::Mariadb);
        assert_eq!(analysis.dialect, Dialect::Mariadb);
        analysis = Analysis::new("SELECT 1", DatabaseBackend::Mysql);
        assert_eq!(analysis.dialect, Dialect::Mysql);
    }
}

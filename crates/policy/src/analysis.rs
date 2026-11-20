//! The catalog loop: analyzes a statement, asking the caller for missing relations.

use dbmcp_config::{DatabaseBackend, PiiConfig};
use dbmcp_sql_lineage::{
    Catalog, Dialect, Entry, Lexical, Lineage, LineageError, Origin, RelationId, StatementKind, WrittenName, analyze,
};

use crate::deny;
use crate::plan::{Names, plan, touches_ruled};
use crate::planned::Planned;
use crate::rejection::{CATALOG_ROUNDS, Rejection, reject_lineage};

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
    /// See [`Names`].
    names: Names,
}

/// What an analysis step needs next.
#[derive(Debug)]
pub enum Step {
    /// The backend must look up these written names and pass an entry for each.
    Missing(Vec<WrittenName>),
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
            names: Names::default(),
        }
    }

    /// Records the schema an unqualified `CREATE` lands in; `None` when unknown.
    pub fn creates_in(&mut self, schema: Option<String>) {
        self.names.creation = schema;
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
        for (name, entry) in entries {
            if let (Origin::Query, [_], Entry::Resolved(relation)) = (&name.context, name.parts.as_slice(), &entry) {
                self.names.bare.insert(relation.id.clone());
            }
            self.catalog.insert(name, entry);
        }
        match analyze(self.sql, self.dialect, &self.catalog) {
            Ok(lineage) => Ok(Step::Done(Analyzed {
                lineage: Box::new(lineage),
                dialect: self.dialect,
                names: std::mem::take(&mut self.names),
            })),
            Err(LineageError::Missing(names))
                if let Some(relation) = names.iter().find_map(|name| deny::written(self.dialect, name)) =>
            {
                Err(Rejection::DeniedRelation { relation }.logged())
            }
            Err(LineageError::Missing(_)) if self.rounds == CATALOG_ROUNDS => Err(Rejection::CatalogRounds.logged()),
            Err(LineageError::Missing(names)) => {
                self.rounds += 1;
                Ok(Step::Missing(names))
            }
            Err(error) => Err(reject_lineage(&error)),
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
    /// Runs every planner gate. `EXPLAIN`, `SHOW`, and `DESCRIBE` redact globally.
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
        let positional = !matches!(
            statement,
            StatementKind::Explain(_) | StatementKind::Show(_) | StatementKind::Describe
        );
        // `PostgreSQL` runs `CREATE INDEX` outside the planning transaction, where names may rebind.
        let withholds = touches_ruled(&self.lineage, config)
            || (self.dialect == Dialect::Postgres && *statement == StatementKind::CreateIndex);
        Ok(Planned::new(positional.then_some(plan), config, withholds))
    }
}

#[cfg(test)]
mod tests {
    use dbmcp_config::{ColumnRule, PiiOperator};
    use dbmcp_sql_lineage::{CatalogColumn, Relation, RelationKind};

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
        analysis.creates_in(creation.map(Into::into));
        let mut entries = Vec::new();
        loop {
            match analysis.step(entries).expect("analysis") {
                Step::Missing(names) => entries = names.into_iter().map(|name| (name.clone(), entry(&name))).collect(),
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
        match Analysis::new(sql, backend).step(Vec::new()) {
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
                "x\\ny.pg_stats",
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
    fn names_the_deny_tables_cannot_match_unresolved_are_fetched() {
        for sql in ["SELECT * FROM help_topic", "SELECT * FROM app.mysql_notes"] {
            assert_eq!(denied_first(DatabaseBackend::Mysql, sql), None, "{sql}");
        }
    }

    #[test]
    fn denied_names_in_a_view_body_reject_in_the_next_round() {
        let mut analysis = Analysis::new("SELECT host FROM v", DatabaseBackend::Mysql);
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
            .map(|name| (name, Entry::Resolved(view.clone())))
            .collect();
        assert_eq!(
            analysis.step(entries).expect_err("denied"),
            Rejection::DeniedRelation {
                relation: "mysql.user".into()
            }
        );
    }

    #[test]
    fn backends_map_to_their_dialects() {
        let mut analysis = Analysis::new("SELECT 1", DatabaseBackend::Mariadb);
        assert_eq!(analysis.dialect, Dialect::Mariadb);
        analysis = Analysis::new("SELECT 1", DatabaseBackend::Mysql);
        assert_eq!(analysis.dialect, Dialect::Mysql);
    }
}

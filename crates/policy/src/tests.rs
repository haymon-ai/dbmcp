//! Unit tests for the planner gates, with hand-built lineage.

use std::collections::BTreeSet;

use dbmcp_config::{ColumnRule, PiiConfig, PiiOperator};
use dbmcp_sql_lineage as lineage;
use dbmcp_sql_lineage::{
    Catalog, Dialect, Entry, ExplainForm, Flow, FunctionUse, Lineage, LineageError, NamePart, Origin, OutputColumn,
    Reference, Relation, RelationId, RelationKind, RelationUse, Role, Show, SourceRef, StatementKind, TargetColumn,
    WrittenName, analyze,
};

use dbmcp_pii::Entity;
use dbmcp_pii::cells::{Plan, Position};

use crate::plan::{Effects, Facts, Families, Names, effects, plan, touches_ruled};
use crate::rejection::reject_lineage;
use crate::{Cascade, CascadeAction, CatalogColumn, Family, LineageClass, Member, Rejection};

const DB: &str = "shop";
const SCHEMA: &str = "public";

/// Builds `shop.public.<table>`.
fn table(name: &str) -> RelationId {
    RelationId::new(DB, SCHEMA, name)
}

/// Builds `shop.public.<table>.<column>`.
fn col(table_name: &str, column: &str) -> SourceRef {
    SourceRef::new(table(table_name), column)
}

/// Builds a rule `r` for one `catalog.schema.table.column` key, folded like the loader.
fn rule(key: &str, operator: PiiOperator, entity: &'static str) -> ColumnRule {
    let segments: Vec<String> = key.split('.').map(ColumnRule::fold).collect();
    ColumnRule {
        name: "r".into(),
        item: 0,
        key: segments.try_into().expect("four segments"),
        entity,
        operator,
    }
}

/// A `mask` rule labelled `US_SSN`.
fn ssn(key: &str) -> ColumnRule {
    rule(key, PiiOperator::Mask, "US_SSN")
}

/// Builds a config holding `rules`, with global defaults.
fn config(rules: Vec<ColumnRule>) -> PiiConfig {
    PiiConfig {
        rules,
        ..PiiConfig::default()
    }
}

/// A query lineage selecting one output column per source set.
fn select(columns: Vec<Vec<SourceRef>>) -> Lineage {
    let relations = columns
        .iter()
        .flatten()
        .map(|source| RelationUse {
            id: source.relation.clone(),
            kind: RelationKind::Table,
            origin: Origin::Query,
        })
        .collect();
    Lineage {
        columns: columns
            .into_iter()
            .enumerate()
            .map(|(position, sources)| OutputColumn {
                name: format!("c{position}"),
                position,
                sources: sources.into_iter().collect(),
                flow: Flow::Bare,
            })
            .collect(),
        references: BTreeSet::new(),
        relations,
        functions: BTreeSet::new(),
        target: None,
        writes: Vec::new(),
        statement: StatementKind::Query,
    }
}

/// Plans `lineage` without bare names.
fn plan_on(lineage: &Lineage, dialect: Dialect, config: &PiiConfig) -> Result<Plan, Rejection> {
    plan(lineage, dialect, config, &Names::default())
}

/// Plans `lineage` on `PostgreSQL`.
fn pg(lineage: &Lineage, config: &PiiConfig) -> Result<Plan, Rejection> {
    plan_on(lineage, Dialect::Postgres, config)
}

/// Analyzes `sql` on `PostgreSQL` against `catalog` and plans it.
fn pg_sql(sql: &str, catalog: &Catalog, config: &PiiConfig) -> Result<Plan, Rejection> {
    pg(&analyze(sql, Dialect::Postgres, catalog).expect(sql), config)
}

/// Plans `lineage` on `PostgreSQL` under `names`.
fn pg_with(lineage: &Lineage, config: &PiiConfig, names: &Names) -> Result<Plan, Rejection> {
    plan(lineage, Dialect::Postgres, config, names)
}

/// The single position of a one-column plan.
fn only(lineage: &Lineage, config: &PiiConfig) -> Position {
    let positions = positions(pg(lineage, config).expect("plan"));
    assert_eq!(positions.len(), 1);
    positions.into_iter().next().expect("one position")
}

/// The positions of `plan`, in output order.
fn positions(plan: Plan) -> Vec<Position> {
    plan.columns.into_iter().map(|column| column.position).collect()
}

/// The masked `US_SSN` position.
const MASKED_SSN: Position = Position::Action(PiiOperator::Mask, Entity::UsSsn);

// Rule match.

#[test]
fn rule_matches_case_insensitively() {
    let config = config(vec![ssn("SHOP.Public.USERS.Ssn")]);
    assert_eq!(only(&select(vec![vec![col("users", "ssn")]]), &config), MASKED_SSN);
}

#[test]
fn exact_key_matches_only_its_column() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = select(vec![
        vec![col("users", "ssn")],
        vec![col("users", "ssn_hash")],
        vec![col("orders", "ssn")],
    ]);
    assert_eq!(
        positions(pg(&lineage, &config).expect("plan")),
        [MASKED_SSN, Position::Global, Position::Global]
    );
}

#[test]
fn case_fold_matches_unicode_like_the_loader() {
    let config = config(vec![ssn("shop.public.ÄRZTE.ssn")]);
    assert_eq!(only(&select(vec![vec![col("ärzte", "ssn")]]), &config), MASKED_SSN);
}

#[test]
fn rule_for_another_engine_tuple_never_matches() {
    // A MySQL relation has catalog `def`; a PG-shaped rule names the database.
    let config = config(vec![ssn("shop.shop.users.ssn")]);
    let source = SourceRef::new(RelationId::new("def", "shop", "users"), "ssn");
    let plan = plan_on(&select(vec![vec![source]]), Dialect::Mysql, &config).expect("plan");
    assert_eq!(positions(plan), [Position::Global]);
}

#[test]
fn ner_only_entity_labels_a_rule() {
    let config = config(vec![rule("shop.public.users.name", PiiOperator::Replace, "PERSON")]);
    assert_eq!(
        only(&select(vec![vec![col("users", "name")]]), &config),
        Position::Action(PiiOperator::Replace, Entity::Person)
    );
}

// Lineage errors.

#[test]
fn lineage_errors_map_to_classes_without_names() {
    let cases = [
        (LineageError::Parse("near 'secret'".into()), LineageClass::Parse),
        (LineageError::MultiStatement, LineageClass::MultiStatement),
        (
            LineageError::ColumnNotFound {
                column: "secret".into(),
            },
            LineageClass::Column,
        ),
        (
            LineageError::AmbiguousColumn {
                column: "secret".into(),
            },
            LineageClass::Column,
        ),
        (
            LineageError::ColumnCountMismatch {
                relation: "secret".into(),
                expected: 1,
                found: 2,
            },
            LineageClass::Column,
        ),
        (
            LineageError::UnknownRelation { name: "secret".into() },
            LineageClass::UnresolvedRelation,
        ),
        (LineageError::Unsupported("secret".into()), LineageClass::Unsupported),
        (
            LineageError::LexicalMismatch("secret".into()),
            LineageClass::LexicalMismatch,
        ),
        (LineageError::DepthLimit, LineageClass::Recursion),
        (
            LineageError::CircularReference { name: "secret".into() },
            LineageClass::Recursion,
        ),
        (
            LineageError::UnknownTableFunction { name: "secret".into() },
            LineageClass::UnknownTableFunction,
        ),
    ];
    for (error, class) in cases {
        let rejection = reject_lineage(&error);
        assert_eq!(rejection, Rejection::Lineage(class));
        let message = rejection.to_string();
        assert!(message.starts_with("column policy: "), "{message}");
        assert!(!message.contains("secret"), "{message}");
    }
}

// Unknown functions.

#[test]
fn unknown_function_rejects_in_any_origin() {
    let config = config(vec![]);
    for origin in [Origin::Query, Origin::View(table("v"))] {
        let mut lineage = select(vec![vec![]]);
        lineage.functions.insert(FunctionUse {
            name: "leak".into(),
            known: false,
            origin,
        });
        assert_eq!(
            pg(&lineage, &config),
            Err(Rejection::UnknownFunction { name: "leak".into() })
        );
    }
}

#[test]
fn known_function_passes() {
    let mut lineage = select(vec![vec![col("users", "name")]]);
    lineage.functions.insert(FunctionUse {
        name: "lower".into(),
        known: true,
        origin: Origin::Query,
    });
    assert!(pg(&lineage, &config(vec![])).is_ok());
}

// Denied relations.

/// Plans a lineage touching one relation.
fn touching(dialect: Dialect, id: RelationId, kind: RelationKind) -> Result<Plan, Rejection> {
    let mut lineage = select(vec![vec![]]);
    lineage.relations.insert(RelationUse {
        id,
        kind,
        origin: Origin::View(table("v")),
    });
    plan_on(&lineage, dialect, &config(vec![]))
}

#[test]
fn denied_relations_reject_per_backend() {
    let denied = [
        (Dialect::Postgres, "pg_catalog", "pg_statistic"),
        (Dialect::Postgres, "pg_catalog", "pg_stats"),
        (Dialect::Postgres, "pg_catalog", "pg_stats_ext"),
        (Dialect::Postgres, "pg_catalog", "pg_stats_ext_exprs"),
        (Dialect::Postgres, "pg_catalog", "pg_statistic_ext_data"),
        (Dialect::Postgres, "pg_catalog", "pg_stat_activity"),
        (Dialect::Postgres, "public", "pg_stat_statements"),
        (Dialect::Postgres, "pg_catalog", "pg_largeobject"),
        (Dialect::Postgres, "pg_catalog", "pg_largeobject_metadata"),
        (Dialect::Postgres, "pg_catalog", "pg_settings"),
        (Dialect::Postgres, "pg_catalog", "pg_file_settings"),
        (Dialect::Postgres, "pg_catalog", "pg_db_role_setting"),
        (Dialect::Mysql, "information_schema", "COLUMN_STATISTICS"),
        (Dialect::Mysql, "information_schema", "PROCESSLIST"),
        (Dialect::Mysql, "performance_schema", "events_statements_current"),
        (Dialect::Mysql, "performance_schema", "events_statements_history_long"),
        (Dialect::Mysql, "performance_schema", "threads"),
        (Dialect::Mysql, "information_schema", "INNODB_TRX"),
        (Dialect::Mysql, "information_schema", "INNODB_LOCKS"),
        (Dialect::Mariadb, "information_schema", "SLAVE_STATUS"),
        (Dialect::Mysql, "performance_schema", "data_locks"),
        (Dialect::Mysql, "performance_schema", "prepared_statements_instances"),
        (Dialect::Mysql, "performance_schema", "user_variables_by_thread"),
        (Dialect::Mysql, "sys", "innodb_lock_waits"),
        (Dialect::Mysql, "sys", "x$innodb_lock_waits"),
        (Dialect::Mysql, "sys", "statement_analysis"),
        (Dialect::Mysql, "sys", "x$processlist"),
        (Dialect::Mysql, "sys", "session"),
        (Dialect::Mysql, "mysql", "user"),
        (Dialect::Mariadb, "information_schema", "PROCESSLIST"),
        (Dialect::Mariadb, "performance_schema", "events_statements_current"),
        (Dialect::Mariadb, "mysql", "column_stats"),
        (Dialect::Mysql, "information_schema", "INNODB_FT_INDEX_CACHE"),
        (Dialect::Mysql, "information_schema", "INNODB_FT_DELETED"),
        (
            Dialect::Mysql,
            "performance_schema",
            "replication_applier_status_by_worker",
        ),
        (Dialect::Sqlite, "main", "sqlite_stat1"),
        (Dialect::Sqlite, "main", "sqlite_stat4"),
        (Dialect::Sqlite, "main", "sqlite_dbpage"),
    ];
    for (dialect, schema, name) in denied {
        let catalog = if dialect == Dialect::Postgres { DB } else { "def" };
        let id = RelationId::new(catalog, schema, name);
        assert_eq!(
            touching(dialect, id, RelationKind::System),
            Err(Rejection::DeniedRelation {
                relation: format!("{schema}.{name}"),
            }),
            "{dialect:?} {schema}.{name}"
        );
    }
}

#[test]
fn sqlite_virtual_relation_rejects() {
    let id = RelationId::new("main", "main", "docs_fts");
    assert!(matches!(
        touching(Dialect::Sqlite, id, RelationKind::Virtual),
        Err(Rejection::DeniedRelation { .. })
    ));
}

#[test]
fn ordinary_relations_pass() {
    let allowed = [
        (Dialect::Postgres, RelationId::new(DB, "pg_catalog", "pg_class")),
        (Dialect::Mysql, RelationId::new("def", "information_schema", "TABLES")),
        (
            Dialect::Mysql,
            RelationId::new("def", "performance_schema", "events_waits_current"),
        ),
        (Dialect::Mysql, RelationId::new("def", "shop", "mysql")),
        (Dialect::Sqlite, RelationId::new("main", "main", "users")),
    ];
    for (dialect, id) in allowed {
        assert!(touching(dialect, id.clone(), RelationKind::Table).is_ok(), "{id:?}");
    }
}

// Denied statements.

#[test]
fn denied_show_statements_reject() {
    let cases = [
        (Dialect::Mysql, Show::Warnings),
        (Dialect::Mariadb, Show::Errors),
        (Dialect::Mysql, Show::Unsupported),
        (Dialect::Mariadb, Show::Unsupported),
        (Dialect::Postgres, Show::Tables),
        (Dialect::Postgres, Show::Variables),
        (Dialect::Postgres, Show::Unsupported),
    ];
    for (dialect, form) in cases {
        let mut lineage = select(vec![]);
        lineage.statement = StatementKind::Show(form);
        assert_eq!(
            plan_on(&lineage, dialect, &config(vec![])),
            Err(Rejection::DeniedStatement),
            "{dialect:?} {form:?}"
        );
    }
}

#[test]
fn harmless_show_passes() {
    let cases = [
        (Dialect::Mysql, Show::Tables),
        (Dialect::Mariadb, Show::CreateTable),
        (Dialect::Sqlite, Show::Tables),
        (Dialect::Sqlite, Show::Warnings),
        (Dialect::Sqlite, Show::Unsupported),
    ];
    for (dialect, form) in cases {
        let mut lineage = select(vec![]);
        lineage.statement = StatementKind::Show(form);
        assert!(
            plan_on(&lineage, dialect, &config(vec![])).is_ok(),
            "{dialect:?} {form:?}"
        );
    }
}

#[test]
fn analyzed_schema_show_forms_pass() {
    let cases = [
        ("SHOW DATABASES", Dialect::Mysql),
        ("SHOW FULL TABLES", Dialect::Mysql),
        ("SHOW TABLE STATUS", Dialect::Mariadb),
        ("SHOW OPEN TABLES", Dialect::Mysql),
        ("SHOW FULL COLUMNS FROM users", Dialect::Mysql),
        ("SHOW INDEX FROM users", Dialect::Mysql),
        ("SHOW KEYS FROM users", Dialect::Mariadb),
        ("SHOW CREATE TABLE users", Dialect::Mysql),
        ("SHOW TRIGGERS", Dialect::Mysql),
        ("SHOW EVENTS", Dialect::Mariadb),
        ("SHOW PROCEDURE STATUS", Dialect::Mysql),
        ("SHOW FUNCTION STATUS", Dialect::Mysql),
        ("SHOW GLOBAL VARIABLES LIKE 'max%'", Dialect::Mysql),
        ("SHOW SESSION STATUS", Dialect::Mariadb),
        ("SHOW CHARACTER SET", Dialect::Mysql),
        ("SHOW COLLATION", Dialect::Mysql),
        ("SHOW STORAGE ENGINES", Dialect::Mariadb),
        ("SHOW PLUGINS", Dialect::Mysql),
        ("SHOW PRIVILEGES", Dialect::Mysql),
    ];
    for (sql, dialect) in cases {
        let lineage = analyze(sql, dialect, &Catalog::new()).expect(sql);
        assert!(
            plan_on(&lineage, dialect, &config(vec![])).is_ok(),
            "{sql}: {:?}",
            lineage.statement
        );
    }
}

#[test]
fn explain_of_show_unwraps_to_the_show_deny() {
    let mut lineage = select(vec![]);
    lineage.statement = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::Show(Show::Warnings)));
    assert_eq!(
        plan_on(&lineage, Dialect::Mysql, &config(vec![])),
        Err(Rejection::DeniedStatement)
    );
}

#[test]
fn analyzed_value_show_forms_reject() {
    let cases = [
        ("SHOW BINLOG EVENTS", Dialect::Mysql),
        ("SHOW RELAYLOG EVENTS", Dialect::Mysql),
        ("SHOW EXPLAIN FOR 1", Dialect::Mariadb),
        ("SHOW ANALYZE FOR 1", Dialect::Mariadb),
        ("SHOW REPLICA STATUS", Dialect::Mysql),
        ("SHOW SLAVE STATUS", Dialect::Mariadb),
        ("SHOW SESSION SLAVE STATUS", Dialect::Mariadb),
        ("SHOW ALL SLAVES STATUS", Dialect::Mariadb),
        ("SHOW REPLICAS", Dialect::Mysql),
        ("SHOW ENGINE INNODB STATUS", Dialect::Mysql),
        ("SHOW FULL PROCESSLIST", Dialect::Mariadb),
        ("SHOW COUNT(*) WARNINGS", Dialect::Mysql),
        ("SHOW PROFILES", Dialect::Mysql),
        ("SHOW GRANTS", Dialect::Mariadb),
        ("SHOW USER_VARIABLES", Dialect::Mariadb),
        ("SHOW `user_variables`", Dialect::Mariadb),
        ("SHOW MASTER STATUS", Dialect::Mysql),
        ("SHOW `TABLES`", Dialect::Mysql),
        ("SHOW TABLE foo", Dialect::Mysql),
        ("SHOW FUNCTION xyz", Dialect::Mysql),
    ];
    for (sql, dialect) in cases {
        let lineage = analyze(sql, dialect, &Catalog::new()).expect(sql);
        assert!(
            plan_on(&lineage, dialect, &config(vec![])) == Err(Rejection::DeniedStatement),
            "{sql}: {:?}",
            lineage.statement
        );
    }
}

// EXPLAIN.

#[test]
fn explain_of_ruled_relation_rejects() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let mut lineage = select(vec![vec![col("users", "name")]]);
    lineage.statement = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::Query));
    assert_eq!(
        pg(&lineage, &config),
        Err(Rejection::Explain {
            relation: "public.users".into(),
        })
    );
}

#[test]
fn explain_of_relation_only_reached_through_a_view_rejects() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let mut lineage = select(vec![vec![]]);
    lineage.relations.insert(RelationUse {
        id: table("users"),
        kind: RelationKind::Table,
        origin: Origin::View(table("v")),
    });
    lineage.statement = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::Query));
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Explain { .. })));
}

#[test]
fn explain_wrapping_ddl_on_ruled_relation_rejects() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let mut lineage = select(vec![]);
    lineage.relations.insert(RelationUse {
        id: table("users"),
        kind: RelationKind::Table,
        origin: Origin::Query,
    });
    lineage.target = Some(table("users"));
    lineage.statement = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::Drop));
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Explain { .. })));
}

#[test]
fn explain_of_unruled_relation_passes() {
    let config = config(vec![ssn("shop.public.audit.x")]);
    let mut lineage = select(vec![vec![col("users", "name")]]);
    lineage.statement = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::Query));
    assert!(pg(&lineage, &config).is_ok());
}

/// An `EXPLAIN` in `form` of a query selecting `users.name`.
fn explained_users(form: ExplainForm) -> Lineage {
    let mut lineage = select(vec![vec![col("users", "name")]]);
    lineage.statement = StatementKind::Explain(form, Box::new(StatementKind::Query));
    lineage
}

#[test]
fn explain_analyze_of_ruled_relation_rejects() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let rejection = pg(&explained_users(ExplainForm::Analyze), &config).expect_err("rejected");
    assert_eq!(
        rejection,
        Rejection::ExplainAnalyze {
            relation: "public.users".into(),
        }
    );
    assert_eq!(rejection.gate(), "explain-analyze");
    assert_eq!(
        rejection.to_string(),
        "column policy: EXPLAIN ANALYZE on ruled relation \"public.users\" is denied; actual row counts can reveal values, explain without ANALYZE"
    );
}

#[test]
fn other_explain_of_ruled_relation_points_to_explain_query() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let rejection = pg(&explained_users(ExplainForm::Other), &config).expect_err("rejected");
    assert_eq!(rejection.gate(), "explain");
    assert_eq!(
        rejection.to_string(),
        "column policy: EXPLAIN touches ruled relation \"public.users\"; use explainQuery"
    );
}

#[test]
fn plain_explain_of_ruled_relation_passes() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    assert!(pg(&explained_users(ExplainForm::Json), &config).is_ok());
    assert!(plan_on(&explained_users(ExplainForm::QueryPlan), Dialect::Sqlite, &config).is_ok());
}

#[test]
fn plain_explain_still_runs_the_reference_gate() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    for origin in [Origin::Query, Origin::View(table("v"))] {
        let mut lineage = referencing(col("users", "ssn"), Role::Filter, origin);
        lineage.statement = StatementKind::Explain(ExplainForm::Json, Box::new(StatementKind::Query));
        assert!(matches!(
            pg(&lineage, &config),
            Err(Rejection::Reference { role: "WHERE", .. })
        ));
    }
}

#[test]
fn plain_explain_of_ruled_create_index_rejects() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let mut lineage = ddl(StatementKind::CreateIndex, "users");
    lineage.statement = StatementKind::Explain(ExplainForm::Json, Box::new(StatementKind::CreateIndex));
    assert!(matches!(pg(&lineage, &config), Err(Rejection::RuledIndex { .. })));
}

#[test]
fn touches_ruled_needs_a_ruled_relation() {
    let lineage = select(vec![vec![col("users", "name")]]);
    let touches = |rules| touches_ruled(&lineage, &config(rules), &Families::new());
    assert!(!touches(vec![]));
    assert!(!touches(vec![ssn("shop.public.audit.x")]));
    assert!(touches(vec![ssn("shop.public.users.ssn")]));
}

// CREATE INDEX.

/// A relations-only lineage of `statement` on `target`.
fn ddl(statement: StatementKind, target: &str) -> Lineage {
    let mut lineage = select(vec![]);
    lineage.relations.insert(RelationUse {
        id: table(target),
        kind: RelationKind::Table,
        origin: Origin::Query,
    });
    lineage.target = Some(table(target));
    lineage.statement = statement;
    lineage
}

#[test]
fn create_index_on_ruled_relation_rejects() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    assert_eq!(
        pg(&ddl(StatementKind::CreateIndex, "users"), &config),
        Err(Rejection::RuledIndex {
            relation: "public.users".into(),
        })
    );
    assert!(pg(&ddl(StatementKind::CreateIndex, "orders"), &config).is_ok());
}

#[test]
fn relations_only_ddl_on_unruled_relation_plans_empty() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    for statement in [StatementKind::Drop, StatementKind::Truncate, StatementKind::CreateTable] {
        assert_eq!(
            pg(&ddl(statement.clone(), "orders"), &config),
            Ok(Plan { columns: Vec::new() }),
            "{statement:?}"
        );
    }
}

// References.

/// A lineage selecting `users.id` and using `source` as `role`.
fn referencing(source: SourceRef, role: Role, origin: Origin) -> Lineage {
    let mut lineage = select(vec![vec![col("users", "id")]]);
    lineage.references.insert(Reference { source, role, origin });
    lineage
}

/// Every role with its SQL wording.
const ROLES: [(Role, &str); 5] = [
    (Role::Join, "JOIN"),
    (Role::Filter, "WHERE"),
    (Role::GroupBy, "GROUP BY"),
    (Role::Sort, "ORDER BY"),
    (Role::Window, "a window"),
];

#[test]
fn ruled_reference_rejects_in_every_role() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    for (role, written) in ROLES {
        let lineage = referencing(col("users", "ssn"), role, Origin::Query);
        assert_eq!(
            pg(&lineage, &config),
            Err(Rejection::Reference {
                column: "public.users.ssn".into(),
                role: written,
                view: None,
            })
        );
    }
}

#[test]
fn ruled_reference_in_view_body_rejects_in_every_role() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    for (role, written) in ROLES {
        let lineage = referencing(col("users", "ssn"), role, Origin::View(table("v")));
        assert_eq!(
            pg(&lineage, &config),
            Err(Rejection::Reference {
                column: "public.users.ssn".into(),
                role: written,
                view: Some("public.v".into()),
            })
        );
    }
}

#[test]
fn view_origin_reference_names_the_view() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = referencing(col("users", "ssn"), Role::Filter, Origin::View(table("v")));
    let rejection = pg(&lineage, &config).expect_err("rejected");
    assert_eq!(
        rejection.to_string(),
        "column policy: column \"public.users.ssn\" (ruled) used in WHERE in view \"public.v\""
    );
}

#[test]
fn unruled_references_are_unrestricted() {
    let config = config(vec![ssn("shop.public.audit.x")]);
    assert!(pg(&referencing(col("users", "name"), Role::Join, Origin::Query), &config).is_ok());
}

// Combine.

#[test]
fn sourceless_and_unruled_positions_are_global() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = select(vec![vec![], vec![col("users", "name")]]);
    assert_eq!(
        positions(pg(&lineage, &config).expect("plan")),
        [Position::Global, Position::Global]
    );
}

#[test]
fn ruled_source_wins_over_unruled() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = select(vec![vec![col("users", "ssn"), col("users", "name")]]);
    assert_eq!(only(&lineage, &config), MASKED_SSN);
}

#[test]
fn different_actions_conflict() {
    let config = config(vec![
        ssn("shop.public.users.ssn"),
        rule("shop.public.users.email", PiiOperator::Redact, "US_SSN"),
    ]);
    let lineage = select(vec![vec![col("users", "ssn"), col("users", "email")]]);
    assert_eq!(pg(&lineage, &config), Err(Rejection::Conflict { position: 0 }));
}

#[test]
fn same_operator_different_entity_conflicts() {
    let config = config(vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.users.phone", PiiOperator::Hash, "PHONE_NUMBER"),
    ]);
    let lineage = select(vec![vec![col("users", "email"), col("users", "phone")]]);
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Conflict { .. })));
}

#[test]
fn same_rule_pair_combines() {
    let config = config(vec![
        rule("shop.public.users.firstname", PiiOperator::Hash, "PERSON"),
        rule("shop.public.users.lastname", PiiOperator::Hash, "PERSON"),
    ]);
    let lineage = select(vec![vec![col("users", "firstname"), col("users", "lastname")]]);
    assert_eq!(
        only(&lineage, &config),
        Position::Action(PiiOperator::Hash, Entity::Person)
    );
}

// Flow: `hash` only on bare outputs; derived outputs read as the placeholder.

/// The `hash` rule on `users.email` and `invoices.billing_email`, `EMAIL_ADDRESS`.
fn hashed_emails() -> Vec<ColumnRule> {
    vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.invoices.billing_email", PiiOperator::Hash, "EMAIL_ADDRESS"),
    ]
}

/// A one-column query lineage over `sources`, reaching the output by `flow`.
fn flowed(sources: Vec<SourceRef>, flow: Flow) -> Lineage {
    let mut lineage = select(vec![sources]);
    lineage.columns[0].flow = flow;
    lineage
}

const HASHED_EMAIL: Position = Position::Action(PiiOperator::Hash, Entity::EmailAddress);
const REPLACED_EMAIL: Position = Position::Action(PiiOperator::Replace, Entity::EmailAddress);
const PLACEHOLDER_EMAIL: Position = Position::Placeholder(Entity::EmailAddress);

#[test]
fn bare_hash_output_keeps_hash() {
    let config = config(hashed_emails());
    assert_eq!(
        only(&flowed(vec![col("users", "email")], Flow::Bare), &config),
        HASHED_EMAIL
    );
}

#[test]
fn bare_union_of_same_hash_pair_keeps_hash() {
    let config = config(hashed_emails());
    let lineage = flowed(
        vec![col("users", "email"), col("invoices", "billing_email")],
        Flow::Bare,
    );
    assert_eq!(only(&lineage, &config), HASHED_EMAIL);
}

#[test]
fn derived_ruled_output_reads_as_the_placeholder_under_every_operator() {
    let config = config(vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.users.phone", PiiOperator::Mask, "PHONE_NUMBER"),
        rule("shop.public.users.ssn", PiiOperator::Redact, "US_SSN"),
        rule("shop.public.users.card", PiiOperator::Replace, "CREDIT_CARD"),
    ]);
    let cases = [
        ("email", Entity::EmailAddress),
        ("phone", Entity::PhoneNumber),
        ("ssn", Entity::UsSsn),
        ("card", Entity::CreditCard),
    ];
    for (column, entity) in cases {
        assert_eq!(
            only(&flowed(vec![col("users", column)], Flow::Derived), &config),
            Position::Placeholder(entity),
            "{column}"
        );
    }
}

#[test]
fn derived_output_with_an_unruled_source_reads_as_the_placeholder() {
    let config = config(hashed_emails());
    let lineage = flowed(vec![col("users", "name"), col("users", "email")], Flow::Derived);
    assert_eq!(only(&lineage, &config), PLACEHOLDER_EMAIL);
}

#[test]
fn bare_union_with_unruled_column_becomes_replace() {
    let config = config(hashed_emails());
    let lineage = flowed(vec![col("users", "email"), col("users", "name")], Flow::Bare);
    assert_eq!(only(&lineage, &config), REPLACED_EMAIL);
}

#[test]
fn bare_union_with_another_rule_merges_into_replace() {
    let config = config(vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule(
            "shop.public.invoices.billing_email",
            PiiOperator::Replace,
            "EMAIL_ADDRESS",
        ),
    ]);
    let lineage = flowed(
        vec![col("users", "email"), col("invoices", "billing_email")],
        Flow::Bare,
    );
    assert_eq!(only(&lineage, &config), REPLACED_EMAIL);
}

#[test]
fn derived_hash_of_different_entities_conflicts() {
    let config = config(vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.users.phone", PiiOperator::Hash, "PHONE_NUMBER"),
    ]);
    let lineage = flowed(vec![col("users", "email"), col("users", "phone")], Flow::Derived);
    assert_eq!(pg(&lineage, &config), Err(Rejection::Conflict { position: 0 }));
}

#[test]
fn derived_same_entity_under_different_operators_reads_as_the_placeholder() {
    let config = config(vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.invoices.billing_email", PiiOperator::Mask, "EMAIL_ADDRESS"),
    ]);
    let lineage = flowed(
        vec![col("users", "email"), col("invoices", "billing_email")],
        Flow::Derived,
    );
    assert_eq!(only(&lineage, &config), PLACEHOLDER_EMAIL);
}

#[test]
fn derived_output_without_ruled_source_stays_global() {
    let config = config(hashed_emails());
    assert_eq!(
        only(&flowed(vec![col("users", "name")], Flow::Derived), &config),
        Position::Global
    );
}

/// A catalog of `shop.public` tables keyed by their bare names.
fn tables(tables: &[(&str, &[&str])]) -> Catalog {
    let mut catalog = Catalog::new();
    for (name, columns) in tables {
        let columns = columns.iter().map(|c| lineage::CatalogColumn::new(*c)).collect();
        catalog.insert(
            WrittenName::query(vec![NamePart::bare(*name)]),
            Entry::Resolved(Relation::new(table(name), RelationKind::Table, columns)),
        );
    }
    catalog
}

#[test]
fn analyzed_outputs_hash_only_when_bare_and_read_derived_as_the_placeholder() {
    let catalog = tables(&[
        ("users", &["id", "email", "name"]),
        ("invoices", &["id", "user_id", "billing_email"]),
    ]);
    let config = config(hashed_emails());
    let cases = [
        ("SELECT email FROM users", HASHED_EMAIL),
        ("SELECT (u.email) AS e FROM users u", HASHED_EMAIL),
        ("SELECT e FROM (SELECT email AS e FROM users) d", HASHED_EMAIL),
        ("WITH c AS (SELECT email FROM users) SELECT * FROM c", HASHED_EMAIL),
        (
            "SELECT email FROM users UNION ALL SELECT billing_email FROM invoices",
            HASHED_EMAIL,
        ),
        ("SELECT email FROM users UNION ALL SELECT NULL", HASHED_EMAIL),
        (
            "SELECT u.email FROM invoices i LEFT JOIN users u ON u.id = i.user_id",
            HASHED_EMAIL,
        ),
        (
            "SELECT email FROM users UNION ALL SELECT name FROM users",
            REPLACED_EMAIL,
        ),
        ("SELECT email FROM users UNION ALL SELECT 'a@b.c'", PLACEHOLDER_EMAIL),
        (
            "SELECT CASE WHEN email IS NULL THEN 'x' ELSE 'x' END FROM users",
            PLACEHOLDER_EMAIL,
        ),
        ("SELECT lower(email) FROM users", PLACEHOLDER_EMAIL),
        ("SELECT min(email) FROM users", PLACEHOLDER_EMAIL),
        ("SELECT count(DISTINCT email) FROM users", PLACEHOLDER_EMAIL),
        ("SELECT CAST(email AS text) FROM users", PLACEHOLDER_EMAIL),
        ("SELECT email COLLATE \"C\" FROM users", PLACEHOLDER_EMAIL),
        (
            "SELECT concat(u.email, i.billing_email) FROM users u, invoices i",
            PLACEHOLDER_EMAIL,
        ),
    ];
    for (sql, expected) in cases {
        let positions = positions(pg_sql(sql, &catalog, &config).expect(sql));
        assert_eq!(positions, [expected], "{sql}");
    }
}

#[test]
fn row_count_uses_of_ruled_columns_reject() {
    let catalog = tables(&[("users", &["id", "email", "ssn", "name"])]);
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let plan = |sql: &str| pg_sql(sql, &catalog, &config);
    for (sql, role) in [
        ("SELECT DISTINCT ssn FROM users", "DISTINCT or a set operation"),
        (
            "SELECT CASE WHEN ssn LIKE '1%' THEN 1 ELSE 2 END FROM users WHERE id = 5 UNION SELECT 1",
            "DISTINCT or a set operation",
        ),
        (
            "SELECT count(*) FROM (SELECT DISTINCT lower(ssn) FROM users) d",
            "DISTINCT or a set operation",
        ),
        (
            "SELECT ssn FROM users EXCEPT ALL SELECT name FROM users",
            "DISTINCT or a set operation",
        ),
        (
            "SELECT generate_series(1, ascii(ssn)) FROM users WHERE id = 1",
            "a set-returning function",
        ),
        (
            "SELECT count(*) FROM (SELECT string_to_table(ssn, NULL) FROM users) t",
            "a set-returning function",
        ),
    ] {
        let rejection = plan(sql).expect_err(sql);
        assert_eq!(
            rejection,
            Rejection::Reference {
                column: "public.users.ssn".into(),
                role,
                view: None,
            },
            "{sql}"
        );
        assert_eq!(
            rejection.to_string(),
            format!(r#"column policy: column "public.users.ssn" (ruled) used in {role}"#),
            "{sql}"
        );
    }
    for sql in [
        "SELECT ssn FROM users UNION ALL SELECT name FROM users",
        "SELECT DISTINCT name FROM users",
        "SELECT name FROM users UNION SELECT 'x'",
        "SELECT generate_series(1, id), ssn FROM users",
    ] {
        assert!(plan(sql).is_ok(), "{sql}");
    }
}

#[test]
fn conflict_message_omits_output_name() {
    let config = config(vec![
        ssn("shop.public.users.ssn"),
        rule("shop.public.users.email", PiiOperator::Redact, "EMAIL_ADDRESS"),
    ]);
    let mut lineage = select(vec![vec![col("users", "ssn"), col("users", "email")]]);
    lineage.columns[0].name = "CASE WHEN ssn = '123-45-6789' THEN ssn ELSE email END".into();
    let message = pg(&lineage, &config).expect_err("rejected").to_string();
    assert!(!message.contains("123-45-6789"), "{message}");
}

// Shadowed create targets.

/// Plans a create of `public.copy` with `names`, without rules.
fn create(statement: StatementKind, names: &Names) -> Result<Plan, Rejection> {
    let lineage = write("copy", "x", vec![col("users", "name")], statement);
    pg_with(&lineage, &config(vec![]), names)
}

/// Names creating in `creation`, with `bare` resolved bare.
fn names(creation: Option<&str>, bare: &[&str]) -> Names {
    Names {
        creation: creation.map(Into::into),
        bare: bare.iter().map(|name| table(name)).collect(),
        ..Names::default()
    }
}

#[test]
fn bare_creates_must_resolve_in_the_creation_schema() {
    let shadowed = Err(Rejection::ShadowedTarget {
        relation: "public.copy".into(),
    });
    for statement in [
        StatementKind::CreateTableAs,
        StatementKind::CreateView,
        StatementKind::CreateMaterializedView,
    ] {
        for (creation, bare, rejected) in [
            (Some(SCHEMA), &["copy"][..], false),
            (Some("app"), &["copy"], true),
            (Some("app"), &["users"], false),
            (None, &["copy"], true),
            (None, &[], false),
        ] {
            let planned = create(statement.clone(), &names(creation, bare));
            assert_eq!(planned == shadowed, rejected, "{statement:?} {creation:?} {bare:?}");
        }
    }
}

#[test]
fn other_statements_skip_the_create_target_gate() {
    let shadowing = names(Some("app"), &["copy"]);
    for statement in [StatementKind::Insert, StatementKind::Update, StatementKind::CreateTable] {
        assert!(create(statement.clone(), &shadowing).is_ok(), "{statement:?}");
    }
    for statement in [StatementKind::CreateIndex, StatementKind::Drop] {
        let lineage = ddl(statement.clone(), "copy");
        assert!(pg_with(&lineage, &config(vec![]), &shadowing).is_ok(), "{statement:?}");
    }
    let explained = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::CreateTableAs));
    assert!(create(explained, &shadowing).is_ok());
}

#[test]
fn shadowed_target_rejects_before_laundering() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = write("copy", "x", vec![col("users", "ssn")], StatementKind::CreateTableAs);
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Laundering { .. })));
    let rejection = pg_with(&lineage, &config, &names(Some("app"), &["copy"])).expect_err("shadowed");
    assert_eq!(
        rejection.to_string(),
        "column policy: create target \"public.copy\" resolves to another relation than it creates"
    );
    assert_eq!(rejection.gate(), "target");
}

// Write laundering.

/// A write lineage into `target` writing `column` (named) from `sources`.
fn write(target: &str, column: &str, sources: Vec<SourceRef>, statement: StatementKind) -> Lineage {
    let mut lineage = select(vec![]);
    for source in &sources {
        lineage.relations.insert(RelationUse {
            id: source.relation.clone(),
            kind: RelationKind::Table,
            origin: Origin::Query,
        });
    }
    lineage.target = Some(table(target));
    lineage.writes = vec![TargetColumn {
        name: column.into(),
        sources: sources.into_iter().collect(),
        flow: Flow::Bare,
        named: true,
    }];
    lineage.statement = statement;
    lineage
}

#[test]
fn derived_hash_source_into_same_hash_target_rejects() {
    let config = config(vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.copy.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
    ]);
    let mut lineage = write("copy", "email", vec![col("users", "email")], StatementKind::Insert);
    let names = facts(&[("copy", &[("email", NONE)])]);
    assert!(pg_with(&lineage, &config, &names).is_ok());
    lineage.writes[0].flow = Flow::Derived;
    assert_eq!(
        pg_with(&lineage, &config, &names),
        Err(Rejection::Laundering {
            column: "public.copy.email".into(),
        })
    );
}

#[test]
fn ruled_source_into_unruled_target_rejects() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = write("copy", "x", vec![col("users", "ssn")], StatementKind::Insert);
    assert_eq!(
        pg(&lineage, &config),
        Err(Rejection::Laundering {
            column: "public.copy.x".into(),
        })
    );
}

#[test]
fn ruled_source_into_same_pair_target_passes() {
    let config = config(vec![ssn("shop.public.users.ssn"), ssn("shop.public.copy.x")]);
    let lineage = write("copy", "x", vec![col("users", "ssn")], StatementKind::Update);
    assert!(pg(&lineage, &config).is_ok());
}

#[test]
fn ruled_source_into_other_pair_target_rejects() {
    let lineage = write("copy", "x", vec![col("users", "ssn")], StatementKind::Insert);
    for target in [
        rule("shop.public.copy.x", PiiOperator::Redact, "US_SSN"),
        rule("shop.public.copy.x", PiiOperator::Mask, "EMAIL_ADDRESS"),
    ] {
        let config = config(vec![ssn("shop.public.users.ssn"), target]);
        assert!(matches!(pg(&lineage, &config), Err(Rejection::Laundering { .. })));
    }
}

#[test]
fn literal_only_write_passes() {
    let config = config(vec![ssn("shop.public.copy.x")]);
    let lineage = write("copy", "x", vec![], StatementKind::Insert);
    assert!(pg(&lineage, &config).is_ok());
}

#[test]
fn unruled_source_into_ruled_target_passes() {
    let config = config(vec![ssn("shop.public.copy.x")]);
    let lineage = write("copy", "x", vec![col("users", "name")], StatementKind::Insert);
    assert!(pg(&lineage, &config).is_ok());
}

#[test]
fn ctas_of_ruled_data_needs_a_target_rule() {
    let lineage = write(
        "new_users",
        "ssn",
        vec![col("users", "ssn")],
        StatementKind::CreateTableAs,
    );
    assert!(matches!(
        pg(&lineage, &config(vec![ssn("shop.public.users.ssn")])),
        Err(Rejection::Laundering { .. })
    ));
    let prewritten = config(vec![ssn("shop.public.users.ssn"), ssn("shop.public.new_users.ssn")]);
    assert!(pg(&lineage, &prewritten).is_ok());
}

#[test]
fn create_view_is_not_laundering() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = write("v", "ssn", vec![col("users", "ssn")], StatementKind::CreateView);
    assert!(pg(&lineage, &config).is_ok());
}

#[test]
fn create_materialized_view_of_ruled_data_needs_a_target_rule() {
    let lineage = write(
        "mv",
        "ssn",
        vec![col("users", "ssn")],
        StatementKind::CreateMaterializedView,
    );
    assert!(matches!(
        pg(&lineage, &config(vec![ssn("shop.public.users.ssn")])),
        Err(Rejection::Laundering { .. })
    ));
    let prewritten = config(vec![ssn("shop.public.users.ssn"), ssn("shop.public.mv.ssn")]);
    assert!(pg(&lineage, &prewritten).is_ok());
}

#[test]
fn unnamed_created_columns_ask_for_an_alias_only_when_bare() {
    let mut catalog = tables(&[("users", &["id", "ssn"])]);
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("copy")]),
        Entry::Vacant(table("copy")),
    );
    let config = config(vec![ssn("shop.public.users.ssn"), ssn("shop.public.copy.ssn")]);
    let plan = |sql: &str| pg_sql(sql, &catalog, &config);

    assert_eq!(
        plan("CREATE TABLE copy AS SELECT (ssn) FROM users"),
        Err(Rejection::Unnamed {
            relation: "public.copy".into(),
            position: 0,
        })
    );
    let rejection = plan("CREATE TABLE copy AS SELECT upper(ssn) FROM users").expect_err("computed");
    assert_eq!(
        rejection,
        Rejection::UnnamedLaundering {
            relation: "public.copy".into(),
            position: 0,
        }
    );
    assert_eq!(
        rejection.to_string(),
        r#"column policy: writing created column 0 of "public.copy" would weaken the rules of its source columns"#
    );
    assert_eq!(
        plan("CREATE TABLE copy AS SELECT upper(ssn) AS ssn FROM users"),
        Err(Rejection::Laundering {
            column: "public.copy.ssn".into(),
        })
    );
}

#[test]
fn ruled_source_into_unnamed_create_column_asks_for_an_alias() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let mut lineage = write("t2", "(ssn)", vec![col("users", "ssn")], StatementKind::CreateTableAs);
    lineage.writes[0].named = false;
    let rejection = pg(&lineage, &config).expect_err("rejected");
    assert_eq!(
        rejection,
        Rejection::Unnamed {
            relation: "public.t2".into(),
            position: 0,
        }
    );
    let message = rejection.to_string();
    assert!(message.contains("alias the column"), "{message}");
    assert!(!message.contains("(ssn)"), "{message}");

    lineage.writes[0].sources = BTreeSet::from([col("users", "name")]);
    assert!(pg(&lineage, &config).is_ok(), "unruled data needs no name");
}

#[test]
fn derived_same_pair_writes_launder_and_bare_copies_pass() {
    let mut catalog = tables(&[("masked", &["id", "phone", "note"]), ("copy", &["id", "phone"])]);
    catalog.insert(
        WrittenName::query(vec![NamePart::bare("mv")]),
        Entry::Vacant(table("mv")),
    );
    let cases = [
        (
            PiiOperator::Mask,
            "INSERT INTO copy (phone) SELECT CASE WHEN phone LIKE '+1%' THEN NULL ELSE '1' END FROM masked",
            "INSERT INTO copy (phone) SELECT phone FROM masked",
            "public.copy.phone",
        ),
        (
            PiiOperator::Redact,
            "UPDATE masked SET note = CASE WHEN phone LIKE '+1%' THEN NULL ELSE 'x' END",
            "UPDATE masked SET note = phone",
            "public.masked.note",
        ),
        (
            PiiOperator::Replace,
            "INSERT INTO copy (id, phone) SELECT id, phone FROM masked \
             ON CONFLICT (id) DO UPDATE SET phone = lower(excluded.phone)",
            "INSERT INTO copy (id, phone) SELECT id, phone FROM masked \
             ON CONFLICT (id) DO UPDATE SET phone = excluded.phone",
            "public.copy.phone",
        ),
        (
            PiiOperator::Mask,
            "MERGE INTO copy c USING masked m ON c.id = m.id WHEN MATCHED THEN UPDATE SET phone = upper(m.phone)",
            "MERGE INTO copy c USING masked m ON c.id = m.id WHEN MATCHED THEN UPDATE SET phone = m.phone",
            "public.copy.phone",
        ),
        (
            PiiOperator::Mask,
            "CREATE MATERIALIZED VIEW mv AS SELECT lower(phone) AS phone FROM masked",
            "CREATE MATERIALIZED VIEW mv AS SELECT phone FROM masked",
            "public.mv.phone",
        ),
    ];
    for (operator, derived, bare, column) in cases {
        let config = config(
            ["masked.phone", "masked.note", "copy.phone", "mv.phone"]
                .map(|key| rule(&format!("shop.public.{key}"), operator, "PHONE_NUMBER"))
                .to_vec(),
        );
        assert_eq!(
            pg_sql(derived, &catalog, &config),
            Err(Rejection::Laundering { column: column.into() }),
            "{derived}"
        );
        assert!(pg_sql(bare, &catalog, &config).is_ok(), "{bare}");
    }
}

#[test]
fn conflicting_write_sources_reject() {
    let config = config(vec![
        ssn("shop.public.users.ssn"),
        rule("shop.public.users.email", PiiOperator::Redact, "EMAIL_ADDRESS"),
        ssn("shop.public.copy.x"),
    ]);
    let lineage = write(
        "copy",
        "x",
        vec![col("users", "ssn"), col("users", "email")],
        StatementKind::Insert,
    );
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Laundering { .. })));
}

// Writes into `hash`-ruled columns.

/// Column facts `(default, generated, on_update)`.
type ColumnFacts = (bool, bool, bool);

const NONE: ColumnFacts = (false, false, false);
const DEFAULT: ColumnFacts = (true, false, false);
const GENERATED: ColumnFacts = (false, true, false);
const ON_UPDATE: ColumnFacts = (false, false, true);

/// Names carrying the write facts of trigger-free `shop.public` tables without cascades.
fn facts(tables: &[(&str, &[(&str, ColumnFacts)])]) -> Names {
    Names {
        facts: tables
            .iter()
            .map(|(name, columns)| {
                let columns = columns
                    .iter()
                    .map(|&(column, (default, generated, on_update))| CatalogColumn {
                        name: column.into(),
                        star_visible: true,
                        default,
                        generated,
                        on_update,
                    })
                    .collect();
                (
                    table(name),
                    Facts {
                        columns,
                        triggers: false,
                        cascades: Vec::new(),
                    },
                )
            })
            .collect(),
        ..Names::default()
    }
}

/// `users.email` and `hashed.email` hashed as `EMAIL_ADDRESS`, `users.ssn` masked.
fn hashed_target() -> PiiConfig {
    config(vec![
        rule("shop.public.users.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.hashed.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        ssn("shop.public.users.ssn"),
    ])
}

/// Plans a one-column write into `hashed.email` reaching it by `flow` from `sources`.
fn into_hashed(
    statement: StatementKind,
    sources: Vec<SourceRef>,
    flow: Flow,
    email: ColumnFacts,
) -> Result<Plan, Rejection> {
    let mut lineage = write("hashed", "email", sources, statement);
    lineage.writes[0].flow = flow;
    let names = facts(&[("hashed", &[("id", DEFAULT), ("email", email)])]);
    pg_with(&lineage, &hashed_target(), &names)
}

/// The `Chosen` rejection of `public.hashed.email`.
fn chosen() -> Result<Plan, Rejection> {
    Err(Rejection::Chosen {
        column: "public.hashed.email".into(),
    })
}

/// The `Computed` rejection of `public.hashed.email`.
fn computed() -> Result<Plan, Rejection> {
    computed_at("hashed.email")
}

#[test]
fn same_hash_bare_copy_into_hash_target_passes() {
    for statement in [StatementKind::Insert, StatementKind::Update, StatementKind::Merge] {
        let planned = into_hashed(statement.clone(), vec![col("users", "email")], Flow::Bare, NONE);
        assert!(planned.is_ok(), "{statement:?}");
    }
}

#[test]
fn null_into_hash_target_passes() {
    assert!(into_hashed(StatementKind::Insert, vec![], Flow::Null, NONE).is_ok());
    assert!(into_hashed(StatementKind::Update, vec![], Flow::Null, DEFAULT).is_ok());
}

#[test]
fn default_into_hash_target_passes_only_without_a_default() {
    for statement in [StatementKind::Insert, StatementKind::Update] {
        assert!(into_hashed(statement.clone(), vec![], Flow::Default, NONE).is_ok());
        assert_eq!(
            into_hashed(statement.clone(), vec![], Flow::Default, DEFAULT),
            computed(),
            "{statement:?}"
        );
    }
}

#[test]
fn chosen_values_into_hash_target_reject() {
    for (sources, flow) in [
        (vec![], Flow::Derived),
        (vec![col("users", "name")], Flow::Bare),
        (vec![col("users", "name")], Flow::Derived),
    ] {
        for statement in [StatementKind::Insert, StatementKind::Update, StatementKind::Merge] {
            assert_eq!(
                into_hashed(statement.clone(), sources.clone(), flow, NONE),
                chosen(),
                "{statement:?} {sources:?} {flow:?}"
            );
        }
    }
}

#[test]
fn derived_or_other_ruled_source_into_hash_target_launders() {
    let laundering = Err(Rejection::Laundering {
        column: "public.hashed.email".into(),
    });
    for (sources, flow) in [
        (vec![col("users", "email")], Flow::Derived),
        (vec![col("users", "ssn")], Flow::Bare),
    ] {
        assert_eq!(
            into_hashed(StatementKind::Insert, sources, flow, NONE),
            laundering,
            "{flow:?}"
        );
    }
}

/// Plans `statement` into `hashed`, writing only `id`, with `email` carrying `email` facts.
fn beside_hashed(statement: StatementKind, email: ColumnFacts) -> Result<Plan, Rejection> {
    let lineage = write("hashed", "id", vec![], statement);
    let names = facts(&[("hashed", &[("id", DEFAULT), ("email", email)])]);
    pg_with(&lineage, &hashed_target(), &names)
}

#[test]
fn omitted_hash_column_passes_only_without_a_default() {
    assert!(beside_hashed(StatementKind::Insert, NONE).is_ok());
    assert_eq!(beside_hashed(StatementKind::Insert, DEFAULT), computed());
    assert!(beside_hashed(StatementKind::Update, DEFAULT).is_ok());
}

#[test]
fn every_merge_may_omit_a_hash_column() {
    assert!(beside_hashed(StatementKind::Merge, NONE).is_ok());
    assert_eq!(beside_hashed(StatementKind::Merge, DEFAULT), computed());
    let written = into_hashed(StatementKind::Merge, vec![col("users", "email")], Flow::Bare, DEFAULT);
    assert_eq!(written, computed());
}

#[test]
fn inserts_writing_a_defaulted_hash_column_still_refuse_since_upserts_may_omit_it() {
    for (sources, flow) in [(vec![], Flow::Null), (vec![col("users", "email")], Flow::Bare)] {
        let written = into_hashed(StatementKind::Insert, sources, flow, DEFAULT);
        assert_eq!(written, computed(), "{flow:?}");
    }
    assert!(into_hashed(StatementKind::Update, vec![], Flow::Null, DEFAULT).is_ok());
}

#[test]
fn hash_columns_missing_from_the_facts_have_every_fact() {
    // A descendant's own column is not among the target's facts.
    let config = config(vec![rule(
        "shop.public.ptab_1.email",
        PiiOperator::Hash,
        "EMAIL_ADDRESS",
    )]);
    let mut names = tree();
    names.facts = facts(&[("ptab", &[("id", NONE)])]).facts;
    let lineage = write("ptab", "id", vec![], StatementKind::Update);
    assert_eq!(
        pg_with(&lineage, &config, &names),
        Err(Rejection::Computed {
            column: "public.ptab.email".into(),
        })
    );
}

#[test]
fn defaults_match_exact_names_before_folded_ones() {
    let config = hashed_target();
    let mut lineage = write("hashed", "email", vec![], StatementKind::Update);
    lineage.writes[0].flow = Flow::Default;
    let plans = |columns: &[(&str, ColumnFacts)]| pg_with(&lineage, &config, &facts(&[("hashed", columns)]));
    assert!(plans(&[("Email", DEFAULT), ("email", NONE)]).is_ok());
    assert_eq!(plans(&[("email", DEFAULT), ("Email", NONE)]), computed());
    lineage.writes[0].name = "EMAIL".into();
    assert_eq!(
        pg_with(
            &lineage,
            &config,
            &facts(&[("hashed", &[("Email", NONE), ("email", DEFAULT)])])
        ),
        Err(Rejection::Computed {
            column: "public.hashed.EMAIL".into(),
        })
    );
}

#[test]
fn generated_hash_column_refuses_every_write_but_delete() {
    for statement in [StatementKind::Insert, StatementKind::Update, StatementKind::Merge] {
        assert_eq!(beside_hashed(statement.clone(), GENERATED), computed(), "{statement:?}");
    }
    assert!(beside_hashed(StatementKind::Delete, GENERATED).is_ok());
}

#[test]
fn on_update_hash_column_refuses_inserts_updates_and_merges() {
    for statement in [StatementKind::Insert, StatementKind::Update, StatementKind::Merge] {
        assert_eq!(beside_hashed(statement.clone(), ON_UPDATE), computed(), "{statement:?}");
    }
    assert!(beside_hashed(StatementKind::Delete, ON_UPDATE).is_ok());
}

#[test]
fn unhashed_tables_ignore_column_facts() {
    let config = config(vec![ssn("shop.public.users.ssn"), ssn("shop.public.hashed.email")]);
    let lineage = write("hashed", "id", vec![], StatementKind::Insert);
    let names = facts(&[("hashed", &[("id", DEFAULT), ("email", (true, true, true))])]);
    assert!(pg_with(&lineage, &config, &names).is_ok());
}

#[test]
fn creates_skip_the_server_filled_checks() {
    for statement in [StatementKind::CreateTableAs, StatementKind::CreateMaterializedView] {
        let lineage = write("hashed", "email", vec![col("users", "email")], statement.clone());
        assert!(pg(&lineage, &hashed_target()).is_ok(), "{statement:?}");
        let mut chosen_lineage = write("hashed", "email", vec![], statement.clone());
        chosen_lineage.writes[0].flow = Flow::Derived;
        assert_eq!(pg(&chosen_lineage, &hashed_target()), chosen(), "{statement:?}");
    }
}

#[test]
fn missing_facts_refuse_writes_to_hash_tables() {
    for statement in [StatementKind::Insert, StatementKind::Update, StatementKind::Merge] {
        let lineage = write("hashed", "id", vec![], statement.clone());
        assert_eq!(pg(&lineage, &hashed_target()), computed(), "{statement:?}");
    }
    let lineage = write("hashed", "id", vec![], StatementKind::Delete);
    assert_eq!(pg(&lineage, &hashed_target()), trigger("hashed"));
}

#[test]
fn columns_with_every_fact_refuse_writes_to_hash_tables() {
    for star_visible in [true, false] {
        let column = CatalogColumn {
            name: "email".into(),
            star_visible,
            default: true,
            generated: true,
            on_update: true,
        };
        let names = Names {
            facts: [(
                table("hashed"),
                Facts {
                    columns: vec![column],
                    triggers: false,
                    cascades: Vec::new(),
                },
            )]
            .into(),
            ..Names::default()
        };
        let lineage = write("hashed", "id", vec![], StatementKind::Update);
        assert_eq!(pg_with(&lineage, &hashed_target(), &names), computed());
    }
}

#[test]
fn hash_rules_on_a_landing_member_gate_writes_through_the_parent() {
    let config = config(vec![rule(
        "shop.public.ptab_1.email",
        PiiOperator::Hash,
        "EMAIL_ADDRESS",
    )]);
    let mut names = tree();
    names.facts = facts(&[("ptab", &[("id", NONE), ("email", NONE)])]).facts;
    let mut lineage = write("ptab", "email", vec![], StatementKind::Insert);
    lineage.writes[0].flow = Flow::Derived;
    assert_eq!(
        pg_with(&lineage, &config, &names),
        Err(Rejection::Chosen {
            column: "public.ptab.email".into(),
        })
    );
}

// Cascades and triggers.

/// The `Trigger` rejection of `public.<relation>`.
fn trigger(relation: &str) -> Result<Plan, Rejection> {
    Err(Rejection::Trigger {
        relation: format!("public.{relation}"),
    })
}

/// A cascade edge from `parent` to `child` (`table.column`) doing `action`.
fn edge(parent: &str, child: &str, action: CascadeAction, on_delete: bool) -> Cascade {
    let split = |name: &str| {
        let (table, column) = name.split_once('.').expect("table.column");
        col(table, column)
    };
    Cascade {
        parent: split(parent),
        child: split(child),
        action,
        on_delete,
        triggers: false,
    }
}

/// `child.k` hashed as `EMAIL_ADDRESS`, plus `parent` rules.
fn cascade_rules(parent: Vec<ColumnRule>) -> PiiConfig {
    let mut rules = vec![rule("shop.public.child.k", PiiOperator::Hash, "EMAIL_ADDRESS")];
    rules.extend(parent);
    config(rules)
}

/// Plans `statement` on `parent` writing `k`, whose facts list `cascades`.
fn through_parent(statement: StatementKind, config: &PiiConfig, cascades: Vec<Cascade>) -> Result<Plan, Rejection> {
    let mut lineage = write("parent", "k", vec![], statement);
    lineage.writes[0].flow = Flow::Derived;
    let mut names = facts(&[("parent", &[("k", NONE)])]);
    names.facts.get_mut(&table("parent")).expect("parent facts").cascades = cascades;
    pg_with(&lineage, config, &names)
}

/// The `Computed` rejection of `public.<column>`.
fn computed_at(column: &str) -> Result<Plan, Rejection> {
    Err(Rejection::Computed {
        column: format!("public.{column}"),
    })
}

#[test]
fn copy_cascades_into_a_hash_column_need_the_same_pair_on_the_parent() {
    let copy = vec![edge("parent.k", "child.k", CascadeAction::Copy, false)];
    assert_eq!(
        through_parent(StatementKind::Update, &cascade_rules(vec![]), copy.clone()),
        computed_at("child.k")
    );
    let masked = vec![rule("shop.public.parent.k", PiiOperator::Mask, "EMAIL_ADDRESS")];
    assert_eq!(
        through_parent(StatementKind::Update, &cascade_rules(masked), copy.clone()),
        computed_at("child.k")
    );
    let same = vec![rule("shop.public.parent.k", PiiOperator::Hash, "EMAIL_ADDRESS")];
    let mut lineage = write("parent", "k", vec![col("child", "k")], StatementKind::Update);
    let mut names = facts(&[("parent", &[("k", NONE)])]);
    names.facts.get_mut(&table("parent")).expect("parent facts").cascades = copy;
    assert!(pg_with(&lineage, &cascade_rules(same), &names).is_ok());
    lineage.writes[0] = TargetColumn {
        name: "other".into(),
        sources: BTreeSet::new(),
        flow: Flow::Derived,
        named: true,
    };
    assert!(
        pg_with(&lineage, &cascade_rules(vec![]), &names).is_ok(),
        "unwritten parent column"
    );
}

#[test]
fn update_cascades_fire_on_updates_merges_and_inserts_only() {
    let copy = vec![edge("parent.k", "child.k", CascadeAction::Copy, false)];
    for statement in [StatementKind::Update, StatementKind::Merge, StatementKind::Insert] {
        assert_eq!(
            through_parent(statement.clone(), &cascade_rules(vec![]), copy.clone()),
            computed_at("child.k"),
            "{statement:?}"
        );
    }
    assert!(through_parent(StatementKind::Delete, &cascade_rules(vec![]), copy).is_ok());
}

#[test]
fn set_default_cascades_into_a_hash_column_are_computed() {
    for on_delete in [false, true] {
        let default = vec![edge("parent.k", "child.k", CascadeAction::Default, on_delete)];
        let statement = if on_delete {
            StatementKind::Delete
        } else {
            StatementKind::Update
        };
        assert_eq!(
            through_parent(statement, &cascade_rules(vec![]), default),
            computed_at("child.k"),
            "{on_delete}"
        );
    }
}

#[test]
fn set_null_and_delete_cascades_pass() {
    let edges = vec![
        edge("parent.k", "child.k", CascadeAction::Null, false),
        edge("parent.k", "child.k", CascadeAction::Null, true),
        edge("parent.k", "child.k", CascadeAction::Delete, true),
    ];
    for statement in [StatementKind::Update, StatementKind::Delete, StatementKind::Merge] {
        let planned = through_parent(statement.clone(), &cascade_rules(vec![]), edges.clone());
        assert!(planned.is_ok(), "{statement:?}");
    }
}

#[test]
fn cascades_into_unhashed_columns_pass() {
    let config = config(vec![rule("shop.public.other.e", PiiOperator::Hash, "EMAIL_ADDRESS")]);
    let edges = vec![edge("parent.k", "child.k", CascadeAction::Default, false)];
    assert!(through_parent(StatementKind::Update, &config, edges).is_ok());
}

#[test]
fn transitive_cascades_are_followed() {
    let rules = config(vec![rule(
        "shop.public.grandchild.k",
        PiiOperator::Hash,
        "EMAIL_ADDRESS",
    )]);
    let chain = |first: CascadeAction, second: Cascade| {
        vec![
            edge("parent.k", "child.k", first, first == CascadeAction::Delete),
            second,
        ]
    };
    let computed = computed_at("grandchild.k");
    // An updated child column fires the update edges of that column.
    let update = edge("child.k", "grandchild.k", CascadeAction::Default, false);
    assert_eq!(
        through_parent(
            StatementKind::Update,
            &rules,
            chain(CascadeAction::Copy, update.clone())
        ),
        computed
    );
    assert_eq!(
        through_parent(
            StatementKind::Update,
            &rules,
            chain(CascadeAction::Null, update.clone())
        ),
        computed
    );
    let other_column = edge("child.j", "grandchild.k", CascadeAction::Default, false);
    assert!(
        through_parent(
            StatementKind::Update,
            &rules,
            chain(CascadeAction::Copy, other_column.clone())
        )
        .is_ok()
    );
    // A removed child row fires every delete edge of the child.
    let delete = edge("child.j", "grandchild.k", CascadeAction::Default, true);
    assert_eq!(
        through_parent(
            StatementKind::Delete,
            &rules,
            chain(CascadeAction::Delete, delete.clone())
        ),
        computed
    );
    assert!(
        through_parent(
            StatementKind::Delete,
            &rules,
            chain(CascadeAction::Delete, other_column)
        )
        .is_ok()
    );
    assert!(through_parent(StatementKind::Update, &rules, chain(CascadeAction::Copy, delete)).is_ok());
}

#[test]
fn cascades_changing_a_child_with_other_hash_columns_are_computed() {
    // A generated or on-update column of the child may be recomputed from the changed column.
    let rules = config(vec![rule("shop.public.child.h", PiiOperator::Hash, "EMAIL_ADDRESS")]);
    for action in [CascadeAction::Copy, CascadeAction::Null, CascadeAction::Default] {
        let edges = vec![edge("parent.k", "child.k", action, false)];
        assert_eq!(
            through_parent(StatementKind::Update, &rules, edges),
            computed_at("child.h"),
            "{action:?}"
        );
    }
    let delete = vec![edge("parent.k", "child.k", CascadeAction::Delete, true)];
    assert!(through_parent(StatementKind::Delete, &rules, delete).is_ok());
}

#[test]
fn updates_through_a_partitioned_target_fire_delete_edges() {
    let default = vec![edge("ptab.k", "child.k", CascadeAction::Default, true)];
    let mut lineage = write("ptab", "k", vec![], StatementKind::Update);
    lineage.writes[0].flow = Flow::Derived;
    let mut names = tree();
    names.facts = facts(&[("ptab", &[("k", NONE)])]).facts;
    names.facts.get_mut(&table("ptab")).expect("ptab facts").cascades = default.clone();
    assert_eq!(
        pg_with(&lineage, &cascade_rules(vec![]), &names),
        computed_at("child.k")
    );
    // A table that is not partitioned moves no rows.
    assert!(through_parent(StatementKind::Update, &cascade_rules(vec![]), default).is_ok());
}

#[test]
fn cascade_relations_match_case_folded() {
    let mut copy = edge("parent.k", "child.k", CascadeAction::Copy, false);
    copy.parent.relation = RelationId::new("SHOP", "Public", "Parent");
    assert_eq!(
        through_parent(StatementKind::Update, &cascade_rules(vec![]), vec![copy.clone()]),
        computed_at("child.k")
    );
    let mut first = edge("parent.k", "mid.k", CascadeAction::Copy, false);
    first.child.relation = RelationId::new(DB, SCHEMA, "MID");
    let second = edge("mid.k", "child.k", CascadeAction::Default, false);
    assert_eq!(
        through_parent(StatementKind::Update, &cascade_rules(vec![]), vec![first, second]),
        computed_at("child.k")
    );
}

#[test]
fn truncate_of_a_triggered_table_is_refused_while_hash_rules_exist() {
    let lineage = ddl(StatementKind::Truncate, "plain");
    let mut names = facts(&[("plain", &[("id", NONE)])]);
    assert!(pg_with(&lineage, &hashed_target(), &names).is_ok());
    names.facts.get_mut(&table("plain")).expect("plain facts").triggers = true;
    assert_eq!(pg_with(&lineage, &hashed_target(), &names), trigger("plain"));
}

/// `hashed_target`, writing `hashed.id`, with `hashed` carrying `triggers` and `cascades`.
fn into_triggered(statement: StatementKind, triggers: bool, cascades: Vec<Cascade>) -> Result<Plan, Rejection> {
    let lineage = write("hashed", "id", vec![], statement);
    let mut names = facts(&[("hashed", &[("id", NONE), ("email", NONE)])]);
    let facts = names.facts.get_mut(&table("hashed")).expect("hashed facts");
    facts.triggers = triggers;
    facts.cascades = cascades;
    pg_with(&lineage, &hashed_target(), &names)
}

#[test]
fn triggers_on_the_target_refuse_every_write_while_hash_rules_exist() {
    for statement in [
        StatementKind::Insert,
        StatementKind::Update,
        StatementKind::Delete,
        StatementKind::Merge,
    ] {
        assert_eq!(
            into_triggered(statement.clone(), true, Vec::new()),
            trigger("hashed"),
            "{statement:?}"
        );
        assert!(into_triggered(statement, false, Vec::new()).is_ok());
    }
    // Whether or not the target has a ruled column.
    let lineage = write("plain", "id", vec![], StatementKind::Update);
    let mut names = facts(&[("plain", &[("id", NONE)])]);
    names.facts.get_mut(&table("plain")).expect("plain facts").triggers = true;
    assert_eq!(pg_with(&lineage, &hashed_target(), &names), trigger("plain"));
    // No `hash` rule, no trigger gate.
    let masked = config(vec![ssn("shop.public.users.ssn")]);
    assert!(pg_with(&lineage, &masked, &names).is_ok());
    // A create targets a new relation.
    let created = write("plain", "id", vec![], StatementKind::CreateTableAs);
    assert!(pg_with(&created, &hashed_target(), &names).is_ok());
}

#[test]
fn triggers_on_a_fired_cascade_child_refuse_the_write() {
    let mut fired = edge("hashed.id", "audit.id", CascadeAction::Delete, true);
    fired.triggers = true;
    assert_eq!(
        into_triggered(StatementKind::Delete, false, vec![fired.clone()]),
        trigger("audit")
    );
    assert!(
        into_triggered(StatementKind::Update, false, vec![fired]).is_ok(),
        "not fired"
    );
}

#[test]
fn chosen_computed_and_trigger_messages_and_gates_are_pinned() {
    let chosen = Rejection::Chosen {
        column: "public.hashed.email".into(),
    };
    assert_eq!(
        chosen.to_string(),
        "column policy: column \"public.hashed.email\" is hashed and only takes copies of columns hashed the same way, or NULL"
    );
    assert_eq!(chosen.gate(), "chosen");
    let computed = Rejection::Computed {
        column: "public.hashed.email".into(),
    };
    assert_eq!(
        computed.to_string(),
        "column policy: column \"public.hashed.email\" is hashed and filled by the database, so this write is refused"
    );
    assert_eq!(computed.gate(), "computed");
    let trigger = Rejection::Trigger {
        relation: "public.hashed".into(),
    };
    assert_eq!(
        trigger.to_string(),
        "column policy: relation \"public.hashed\" has triggers or rules, which may fill hashed columns, so writes to it are refused"
    );
    assert_eq!(trigger.gate(), "trigger");
}

// Messages.

#[test]
fn rejection_messages_name_no_rule() {
    let mut secret = rule("shop.public.users.ssn", PiiOperator::Mask, "US_SSN");
    secret.name = "secret-rule".into();
    let config = config(vec![secret]);
    let lineages = [
        referencing(col("users", "ssn"), Role::Filter, Origin::Query),
        write("copy", "x", vec![col("users", "ssn")], StatementKind::Insert),
        {
            let mut lineage = select(vec![vec![col("users", "ssn")]]);
            lineage.statement = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::Query));
            lineage
        },
        ddl(StatementKind::CreateIndex, "users"),
    ];
    for lineage in lineages {
        let message = pg(&lineage, &config).expect_err("rejected").to_string();
        assert!(message.starts_with("column policy: "), "{message}");
        for leaked in ["secret-rule", "US_SSN", "hash", "mask", "replace", "redact"] {
            assert!(!message.contains(leaked), "{message}");
        }
    }
}

#[test]
fn rejection_messages_escape_identifiers() {
    let name = "a\"b\\c\nd\u{1b}e\u{202e}f'g";
    let escaped = r#""a\"b\\c\nd\u{1b}e\u{202e}f'g""#;
    let cases = [
        (
            Rejection::UnknownFunction { name: name.into() },
            format!("column policy: function {escaped} is not a known built-in"),
        ),
        (
            Rejection::DeniedRelation { relation: name.into() },
            format!("column policy: relation {escaped} is denied"),
        ),
        (
            Rejection::Reference {
                column: name.into(),
                role: "WHERE",
                view: Some(name.into()),
            },
            format!("column policy: column {escaped} (ruled) used in WHERE in view {escaped}"),
        ),
        (
            Rejection::Unnamed {
                relation: name.into(),
                position: 0,
            },
            format!(
                "column policy: created column 0 of {escaped} carries ruled values; alias the column so a rule can name it"
            ),
        ),
    ];
    for (rejection, message) in cases {
        assert_eq!(rejection.to_string(), message);
    }
}

#[test]
fn catalog_round_cap_names_the_cap_and_gate() {
    let rejection = Rejection::CatalogRounds.logged();
    assert_eq!(
        rejection.to_string(),
        "column policy: catalog resolution exceeded 16 rounds"
    );
    assert_eq!(rejection.gate(), "lineage");
}

#[test]
fn names_are_known_only_for_single_same_name_sources() {
    let mut lineage = select(vec![
        vec![col("users", "Email")],
        vec![col("users", "ssn")],
        vec![col("users", "name")],
        vec![col("users", "id"), col("users", "ssn")],
        vec![],
    ]);
    lineage.columns[0].name = "email".into();
    lineage.columns[1].name = "ssn".into();
    lineage.columns[2].name = "full_name".into();
    lineage.columns[3].name = "id".into();
    lineage.columns[4].name = "one".into();
    let plan = pg(&lineage, &config(vec![])).expect("plan");
    assert_eq!(
        plan.columns.into_iter().map(|column| column.name).collect::<Vec<_>>(),
        [Some("email".to_string()), Some("ssn".to_string()), None, None, None]
    );
}

// Inheritance and partition families.

/// A descendant `name` with `ancestors`.
fn member(name: &str, partitioned: bool, ancestors: &[&str]) -> Member {
    Member {
        id: table(name),
        partitioned,
        ancestors: ancestors.iter().map(|name| table(name)).collect(),
    }
}

/// Names carrying the families of two trees, each seen from every member.
///
/// Partitioned `ptab` ← `ptab_1`, `ptab_2`; legacy `inh_parent` ←
/// `inh_child` (which also inherits `inh_other`) ← `inh_grandchild`.
fn tree() -> Names {
    let family = |partitioned, ancestors: &[&str], descendants: Vec<Member>| Family {
        partitioned,
        ancestors: ancestors.iter().map(|name| table(name)).collect(),
        descendants,
    };
    let child = || member("inh_child", false, &["inh_parent", "inh_other"]);
    let grandchild = || member("inh_grandchild", false, &["inh_child", "inh_parent", "inh_other"]);
    let families = [
        (
            "ptab",
            family(
                true,
                &[],
                vec![member("ptab_1", false, &["ptab"]), member("ptab_2", false, &["ptab"])],
            ),
        ),
        ("ptab_1", family(false, &["ptab"], vec![])),
        ("ptab_2", family(false, &["ptab"], vec![])),
        ("inh_parent", family(false, &[], vec![child(), grandchild()])),
        ("inh_other", family(false, &[], vec![child(), grandchild()])),
        (
            "inh_child",
            family(false, &["inh_parent", "inh_other"], vec![grandchild()]),
        ),
        (
            "inh_grandchild",
            family(false, &["inh_child", "inh_parent", "inh_other"], vec![]),
        ),
    ];
    Names {
        families: families
            .into_iter()
            .map(|(name, family)| (table(name), family))
            .collect(),
        ..Names::default()
    }
}

/// The single position of a one-column plan under [`tree`].
fn only_in_tree(lineage: &Lineage, config: &PiiConfig) -> Position {
    let positions = positions(pg_with(lineage, config, &tree()).expect("plan"));
    assert_eq!(positions.len(), 1);
    positions.into_iter().next().expect("one position")
}

#[test]
fn child_rule_covers_reads_through_the_parent() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    assert_eq!(
        only_in_tree(&select(vec![vec![col("ptab", "email")]]), &config),
        MASKED_SSN
    );
}

#[test]
fn parent_rule_covers_direct_reads_of_every_descendant() {
    let config = config(vec![ssn("shop.public.inh_parent.email")]);
    for member in ["inh_child", "inh_grandchild"] {
        assert_eq!(
            only_in_tree(&select(vec![vec![col(member, "email")]]), &config),
            MASKED_SSN,
            "{member}"
        );
    }
}

#[test]
fn siblings_never_cover_each_other() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    assert_eq!(
        only_in_tree(&select(vec![vec![col("ptab_2", "email")]]), &config),
        Position::Global
    );
}

#[test]
fn reads_through_a_parent_follow_a_members_other_parents() {
    let config = config(vec![ssn("shop.public.inh_other.email")]);
    assert_eq!(
        only_in_tree(&select(vec![vec![col("inh_parent", "email")]]), &config),
        MASKED_SSN
    );
}

#[test]
fn family_rules_match_by_column_name() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    assert_eq!(
        only_in_tree(&select(vec![vec![col("ptab", "note")]]), &config),
        Position::Global
    );
}

#[test]
fn different_pairs_across_a_family_conflict() {
    let config = config(vec![
        rule("shop.public.ptab.email", PiiOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.ptab_1.email", PiiOperator::Mask, "EMAIL_ADDRESS"),
    ]);
    for relation in ["ptab", "ptab_1"] {
        let lineage = select(vec![vec![col(relation, "email")]]);
        assert_eq!(
            pg_with(&lineage, &config, &tree()),
            Err(Rejection::Conflict { position: 0 }),
            "{relation}"
        );
    }
    assert_eq!(
        only_in_tree(&select(vec![vec![col("ptab_2", "email")]]), &config),
        Position::Action(PiiOperator::Hash, Entity::EmailAddress)
    );
}

#[test]
fn same_pairs_across_a_family_merge() {
    let config = config(vec![ssn("shop.public.ptab.email"), ssn("shop.public.ptab_1.email")]);
    assert_eq!(
        only_in_tree(&select(vec![vec![col("ptab", "email")]]), &config),
        MASKED_SSN
    );
}

#[test]
fn child_rule_covers_references_through_the_parent() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    let lineage = referencing(col("ptab", "email"), Role::Filter, Origin::Query);
    assert!(matches!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::Reference { .. })
    ));
}

#[test]
fn explain_of_a_parent_of_a_ruled_member_rejects() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    let mut lineage = select(vec![vec![col("ptab", "id")]]);
    lineage.statement = StatementKind::Explain(ExplainForm::Other, Box::new(StatementKind::Query));
    assert_eq!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::Explain {
            relation: "public.ptab".into(),
        })
    );
}

#[test]
fn create_index_on_a_parent_of_a_ruled_member_rejects() {
    let config = config(vec![ssn("shop.public.inh_grandchild.email")]);
    assert_eq!(
        pg_with(&ddl(StatementKind::CreateIndex, "inh_parent"), &config, &tree()),
        Err(Rejection::RuledIndex {
            relation: "public.inh_parent".into(),
        })
    );
    assert!(pg_with(&ddl(StatementKind::CreateIndex, "ptab"), &config, &tree()).is_ok());
}

#[test]
fn reads_of_a_family_with_a_ruled_member_touch_ruled() {
    let lineage = select(vec![vec![col("ptab", "id")]]);
    let families = tree().families;
    assert!(touches_ruled(
        &lineage,
        &config(vec![ssn("shop.public.ptab_2.x")]),
        &families
    ));
    assert!(!touches_ruled(
        &lineage,
        &config(vec![ssn("shop.public.inh_child.x")]),
        &families
    ));
}

#[test]
fn writes_from_a_parent_of_a_ruled_member_launder() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    let lineage = write("copy", "x", vec![col("ptab", "email")], StatementKind::Insert);
    assert!(matches!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::Laundering { .. })
    ));
}

#[test]
fn writes_into_a_member_follow_its_ancestors_rules() {
    let config = config(vec![ssn("shop.public.users.ssn"), ssn("shop.public.ptab.email")]);
    let lineage = write("ptab_1", "email", vec![col("users", "ssn")], StatementKind::Insert);
    assert!(pg_with(&lineage, &config, &tree()).is_ok());
}

#[test]
fn writes_launder_into_a_descendant_ruled_otherwise() {
    let config = config(vec![
        ssn("shop.public.users.ssn"),
        ssn("shop.public.ptab.email"),
        rule("shop.public.ptab_2.email", PiiOperator::Redact, "US_SSN"),
    ]);
    let lineage = write("ptab", "email", vec![col("users", "ssn")], StatementKind::Insert);
    assert_eq!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::Laundering {
            column: "public.ptab.email".into(),
        })
    );
}

#[test]
fn writes_check_each_landing_member_not_the_family_union() {
    let config = config(vec![
        ssn("shop.public.users.ssn"),
        ssn("shop.public.ptab_1.email"),
        ssn("shop.public.ptab_2.email"),
    ]);
    let lineage = write("ptab", "email", vec![col("users", "ssn")], StatementKind::Insert);
    assert!(matches!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::Laundering { .. })
    ));
}

#[test]
fn unruled_writes_skip_the_landing_check() {
    let config = config(vec![
        ssn("shop.public.ptab.email"),
        rule("shop.public.ptab_2.email", PiiOperator::Redact, "US_SSN"),
    ]);
    let lineage = write("ptab", "email", vec![col("users", "name")], StatementKind::Insert);
    assert!(pg_with(&lineage, &config, &tree()).is_ok());
}

#[test]
fn updates_through_a_non_uniformly_ruled_partitioned_table_reject() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    let lineage = write("ptab", "id", vec![], StatementKind::Update);
    assert_eq!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::RowMovement {
            relation: "public.ptab".into(),
        })
    );
}

#[test]
fn explained_updates_through_a_non_uniformly_ruled_partitioned_table_reject() {
    let config = config(vec![ssn("shop.public.ptab_1.email")]);
    let mut lineage = write("ptab", "id", vec![], StatementKind::Update);
    lineage.statement = StatementKind::Explain(ExplainForm::QueryPlan, Box::new(StatementKind::Update));
    assert_eq!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::RowMovement {
            relation: "public.ptab".into(),
        })
    );
}

#[test]
fn every_merge_into_a_non_uniformly_ruled_partitioned_table_rejects() {
    let config = config(vec![
        ssn("shop.public.ptab_1.email"),
        rule("shop.public.ptab_2.email", PiiOperator::Redact, "US_SSN"),
    ]);
    let lineage = write("ptab", "note", vec![], StatementKind::Merge);
    assert!(matches!(
        pg_with(&lineage, &config, &tree()),
        Err(Rejection::RowMovement { .. })
    ));
}

#[test]
fn row_movement_allows_leaves_legacy_trees_and_other_writes() {
    let config = config(vec![
        ssn("shop.public.ptab_1.email"),
        ssn("shop.public.inh_child.email"),
    ]);
    for (target, statement) in [
        ("ptab_1", StatementKind::Update),
        ("ptab_2", StatementKind::Merge),
        ("inh_parent", StatementKind::Update),
        ("ptab", StatementKind::Delete),
        // `INSERT … ON CONFLICT DO UPDATE` is an `Insert`.
        ("ptab", StatementKind::Insert),
    ] {
        let lineage = write(target, "id", vec![], statement.clone());
        assert!(pg_with(&lineage, &config, &tree()).is_ok(), "{target} {statement:?}");
    }
}

#[test]
fn row_movement_allows_uniformly_ruled_and_unruled_trees() {
    for rules in [
        vec![ssn("shop.public.ptab.email")],
        vec![ssn("shop.public.ptab_1.email"), ssn("shop.public.ptab_2.email")],
        vec![ssn("shop.public.users.ssn")],
    ] {
        let config = config(rules);
        let lineage = write("ptab", "id", vec![], StatementKind::Update);
        assert!(pg_with(&lineage, &config, &tree()).is_ok());
    }
}

#[test]
fn statement_kinds_map_to_their_write_effects() {
    let none = Effects {
        fills: false,
        omits: false,
        updates: false,
        deletes: false,
        triggers: false,
        moves: false,
    };
    let insert = Effects {
        fills: true,
        omits: true,
        updates: true,
        deletes: true,
        triggers: true,
        ..none
    };
    let update = Effects {
        fills: true,
        updates: true,
        triggers: true,
        moves: true,
        ..none
    };
    let rows = [
        (StatementKind::Insert, false, insert),
        (StatementKind::Update, false, update),
        // A row moved between partitions may be deleted and re-inserted.
        (
            StatementKind::Update,
            true,
            Effects {
                deletes: true,
                ..update
            },
        ),
        (StatementKind::Merge, false, Effects { moves: true, ..insert }),
        (
            StatementKind::Delete,
            false,
            Effects {
                deletes: true,
                triggers: true,
                ..none
            },
        ),
        (StatementKind::Truncate, false, Effects { triggers: true, ..none }),
        (StatementKind::CreateTableAs, false, none),
        (StatementKind::CreateMaterializedView, false, none),
        (StatementKind::Query, false, none),
        (StatementKind::Show(Show::Tables), false, none),
        (StatementKind::Describe, false, none),
        (StatementKind::CreateView, false, none),
        (StatementKind::CreateTable, false, none),
        (StatementKind::Drop, false, none),
        (StatementKind::CreateIndex, false, none),
    ];
    for (statement, partitioned, expected) in rows {
        assert_eq!(
            effects(&statement, partitioned),
            expected,
            "{statement:?} partitioned={partitioned}"
        );
    }
}

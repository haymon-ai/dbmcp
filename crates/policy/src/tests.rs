//! Unit tests for the planner gates, with hand-built lineage.

use std::collections::BTreeSet;

use dbmcp_config::{ColumnRule, PrivacyConfig, PrivacyOperator};
use dbmcp_sql_lineage::{
    Catalog, Dialect, ExplainForm, Family, FunctionUse, Lineage, LineageError, Member, Origin, OutputColumn, Reference,
    RelationId, RelationKind, RelationUse, Role, Show, SourceRef, StatementKind, TargetColumn, analyze,
};

use dbmcp_privacy::Entity;
use dbmcp_privacy::cells::{Plan, Position};

use crate::plan::{Families, Names, plan, touches_ruled};
use crate::rejection::reject_lineage;
use crate::{LineageClass, Rejection};

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
fn rule(key: &str, operator: PrivacyOperator, entity: &'static str) -> ColumnRule {
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
    rule(key, PrivacyOperator::Mask, "US_SSN")
}

/// Builds a config holding `rules`, with global defaults.
fn config(rules: Vec<ColumnRule>) -> PrivacyConfig {
    PrivacyConfig {
        rules,
        ..PrivacyConfig::default()
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
fn plan_on(lineage: &Lineage, dialect: Dialect, config: &PrivacyConfig) -> Result<Plan, Rejection> {
    plan(lineage, dialect, config, &Names::default())
}

/// Plans `lineage` on `PostgreSQL`.
fn pg(lineage: &Lineage, config: &PrivacyConfig) -> Result<Plan, Rejection> {
    plan_on(lineage, Dialect::Postgres, config)
}

/// Plans `lineage` on `PostgreSQL` under `names`.
fn pg_with(lineage: &Lineage, config: &PrivacyConfig, names: &Names) -> Result<Plan, Rejection> {
    plan(lineage, Dialect::Postgres, config, names)
}

/// The single position of a one-column plan.
fn only(lineage: &Lineage, config: &PrivacyConfig) -> Position {
    let positions = positions(pg(lineage, config).expect("plan"));
    assert_eq!(positions.len(), 1);
    positions.into_iter().next().expect("one position")
}

/// The positions of `plan`, in output order.
fn positions(plan: Plan) -> Vec<Position> {
    plan.columns.into_iter().map(|column| column.position).collect()
}

/// The masked `US_SSN` position.
const MASKED_SSN: Position = Position::Action(PrivacyOperator::Mask, Entity::UsSsn);

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
    let config = config(vec![rule("shop.public.users.name", PrivacyOperator::Replace, "PERSON")]);
    assert_eq!(
        only(&select(vec![vec![col("users", "name")]]), &config),
        Position::Action(PrivacyOperator::Replace, Entity::Person)
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
        rule("shop.public.users.email", PrivacyOperator::Redact, "US_SSN"),
    ]);
    let lineage = select(vec![vec![col("users", "ssn"), col("users", "email")]]);
    assert_eq!(pg(&lineage, &config), Err(Rejection::Conflict { position: 0 }));
}

#[test]
fn same_operator_different_entity_conflicts() {
    let config = config(vec![
        rule("shop.public.users.email", PrivacyOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.users.phone", PrivacyOperator::Hash, "PHONE_NUMBER"),
    ]);
    let lineage = select(vec![vec![col("users", "email"), col("users", "phone")]]);
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Conflict { .. })));
}

#[test]
fn same_rule_pair_combines() {
    let config = config(vec![
        rule("shop.public.users.firstname", PrivacyOperator::Hash, "PERSON"),
        rule("shop.public.users.lastname", PrivacyOperator::Hash, "PERSON"),
    ]);
    let lineage = select(vec![vec![col("users", "firstname"), col("users", "lastname")]]);
    assert_eq!(
        only(&lineage, &config),
        Position::Action(PrivacyOperator::Hash, Entity::Person)
    );
}

#[test]
fn conflict_message_omits_output_name() {
    let config = config(vec![
        ssn("shop.public.users.ssn"),
        rule("shop.public.users.email", PrivacyOperator::Redact, "EMAIL_ADDRESS"),
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
        named: true,
    }];
    lineage.statement = statement;
    lineage
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
        rule("shop.public.copy.x", PrivacyOperator::Redact, "US_SSN"),
        rule("shop.public.copy.x", PrivacyOperator::Mask, "EMAIL_ADDRESS"),
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
fn ruled_source_into_unnamed_create_column_asks_for_an_alias() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let mut lineage = write(
        "t2",
        "upper(ssn)",
        vec![col("users", "ssn")],
        StatementKind::CreateTableAs,
    );
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
    assert!(!message.contains("upper(ssn)"), "{message}");

    lineage.writes[0].sources = BTreeSet::from([col("users", "name")]);
    assert!(pg(&lineage, &config).is_ok(), "unruled data needs no name");
}

#[test]
fn conflicting_write_sources_reject() {
    let config = config(vec![
        ssn("shop.public.users.ssn"),
        rule("shop.public.users.email", PrivacyOperator::Redact, "EMAIL_ADDRESS"),
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

// Messages.

#[test]
fn rejection_messages_name_no_rule() {
    let mut secret = rule("shop.public.users.ssn", PrivacyOperator::Mask, "US_SSN");
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
fn only_in_tree(lineage: &Lineage, config: &PrivacyConfig) -> Position {
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
        rule("shop.public.ptab.email", PrivacyOperator::Hash, "EMAIL_ADDRESS"),
        rule("shop.public.ptab_1.email", PrivacyOperator::Mask, "EMAIL_ADDRESS"),
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
        Position::Action(PrivacyOperator::Hash, Entity::EmailAddress)
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
        rule("shop.public.ptab_2.email", PrivacyOperator::Redact, "US_SSN"),
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
        rule("shop.public.ptab_2.email", PrivacyOperator::Redact, "US_SSN"),
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
fn every_merge_into_a_non_uniformly_ruled_partitioned_table_rejects() {
    let config = config(vec![
        ssn("shop.public.ptab_1.email"),
        rule("shop.public.ptab_2.email", PrivacyOperator::Redact, "US_SSN"),
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

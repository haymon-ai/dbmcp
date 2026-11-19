//! Unit tests for the planner gates, with hand-built lineage.

use std::collections::BTreeSet;

use dbmcp_config::{ColumnRule, PiiConfig, PiiOperator};
use dbmcp_sql_lineage::{
    Catalog, Dialect, FunctionUse, Lineage, LineageError, Origin, OutputColumn, Reference, RelationId, RelationKind,
    RelationUse, Role, SourceRef, StatementKind, TargetColumn, analyze,
};

use dbmcp_pii::Entity;
use dbmcp_pii::cells::{Plan, Position};

use crate::plan::{Names, plan, touches_ruled};
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

/// Plans `lineage` in `dialect` with no bare names.
fn plan_on(lineage: &Lineage, dialect: Dialect, config: &PiiConfig) -> Result<Plan, Rejection> {
    plan(lineage, dialect, config, &Names::default())
}

/// Plans `lineage` on `PostgreSQL`.
fn pg(lineage: &Lineage, config: &PiiConfig) -> Result<Plan, Rejection> {
    plan_on(lineage, Dialect::Postgres, config)
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
        (Dialect::Mysql, "WARNINGS"),
        (Dialect::Mysql, "ERRORS"),
        (Dialect::Mysql, "ENGINE"),
        (Dialect::Mariadb, "PROCESSLIST"),
        (Dialect::Mysql, "BINLOG"),
        (Dialect::Mariadb, "RELAYLOG"),
        (Dialect::Mariadb, "EXPLAIN"),
        (Dialect::Mariadb, "ANALYZE"),
        (Dialect::Postgres, "SEARCH_PATH"),
        (Dialect::Postgres, "TABLES"),
        (Dialect::Postgres, "ALL"),
    ];
    for (dialect, what) in cases {
        let mut lineage = select(vec![]);
        lineage.statement = StatementKind::Show(what.into());
        assert_eq!(
            plan_on(&lineage, dialect, &config(vec![])),
            Err(Rejection::DeniedStatement { statement: what.into() }),
            "{dialect:?} {what}"
        );
    }
}

#[test]
fn harmless_show_passes() {
    let mut lineage = select(vec![]);
    lineage.statement = StatementKind::Show("TABLES".into());
    assert!(plan_on(&lineage, Dialect::Mysql, &config(vec![])).is_ok());
}

#[test]
fn explain_of_show_unwraps_to_the_show_deny() {
    let mut lineage = select(vec![]);
    lineage.statement = StatementKind::Explain(Box::new(StatementKind::Show("WARNINGS".into())));
    assert!(matches!(
        plan_on(&lineage, Dialect::Mysql, &config(vec![])),
        Err(Rejection::DeniedStatement { .. })
    ));
}

#[test]
fn analyzed_log_show_forms_reject() {
    let cases = [
        ("SHOW BINLOG EVENTS", Dialect::Mysql),
        ("SHOW RELAYLOG EVENTS", Dialect::Mysql),
        ("SHOW EXPLAIN FOR 1", Dialect::Mariadb),
        ("SHOW ANALYZE FOR 1", Dialect::Mariadb),
        ("SHOW REPLICA STATUS", Dialect::Mysql),
        ("SHOW SLAVE STATUS", Dialect::Mariadb),
    ];
    for (sql, dialect) in cases {
        let lineage = analyze(sql, dialect, &Catalog::new()).expect(sql);
        assert!(
            matches!(
                plan_on(&lineage, dialect, &config(vec![])),
                Err(Rejection::DeniedStatement { .. })
            ),
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
    lineage.statement = StatementKind::Explain(Box::new(StatementKind::Query));
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
    lineage.statement = StatementKind::Explain(Box::new(StatementKind::Query));
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
    lineage.statement = StatementKind::Explain(Box::new(StatementKind::Drop));
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Explain { .. })));
}

#[test]
fn explain_of_unruled_relation_passes() {
    let config = config(vec![ssn("shop.public.audit.x")]);
    let mut lineage = select(vec![vec![col("users", "name")]]);
    lineage.statement = StatementKind::Explain(Box::new(StatementKind::Query));
    assert!(pg(&lineage, &config).is_ok());
}

#[test]
fn touches_ruled_needs_a_ruled_relation() {
    let lineage = select(vec![vec![col("users", "name")]]);
    assert!(!touches_ruled(&lineage, &config(vec![])));
    assert!(!touches_ruled(&lineage, &config(vec![ssn("shop.public.audit.x")])));
    assert!(touches_ruled(&lineage, &config(vec![ssn("shop.public.users.ssn")])));
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
                view: String::new(),
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
                view: " in view \"public.v\"".into(),
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
    plan(&lineage, Dialect::Postgres, &config(vec![]), names)
}

/// Names creating in `creation`, with bare names resolving to `bare`.
fn names(creation: Option<&str>, bare: &[&str]) -> Names {
    Names {
        creation: creation.map(Into::into),
        bare: bare.iter().map(|name| table(name)).collect(),
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
        assert!(
            plan(&lineage, Dialect::Postgres, &config(vec![]), &shadowing).is_ok(),
            "{statement:?}"
        );
    }
    let explained = StatementKind::Explain(Box::new(StatementKind::CreateTableAs));
    assert!(create(explained, &shadowing).is_ok());
}

#[test]
fn shadowed_target_rejects_before_laundering() {
    let config = config(vec![ssn("shop.public.users.ssn")]);
    let lineage = write("copy", "x", vec![col("users", "ssn")], StatementKind::CreateTableAs);
    assert!(matches!(pg(&lineage, &config), Err(Rejection::Laundering { .. })));
    let rejection = plan(&lineage, Dialect::Postgres, &config, &names(Some("app"), &["copy"])).expect_err("shadowed");
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
            lineage.statement = StatementKind::Explain(Box::new(StatementKind::Query));
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

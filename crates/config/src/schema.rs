//! The config schema: the JSON Schema every config file is validated against.
//!
//! The schema is hand-written in `assets/dbmcp.schema.json` and embedded in
//! the binary. Validation runs on the interpolated YAML tree: strings at
//! integer, boolean, and list settings are first cast with [`Kind::cast`],
//! then the schema checks the file's keys, shapes, types, enum values, and
//! integer ranges. Cast and schema errors form one list, rendered here,
//! value-free, in the loader's wording; `boon`'s own messages echo values
//! and are never shown.
//!
//! [`Kind::cast`]: crate::env::Kind::cast

use std::collections::HashSet;

use boon::{Compiler, ErrorKind, InstanceToken, SchemaIndex, Schemas, Type, Types, ValidationError};
use serde_json::{Map, Value};
use serde_saphyr::Location;

use crate::env::SETTINGS;
use crate::node::{Node, SpannedNode};

/// The schema's `$id`, also where editors fetch it from.
pub(crate) const URL: &str =
    "https://raw.githubusercontent.com/haymon-ai/dbmcp/master/crates/config/assets/dbmcp.schema.json";

/// The schema text.
pub(crate) const SCHEMA: &str = include_str!("../assets/dbmcp.schema.json");

/// One schema violation, located in the YAML file.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Problem {
    line: u64,
    column: u64,
    path: String,
    reason: String,
}

/// Casts the setting strings in `root`, then validates it against the config schema.
///
/// # Errors
///
/// Returns every cast failure and violation as `line:column: path: reason`,
/// sorted by location.
///
/// # Panics
///
/// Panics when the embedded schema does not compile, which its tests rule out.
pub(crate) fn validate(root: &mut SpannedNode) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    cast(root, &mut problems);
    let (schemas, index) = compile();
    let instance = to_json(&root.value);
    if let Err(error) = schemas.validate(&instance, index) {
        collect(&error, root, &mut problems);
    }
    if problems.is_empty() {
        return Ok(());
    }
    problems.sort();
    problems.dedup_by(|a, b| a.path == b.path && a.reason == b.reason);
    Err(problems
        .into_iter()
        .map(|p| format!("{}:{}: {}: {}", p.line, p.column, p.path, p.reason))
        .collect())
}

/// Casts each setting's string value to the YAML type its kind expects.
///
/// A value that fails to cast stays a string, which the schema admits at
/// every typed setting, so it is reported once.
fn cast(root: &mut SpannedNode, problems: &mut Vec<Problem>) {
    'settings: for setting in SETTINGS {
        let mut node = &mut *root;
        for key in setting.path.split('.') {
            node = match &mut node.value {
                Node::Map(entries) => match entries.get_mut(key) {
                    Some((_, value)) => value,
                    None => continue 'settings,
                },
                _ => continue 'settings,
            };
        }
        let Node::Str(text) = &node.value else {
            continue;
        };
        match setting.kind.cast(text, node.referenced) {
            Ok(value) => node.value = value,
            Err(reason) => problems.push(Problem {
                line: node.referenced.line(),
                column: node.referenced.column(),
                path: setting.path.to_owned(),
                reason,
            }),
        }
    }
}

/// Compiles the embedded schema.
fn compile() -> (Schemas, SchemaIndex) {
    let document: Value = serde_json::from_str(SCHEMA).expect("embedded schema is valid JSON");
    let mut compiler = Compiler::new();
    compiler.add_resource(URL, document).expect("schema URL is absolute");
    let mut schemas = Schemas::new();
    let index = compiler.compile(URL, &mut schemas).expect("embedded schema compiles");
    (schemas, index)
}

/// Converts the YAML tree to JSON for validation.
///
/// No setting takes a float, so every float becomes the stand-in `0.5`: a
/// fractional number fails every type, while `5.0` would pass as an
/// integer. Errors name the YAML node's own type.
fn to_json(node: &Node) -> Value {
    match node {
        Node::Null => Value::Null,
        Node::Bool(value) => Value::Bool(*value),
        Node::Int(value) => Value::from(*value),
        Node::UInt(value) => Value::from(*value),
        Node::Float(_) => Value::from(0.5),
        Node::Str(value) => Value::String(value.clone()),
        Node::Seq(items) => Value::Array(items.iter().map(|item| to_json(&item.value)).collect()),
        Node::Map(entries) => Value::Object(
            entries
                .iter()
                .map(|(key, (_, value))| (key.clone(), to_json(&value.value)))
                .collect::<Map<_, _>>(),
        ),
    }
}

/// Walks `error` down to its leaves and records one problem per leaf.
///
/// A failing `anyOf` / `oneOf` is one problem at its own location; its
/// branch errors are dropped.
fn collect(error: &ValidationError<'_, '_>, root: &SpannedNode, problems: &mut Vec<Problem>) {
    if !error.causes.is_empty() && !matches!(error.kind, ErrorKind::AnyOf | ErrorKind::OneOf(_)) {
        for cause in &error.causes {
            collect(cause, root, problems);
        }
        return;
    }
    let mut node = root;
    let mut keys = Vec::new();
    let mut item = None;
    for token in &error.instance_location.tokens {
        let child = match (&node.value, token) {
            (Node::Map(entries), InstanceToken::Prop(key)) => {
                keys.push(key.as_ref());
                entries.get(key.as_ref()).map(|(_, value)| value)
            }
            (Node::Seq(items), InstanceToken::Item(index)) => {
                item = Some(*index);
                items.get(*index)
            }
            _ => None,
        };
        node = child.expect("instance location points into the tree");
    }
    let mut push = |location: Location, path: String, reason: String| {
        let reason = match item {
            Some(index) => format!("item {index}: {reason}"),
            None => reason,
        };
        problems.push(Problem {
            line: location.line(),
            column: location.column(),
            path,
            reason,
        });
    };
    let path = if keys.is_empty() {
        "<root>".to_owned()
    } else {
        keys.join(".")
    };
    match &error.kind {
        ErrorKind::AdditionalProperties { got } => {
            let Node::Map(entries) = &node.value else {
                unreachable!("additionalProperties applies to mappings");
            };
            let reason = format!("unknown field; expected one of {}", expected_keys(&keys).join(", "));
            for key in got {
                let (location, _) = entries[key.as_ref()];
                let child = keys.iter().copied().chain([key.as_ref()]).collect::<Vec<_>>().join(".");
                push(location, child, reason.clone());
            }
        }
        kind => push(node.referenced, path, reason(kind, &node.value)),
    }
}

/// Renders a value-free reason for a violation of `kind` by `value`.
fn reason(kind: &ErrorKind<'_, '_>, value: &Node) -> String {
    match kind {
        ErrorKind::Type { want, .. } => format!("expected {}, found {}", expected_types(*want), value.kind()),
        ErrorKind::Enum { want } => {
            let names: Vec<&str> = want.iter().filter_map(Value::as_str).collect();
            format!("expected one of {}", names.join(", "))
        }
        ErrorKind::Minimum { want, .. } => format!("must be at least {want}"),
        ErrorKind::Maximum { want, .. } => format!("must be at most {want}"),
        kind => {
            let keyword = kind
                .keyword_path()
                .map_or_else(|| "schema".to_owned(), |path| path.keyword.to_owned());
            format!("does not match the config schema ({keyword})")
        }
    }
}

/// Names the expected YAML types for a type error.
///
/// `string` is dropped from integer and boolean unions, where it only
/// admits `${}` before casting.
fn expected_types(want: Types) -> String {
    let scalar = want.contains(Type::Integer) || want.contains(Type::Boolean);
    let names: Vec<&str> = want
        .iter()
        .filter(|t| *t != Type::Null && !(scalar && *t == Type::String))
        .map(|t| match t {
            Type::Null => "null",
            Type::Boolean => "boolean",
            Type::Number => "number",
            Type::Integer => "integer",
            Type::String => "string",
            Type::Array => "sequence",
            Type::Object => "mapping",
        })
        .collect();
    names.join(" or ")
}

/// Lists the keys accepted inside the mapping at `keys`, in setting order.
fn expected_keys(keys: &[&str]) -> Vec<&'static str> {
    let mut seen = HashSet::new();
    SETTINGS
        .iter()
        .filter_map(|setting| {
            let mut segments = setting.path.split('.');
            keys.iter()
                .all(|key| segments.next() == Some(key))
                .then(|| segments.next())?
        })
        .filter(|key| seen.insert(*key))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use super::*;
    use crate::database::DatabaseBackend;
    use crate::env::Kind;
    use crate::interpolate;
    use crate::log::LogLevel;
    use crate::node;
    use crate::pii::PiiOperator;
    use crate::ssh::StrictHostKeyChecking;
    use crate::transport::Transport;

    pub(crate) fn document() -> Value {
        serde_json::from_str(SCHEMA).expect("valid JSON")
    }

    /// Returns the property schema for a dotted `path`.
    pub(crate) fn property<'a>(document: &'a Value, path: &str) -> Option<&'a Value> {
        path.split('.')
            .try_fold(document, |schema, key| schema.get("properties")?.get(key))
    }

    fn types(schema: &Value) -> Vec<&str> {
        schema["type"]
            .as_array()
            .expect("type is an array")
            .iter()
            .filter_map(Value::as_str)
            .collect()
    }

    /// Collects the dotted path of every leaf property.
    fn leaves(schema: &Value, prefix: &str, out: &mut Vec<String>) {
        let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
            out.push(prefix.to_owned());
            return;
        };
        for (key, child) in properties {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            leaves(child, &path, out);
        }
    }

    /// The schema `enum` for `choices`: every name, then `null` for unset.
    fn choices<T>(choices: &[(&str, T)]) -> Vec<Value> {
        choices
            .iter()
            .map(|(name, _)| Value::from(*name))
            .chain([Value::Null])
            .collect()
    }

    fn enum_choices(path: &str) -> Vec<Value> {
        match path {
            "transport" => choices(Transport::CHOICES),
            "log_level" => choices(LogLevel::CHOICES),
            "database.backend" => choices(DatabaseBackend::CHOICES),
            "pii.operator" => choices(PiiOperator::CHOICES),
            "ssh.strict_host_key_checking" => choices(StrictHostKeyChecking::CHOICES),
            other => panic!("no CHOICES for enum setting {other}"),
        }
    }

    #[test]
    fn schema_matches_the_setting_table() {
        let document = document();
        let mut paths = Vec::new();
        leaves(&document, "", &mut paths);
        let settings: Vec<&str> = SETTINGS.iter().map(|s| s.path).collect();
        assert_eq!(paths.len(), settings.len(), "schema leaves: {paths:?}");
        for setting in SETTINGS {
            let schema = property(&document, setting.path).unwrap_or_else(|| panic!("{} missing", setting.path));
            let path = setting.path;
            match setting.kind {
                Kind::Text | Kind::Path { .. } => assert_eq!(types(schema), ["string", "null"], "{path}"),
                Kind::Bool => assert_eq!(types(schema), ["boolean", "string", "null"], "{path}"),
                Kind::Int { min, max } => {
                    assert_eq!(types(schema), ["integer", "string", "null"], "{path}");
                    assert_eq!(schema["minimum"], Value::from(min), "{path}");
                    let maximum = (max != u64::MAX).then(|| Value::from(max));
                    assert_eq!(schema.get("maximum"), maximum.as_ref(), "{path}");
                }
                Kind::Enum => {
                    assert_eq!(types(schema), ["string", "null"], "{path}");
                    assert_eq!(schema["enum"], Value::Array(enum_choices(path)), "{path}");
                }
                Kind::List => {
                    assert_eq!(types(schema), ["string", "array", "null"], "{path}");
                    assert_eq!(schema["items"]["type"], "string", "{path}");
                }
            }
        }
    }

    #[test]
    fn sections_are_strict_and_nullable() {
        let document = document();
        for section in
            std::iter::once(&document).chain(["database", "http", "pii", "ssh"].map(|key| &document["properties"][key]))
        {
            assert_eq!(section["additionalProperties"], false);
            assert_eq!(types(section), ["object", "null"]);
        }
    }

    #[test]
    fn schema_id_is_the_published_url() {
        assert_eq!(document()["$id"], URL);
    }

    /// Collects every keyword used by the schema at any depth.
    fn keywords(schema: &Value, out: &mut HashSet<String>) {
        let Some(object) = schema.as_object() else {
            return;
        };
        for (keyword, value) in object {
            out.insert(keyword.clone());
            match keyword.as_str() {
                "properties" => {
                    for child in value.as_object().expect("properties is an object").values() {
                        keywords(child, out);
                    }
                }
                "items" => keywords(value, out),
                _ => {}
            }
        }
    }

    #[test]
    fn every_schema_keyword_has_a_rendered_reason() {
        // Validation keywords with their own reason in `reason` / `collect`,
        // plus annotations and applicators that never fail on their own.
        const MAPPED: &[&str] = &[
            "type",
            "enum",
            "minimum",
            "maximum",
            "additionalProperties",
            "properties",
            "items",
            "$schema",
            "$id",
            "title",
            "description",
            "default",
        ];
        let mut used = HashSet::new();
        keywords(&document(), &mut used);
        let unmapped: Vec<&String> = used.iter().filter(|k| !MAPPED.contains(&k.as_str())).collect();
        assert!(unmapped.is_empty(), "add a reason for {unmapped:?}");
    }

    #[test]
    fn defaults_validate_against_their_own_schema() {
        let document = document();
        let (schemas, index) = compile();
        for setting in SETTINGS {
            let Some(default) = property(&document, setting.path).and_then(|s| s.get("default")) else {
                continue;
            };
            // Nest the default under its keys and validate it as a whole file.
            let instance = setting
                .path
                .rsplit('.')
                .fold(default.clone(), |value, key| serde_json::json!({ key: value }));
            assert!(schemas.validate(&instance, index).is_ok(), "{}", setting.path);
        }
    }

    fn check(yaml: &str) -> Result<(), Vec<String>> {
        let mut root = node::parse(yaml).expect("valid yaml");
        assert!(interpolate::interpolate(&mut root, &|_| None).errors.is_empty());
        validate(&mut root)
    }

    #[test]
    fn errors_point_at_keys_and_values_without_echoing_them() {
        let errors = check("database:\n  hots: s3cret\n  port: -5\nlog_level: s3cret\n").expect_err("invalid");
        assert_eq!(
            errors,
            [
                "2:3: database.hots: unknown field; expected one of backend, host, port, user, password, name, \
                 charset, ssl, ssl_ca, ssl_cert, ssl_key, ssl_verify_cert, read_only, max_pool_size, \
                 connection_timeout, query_timeout, page_size",
                "3:9: database.port: must be at least 0",
                "4:12: log_level: expected one of error, warn, info, debug, trace",
            ]
        );
        assert!(!errors.concat().contains("s3cret"));
    }

    #[test]
    fn unions_name_the_yaml_types() {
        let errors =
            check("database:\n  ssl: 1\n  host: 2\nhttp:\n  allowed_hosts: {a: b}\npii: []\n").expect_err("invalid");
        assert_eq!(
            errors,
            [
                "2:8: database.ssl: expected boolean, found integer",
                "3:9: database.host: expected string, found integer",
                "5:18: http.allowed_hosts: expected string or sequence, found mapping",
                "6:6: pii: expected mapping, found sequence",
            ]
        );
    }

    #[test]
    fn floats_are_rejected_even_when_whole() {
        let errors = check("transport: 1.5\ndatabase:\n  port: 5432.0\n  host: 2.0\n").expect_err("invalid");
        assert_eq!(
            errors,
            [
                "1:12: transport: expected string, found float",
                "3:9: database.port: expected integer, found float",
                "4:9: database.host: expected string, found float",
            ]
        );
    }

    #[test]
    fn setting_strings_are_cast_and_failures_sorted_with_schema_errors() {
        let mut root = node::parse(
            "database:\n  port: \"5432\"\n  ssl: \"t\"\n  read_only: x-secret\n  host: \"1\"\n  page_size: \"\"\n\
             http:\n  allowed_hosts: \"a, b\"\n  allowed_origins: \"a,,b\"\ntransport: 1\n",
        )
        .expect("valid yaml");
        let errors = validate(&mut root).expect_err("invalid");
        assert_eq!(
            errors,
            [
                "4:14: database.read_only: expected boolean (true/false/1/0/t/f), found string",
                "9:20: http.allowed_origins: empty item in comma-separated list",
                "10:12: transport: expected string, found integer",
            ]
        );
        assert!(!errors.concat().contains("secret"));
        let at = |path: &str| {
            path.split('.').fold(&root.value, |node, key| match node {
                Node::Map(entries) => &entries[key].1.value,
                other => panic!("not a mapping: {other:?}"),
            })
        };
        assert_eq!(at("database.port"), &Node::Int(5432));
        assert_eq!(at("database.ssl"), &Node::Bool(true));
        assert_eq!(at("database.host"), &Node::Str("1".into()));
        assert_eq!(at("database.page_size"), &Node::Null, "empty is unset");
        let Node::Seq(hosts) = at("http.allowed_hosts") else {
            panic!("allowed_hosts is split");
        };
        assert_eq!(hosts.len(), 2);
    }

    #[test]
    fn nulls_and_empty_files_are_valid() {
        assert_eq!(check(""), Ok(()));
        assert_eq!(check("transport:\ndatabase:\nssh: ~\n"), Ok(()));
    }

    #[test]
    fn failing_one_of_is_one_problem_at_its_location() {
        let schema = serde_json::json!({
            "properties": {"x": {"oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]}}
        });
        let mut compiler = Compiler::new();
        compiler.add_resource("urn:test", schema).expect("absolute");
        let mut schemas = Schemas::new();
        let index = compiler.compile("urn:test", &mut schemas).expect("compiles");
        let root = node::parse("x: [1, 2]\n").expect("valid yaml");
        let instance = to_json(&root.value);
        let error = schemas.validate(&instance, index).expect_err("invalid");
        let mut problems = Vec::new();
        collect(&error, &root, &mut problems);
        assert_eq!(
            problems,
            [Problem {
                line: 1,
                column: 4,
                path: "x".into(),
                reason: "does not match the config schema (oneOf)".into(),
            }]
        );
    }

    /// Returns the names `${NAME:?…}` / `${NAME?…}` require in `yaml`.
    fn required_vars(yaml: &str) -> Vec<String> {
        yaml.split("${")
            .skip(1)
            .filter_map(|rest| {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                let after = &rest[name.len()..];
                (after.starts_with(":?") || after.starts_with('?')).then_some(name)
            })
            .collect()
    }

    #[test]
    fn docs_yaml_examples_match_the_schema() {
        let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/content/docs");
        let mut checked = 0;
        for entry in std::fs::read_dir(&docs).expect("docs directory") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|ext| ext != "mdx") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable doc");
            let mut lines = text.lines().enumerate();
            while let Some((start, line)) = lines.next() {
                if line.trim() != "```yaml" {
                    continue;
                }
                let block: Vec<&str> = lines
                    .by_ref()
                    .map(|(_, l)| l)
                    .take_while(|l| l.trim() != "```")
                    .collect();
                let yaml = block.join("\n");
                let required = required_vars(&yaml);
                let lookup = |name: &str| required.iter().any(|r| r == name).then(|| "placeholder".to_owned());
                let mut root = node::parse(&yaml).unwrap_or_else(|e| panic!("{}:{}: {e}", path.display(), start + 1));
                let outcome = interpolate::interpolate(&mut root, &lookup);
                assert!(
                    outcome.errors.is_empty(),
                    "{}:{}: {:?}",
                    path.display(),
                    start + 1,
                    outcome.errors
                );
                if let Err(errors) = validate(&mut root) {
                    panic!("{}:{}: {errors:?}", path.display(), start + 1);
                }
                checked += 1;
            }
        }
        assert!(checked >= 4, "found only {checked} yaml examples");
    }
}

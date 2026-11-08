//! JSON-schema generators for MCP tool input and output schemas.
//!
//! These helpers replace the default [`rmcp::handler::server::router::tool::ToolBase`]
//! schema generators with versions that strip the four metadata fields no MCP
//! client consumes — root `$schema`, root `title`, root `description`, and the
//! `$defs` / `$ref` indirection — while preserving every keyword the model and
//! clients actually use. In output schemas, optional properties' nullable
//! `type` arrays collapse to a single `type` so they stay portable to
//! single-type dialects. Each backend's `ToolBase` impls override
//! [`input_schema`] / [`output_schema`] to call into this module so the input
//! and output shapes are established at schema-generation time, not by
//! post-processing the `tools/list` response.
//!
//! [`input_schema`]: rmcp::handler::server::router::tool::ToolBase::input_schema
//! [`output_schema`]: rmcp::handler::server::router::tool::ToolBase::output_schema

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use schemars::Schema;
use schemars::generate::SchemaSettings;
use schemars::transform::{RecursiveTransform, Transform as _};
use serde_json::Value;

thread_local! {
    static SCHEMA_CACHE: Mutex<HashMap<(TypeId, bool, bool), Arc<JsonObject>>> = Mutex::new(HashMap::new());
}

/// Returns the input JSON schema for the tool parameter type `T`.
///
/// Strips root `$schema`, `title`, and `description`, and inlines every
/// `$defs` / `$ref` indirection. When `pinned` is `true`, also strips the
/// root `database` property and removes it from `required` — used for tools
/// registered against a database name pinned in config, where the field is
/// not part of the client-facing input. Nullable `type` arrays are kept, since
/// clients may send an explicit `null` and the server accepts it. Results are
/// cached per `(TypeId, pinned)` in a thread-local map, so repeated calls for
/// the same `T` and `pinned` return the same [`Arc`].
///
/// # Panics
///
/// Panics if `T`'s `JsonSchema` impl produces a non-object JSON value, which
/// would indicate a broken derive.
#[must_use]
pub fn input_schema<T: JsonSchema + Any>(pinned: bool) -> Arc<JsonObject> {
    cached::<T>(pinned, false)
}

/// Returns the output JSON schema for the tool result type `T`.
///
/// Uses the same cache and generation pipeline as [`input_schema`], plus
/// collapsing `type: [T, "null"]` to `type: T` on optional properties, and
/// enforces the MCP-spec invariant that the schema's root `type` is `"object"`.
/// Output schemas never carry a pinned/unpinned variant, so the cache key is
/// fixed at `pinned = false`.
///
/// # Panics
///
/// Panics when `T`'s schema does not have `"type": "object"` at the root, or
/// when `T`'s `JsonSchema` impl produces a non-object JSON value.
#[must_use]
pub fn output_schema<T: JsonSchema + Any>() -> Arc<JsonObject> {
    let schema = cached::<T>(false, true);
    match schema.get("type") {
        Some(Value::String(t)) if t == "object" => schema,
        other => panic!(
            "Invalid output schema for type `{}`: root `type` must be \"object\", got {:?}",
            type_name::<T>(),
            other,
        ),
    }
}

/// Returns the cached schema for `T`, building it on first use.
fn cached<T: JsonSchema + Any>(pinned: bool, output: bool) -> Arc<JsonObject> {
    SCHEMA_CACHE.with(|cache| {
        cache
            .lock()
            .expect("schema cache poisoned")
            .entry((TypeId::of::<T>(), pinned, output))
            .or_insert_with(|| Arc::new(build::<T>(pinned, output)))
            .clone()
    })
}

/// Builds the JSON schema object for `T` via schemars draft-2020-12.
///
/// Configures `inline_subschemas = true` to fold every `$defs` / `$ref` into
/// the parent, `meta_schema = None` to suppress the root `$schema` key
/// natively (schemars only inserts it when this is `Some`), and root-only
/// transforms that strip the `title`/`description` schemars derives from the
/// type's name and doc-comment, and — when `pinned` is `true` — strip the
/// root `database` property and remove it from the `required` array. When
/// `output` is `true`, a recursive transform collapses nullable `type` arrays
/// on optional properties.
fn build<T: JsonSchema>(pinned: bool, output: bool) -> JsonObject {
    let value = SchemaSettings::draft2020_12()
        .with(|s| {
            s.inline_subschemas = true;
            s.meta_schema = None;
        })
        .with_transform(strip_root_metadata)
        .with_transform(move |schema: &mut Schema| {
            if output {
                RecursiveTransform(strip_null_type).transform(schema);
            }
        })
        .with_transform(move |schema: &mut Schema| {
            if pinned {
                strip_root_database(schema);
            }
        })
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();

    let Value::Object(object) = value else {
        panic!("schema for `{}` did not produce a JSON object", type_name::<T>());
    };
    object
}

/// Removes the root `title` and `description` keys MCP clients ignore.
///
/// Works as a schemars [`Transform`] via the
/// `impl<F: FnMut(&mut Schema)> Transform for F` blanket. Root-only: the
/// schemars generator calls each transform once on the root schema and never
/// recurses into subschemas unless the transform itself calls
/// [`transform_subschemas`][schemars::transform::transform_subschemas].
fn strip_root_metadata(schema: &mut Schema) {
    if let Some(object) = schema.as_object_mut() {
        object.remove("title");
        object.remove("description");
    }
}

/// Collapses `type: [T, "null"]` to `type: T` on optional object properties.
///
/// Applied to output schemas only. An optional output property (one not
/// listed in `required`) is skipped when `None`, so it is never sent as
/// `null`. A single `type` keeps the schema portable to clients that map tool
/// schemas onto single-type dialects (e.g. Gemini function declarations).
/// Also drops the matching `default: null`. Only an exact two-entry
/// `[T, "null"]` array collapses; required properties and array items keep
/// their nullable type, since `null` is a real value there. Applied to every
/// subschema via [`RecursiveTransform`].
fn strip_null_type(schema: &mut Schema) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    let required = match object.get("required") {
        Some(Value::Array(names)) => names.clone(),
        _ => Vec::new(),
    };
    let Some(Value::Object(properties)) = object.get_mut("properties") else {
        return;
    };
    for (name, property) in properties.iter_mut() {
        if required.iter().any(|value| value.as_str() == Some(name)) {
            continue;
        }
        let Value::Object(property) = property else {
            continue;
        };
        let kept = match property.get("type") {
            Some(Value::Array(types)) if types.len() == 2 && types.contains(&Value::from("null")) => {
                types.iter().find(|value| value.as_str() != Some("null")).cloned()
            }
            _ => None,
        };
        let Some(kept) = kept else {
            continue;
        };
        property.insert("type".into(), kept);
        if property.get("default") == Some(&Value::Null) {
            property.remove("default");
        }
    }
}

/// Removes the root `database` property and its entry in `required`.
///
/// Applied only for tools registered against a pinned database, where the
/// runtime always passes `None` for `database` and the client never supplies
/// it. Root-only — see [`strip_root_metadata`].
fn strip_root_database(schema: &mut Schema) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        properties.remove("database");
    }
    if let Some(Value::Array(required)) = object.get_mut("required") {
        required.retain(|value| value.as_str() != Some("database"));
        if required.is_empty() {
            object.remove("required");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build, input_schema, output_schema};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use serde_json::json;

    /// Outer fixture struct whose schemars docs we expect to be stripped.
    #[derive(Deserialize, Serialize, JsonSchema)]
    #[schemars(title = "FixtureTitle", description = "Fixture root description.")]
    struct Fixture {
        /// Doc-comment kept on the property — must survive schema generation.
        name: String,
        nested: Nested,
    }

    #[derive(Deserialize, Serialize, JsonSchema)]
    struct Nested {
        value: u32,
    }

    /// Distinct fixture to confirm the cache keys by type, not by name.
    #[derive(Deserialize, Serialize, JsonSchema)]
    struct OtherFixture {
        value: u32,
    }

    fn contains_key(value: &Value, key: &str) -> bool {
        match value {
            Value::Object(map) => map.contains_key(key) || map.values().any(|v| contains_key(v, key)),
            Value::Array(items) => items.iter().any(|v| contains_key(v, key)),
            _ => false,
        }
    }

    /// Fixture with a `database` property to exercise the pinned-strip transform.
    #[derive(Deserialize, Serialize, JsonSchema)]
    struct PinnedFixture {
        query: String,
        #[serde(default)]
        database: Option<String>,
    }

    #[test]
    fn input_schema_strips_dollar_schema_title_and_description() {
        let schema = input_schema::<Fixture>(false);
        assert!(!schema.contains_key("$schema"), "root $schema not stripped: {schema:?}");
        assert!(!schema.contains_key("title"), "root title not stripped: {schema:?}");
        assert!(
            !schema.contains_key("description"),
            "root description not stripped: {schema:?}"
        );
        assert_eq!(schema.get("type"), Some(&Value::String("object".into())));
    }

    #[test]
    fn input_schema_inlines_nested_subschemas() {
        let schema = input_schema::<Fixture>(false);
        let value = Value::Object((*schema).clone());
        assert!(!contains_key(&value, "$defs"), "$defs not inlined: {value}");
        assert!(!contains_key(&value, "$ref"), "$ref not inlined: {value}");
    }

    #[test]
    fn input_schema_caches_by_type_and_pinned() {
        let first = input_schema::<Fixture>(false);
        let second = input_schema::<Fixture>(false);
        assert!(
            std::sync::Arc::ptr_eq(&first, &second),
            "same (type, pinned) should return cached Arc"
        );
        let other = input_schema::<OtherFixture>(false);
        assert!(
            !std::sync::Arc::ptr_eq(&first, &other),
            "different types must not share cache entry"
        );
        let pinned = input_schema::<PinnedFixture>(true);
        let unpinned = input_schema::<PinnedFixture>(false);
        assert!(
            !std::sync::Arc::ptr_eq(&pinned, &unpinned),
            "same type with different pinned flags must not share cache entry"
        );
    }

    #[test]
    fn output_schema_accepts_object_root() {
        let schema = output_schema::<Fixture>();
        assert_eq!(schema.get("type"), Some(&Value::String("object".into())));
        let again = output_schema::<Fixture>();
        assert!(std::sync::Arc::ptr_eq(&schema, &again));
    }

    #[test]
    #[should_panic(expected = "root `type` must be \"object\"")]
    fn output_schema_panics_on_non_object_root() {
        let _ = output_schema::<u32>();
    }

    #[test]
    fn build_preserves_properties() {
        let schema = build::<Fixture>(false, false);
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties survive generation");
        assert!(properties.contains_key("name"));
        assert!(properties.contains_key("nested"));
        let name = properties.get("name").and_then(Value::as_object).unwrap();
        assert_eq!(
            name.get("description").and_then(Value::as_str),
            Some("Doc-comment kept on the property — must survive schema generation."),
            "per-property descriptions must survive schema generation"
        );
    }

    /// Fixture with optional fields that schemars renders as nullable.
    #[derive(Deserialize, Serialize, JsonSchema)]
    struct NullableFixture {
        #[serde(default)]
        search: Option<String>,
        #[serde(default)]
        nested: Option<Vec<Option<u32>>>,
        #[serde(default)]
        unit: Option<()>,
        inner: NullableInner,
    }

    #[derive(Deserialize, Serialize, JsonSchema)]
    struct NullableInner {
        #[serde(default)]
        cursor: Option<String>,
    }

    #[test]
    fn output_schema_collapses_nullable_optional_properties() {
        let schema = output_schema::<NullableFixture>();
        let value = Value::Object((*schema).clone());
        let search = &value["properties"]["search"];
        assert_eq!(search["type"], "string", "nullable type not collapsed: {value}");
        assert!(search.get("default").is_none(), "`default: null` not dropped: {value}");
        let nested = &value["properties"]["nested"];
        assert_eq!(nested["type"], "array", "nullable type not collapsed: {value}");
        assert_eq!(
            nested["items"]["type"],
            json!(["integer", "null"]),
            "array items must keep `null`: {value}"
        );
        assert_eq!(
            value["properties"]["unit"]["type"], "null",
            "a null-only type must stay intact: {value}"
        );
        let inner = &value["properties"]["inner"]["properties"]["cursor"];
        assert_eq!(inner["type"], "string", "nested property not collapsed: {value}");
    }

    #[test]
    fn input_schema_keeps_nullable_optional_properties() {
        let schema = input_schema::<NullableFixture>(false);
        let value = Value::Object((*schema).clone());
        assert_eq!(
            value["properties"]["search"]["type"],
            json!(["string", "null"]),
            "input must keep `null`, the server accepts it: {value}"
        );
    }

    #[test]
    fn pinned_input_schema_strips_database_property_and_required() {
        let unpinned = input_schema::<PinnedFixture>(false);
        let unpinned_props = unpinned.get("properties").and_then(Value::as_object).unwrap();
        assert!(unpinned_props.contains_key("database"));
        assert!(unpinned_props.contains_key("query"));

        let pinned = input_schema::<PinnedFixture>(true);
        let pinned_props = pinned.get("properties").and_then(Value::as_object).unwrap();
        assert!(
            !pinned_props.contains_key("database"),
            "pinned schema must drop `database`: {pinned:?}"
        );
        assert!(
            pinned_props.contains_key("query"),
            "pinned schema preserves other props"
        );

        let required = pinned.get("required").and_then(Value::as_array);
        if let Some(required) = required {
            assert!(
                !required.iter().any(|v| v.as_str() == Some("database")),
                "pinned schema must drop `database` from required: {required:?}"
            );
        }
    }
}

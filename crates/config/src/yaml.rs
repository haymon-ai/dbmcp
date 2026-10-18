//! Config-file YAML: a span-carrying document tree and its typed deserializer.
//!
//! The file is parsed once into a [`Node`] tree that keeps each node's
//! line and column; string values (never keys) are interpolated while
//! parsing. The tree is then deserialized into the typed layer by a
//! path-aware [`Deserializer`](serde::Deserializer) that:
//!
//! - coerces a string into a bool or integer field with the same parsers
//!   the env layer uses (`BoolishValueParser`; decimal integers);
//! - never coerces the other way: a number in a string field is an error;
//! - reports type errors without echoing the offending value;
//! - tags every error with the config path and location of the node it
//!   came from.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;

use clap::builder::{BoolishValueParser, TypedValueParser};
use serde::de::{self, DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, SeqAccess, Unexpected, Visitor};
use serde::{Deserialize, de::Error as _};
use serde_saphyr::budget::BudgetBreach;
use serde_saphyr::{Location, PropertySyntax, Spanned};

/// Document node with the location it was written at.
pub(crate) type Node = Spanned<Value>;

/// Value of a YAML node.
#[derive(Clone, Debug)]
pub(crate) enum Value {
    /// `null`, `~`, or an empty value.
    Null,
    /// Native boolean (`true` / `false`).
    Bool(bool),
    /// Native signed integer.
    Int(i64),
    /// Native unsigned integer too large for `i64`.
    UInt(u64),
    /// Native float.
    Float(f64),
    /// String scalar.
    Str(String),
    /// Sequence.
    Seq(Vec<Node>),
    /// Mapping with string keys, in document order.
    Map(Vec<(Spanned<String>, Node)>),
}

impl Value {
    /// Describes this value for a type error; [`kind`] renders it value-free.
    fn unexpected(&self) -> Unexpected<'_> {
        match self {
            Self::Null => Unexpected::Unit,
            Self::Bool(v) => Unexpected::Bool(*v),
            Self::Int(v) => Unexpected::Signed(*v),
            Self::UInt(v) => Unexpected::Unsigned(*v),
            Self::Float(v) => Unexpected::Float(*v),
            Self::Str(v) => Unexpected::Str(v),
            Self::Seq(_) => Unexpected::Seq,
            Self::Map(_) => Unexpected::Map,
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a YAML value")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(i64::try_from(v).map_or(Value::UInt(v), Value::Int))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::Str(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::Str(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Node>()? {
            items.push(item);
        }
        Ok(Value::Seq(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut entries = Vec::new();
        while let Some(key) = map.next_key::<Spanned<String>>()? {
            entries.push((key, map.next_value::<Node>()?));
        }
        Ok(Value::Map(entries))
    }
}

/// Why [`parse`] failed.
#[derive(Debug)]
pub(crate) struct ParseError {
    /// Where it failed, when known.
    pub(crate) location: Option<Location>,
    /// Parser message, without its `at line …, column …` suffix.
    pub(crate) message: String,
    /// A substitution failed, not the YAML itself.
    pub(crate) interpolation: bool,
}

/// Parses a YAML document into a node tree; an empty document is a null node.
///
/// String values (never keys) are interpolated from `properties` with
/// Docker Compose syntax; an unset variable becomes `""`.
///
/// # Errors
///
/// Returns the location (when known) and parser message for invalid YAML
/// or the first failed substitution.
pub(crate) fn parse(text: &str, properties: HashMap<String, String>) -> Result<Node, ParseError> {
    let mut options = serde_saphyr::options! {
        strict_booleans: true,
        with_snippet: false,
        property_syntax: PropertySyntax::DockerCompose,
    };
    options = options.with_properties(properties);
    let root = serde_saphyr::from_str_with_options::<Option<Node>>(text, options).map_err(|e| {
        let inner = innermost(&e);
        let message = if matches!(inner, serde_saphyr::Error::InvalidPropertyName { .. }) {
            // The parser quotes the rest of the scalar, which may be a secret.
            INVALID_SUBSTITUTION.to_owned()
        } else {
            let message = e.to_string();
            match message.rfind(" at line ") {
                Some(end) if e.location().is_some() => message[..end].to_owned(),
                _ => message,
            }
        };
        ParseError {
            location: e.location(),
            message,
            interpolation: is_interpolation_error(inner),
        }
    })?;
    Ok(root.unwrap_or_else(|| Spanned::new(Value::Null, Location::UNKNOWN, Location::UNKNOWN)))
}

/// Value-free message for a malformed `${...}`.
const INVALID_SUBSTITUTION: &str =
    "invalid substitution; expected `${NAME}` optionally followed by :-, -, :?, ?, :+, or +, and a closing `}`";

/// Returns the error inside any snippet or alias wrappers.
fn innermost(mut error: &serde_saphyr::Error) -> &serde_saphyr::Error {
    while let serde_saphyr::Error::AliasError { error: inner, .. }
    | serde_saphyr::Error::WithSnippet { error: inner, .. } = error
    {
        error = inner;
    }
    error
}

/// Returns `true` when `error` comes from a failed substitution.
fn is_interpolation_error(error: &serde_saphyr::Error) -> bool {
    matches!(
        error,
        serde_saphyr::Error::UnresolvedProperty { .. }
            | serde_saphyr::Error::InvalidPropertyName { .. }
            | serde_saphyr::Error::PropertyRequiredButUnset { .. }
            | serde_saphyr::Error::PropertyRequiredButEmpty { .. }
            | serde_saphyr::Error::Budget {
                breach: BudgetBreach::PropertyExpansionDepth { .. } | BudgetBreach::PropertyInterpolationWork { .. },
                ..
            }
    )
}

/// Returns `line:column` of `location`.
pub(crate) fn line_col(location: &Location) -> String {
    format!("{}:{}", location.line(), location.column())
}

/// Appends `key` to the config path `parent`.
fn join(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_owned()
    } else {
        format!("{parent}.{key}")
    }
}

/// Returns the config path and location of every non-null setting under `node`.
///
/// A list counts as one setting.
pub(crate) fn locations(node: &Node) -> Vec<(String, Location)> {
    let mut found = Vec::new();
    walk(node, "", &mut found);
    found
}

/// Records `node` at `path`, then the entries of a mapping.
fn walk(node: &Node, path: &str, found: &mut Vec<(String, Location)>) {
    match &node.value {
        Value::Null => {}
        Value::Map(entries) => {
            for (key, value) in entries {
                walk(value, &join(path, &key.value), found);
            }
        }
        _ => found.push((path.to_owned(), node.referenced)),
    }
}

/// Typed-deserialization failure, tagged with where it happened.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct DeError {
    /// Value-free reason.
    pub(crate) message: String,
    /// Config path of the failing node, filled by the innermost node.
    pub(crate) path: Option<String>,
    /// Location of the failing node.
    pub(crate) location: Option<Location>,
}

/// Value-free name of an [`Unexpected`] kind.
fn kind(unexpected: &Unexpected<'_>) -> &'static str {
    match unexpected {
        Unexpected::Bool(_) => "boolean",
        Unexpected::Unsigned(_) | Unexpected::Signed(_) => "integer",
        Unexpected::Float(_) => "float",
        Unexpected::Char(_) | Unexpected::Str(_) => "string",
        Unexpected::Bytes(_) => "bytes",
        Unexpected::Unit => "null",
        Unexpected::Option => "option",
        Unexpected::NewtypeStruct => "newtype struct",
        Unexpected::Seq => "sequence",
        Unexpected::Map => "mapping",
        Unexpected::Enum
        | Unexpected::UnitVariant
        | Unexpected::NewtypeVariant
        | Unexpected::TupleVariant
        | Unexpected::StructVariant => "enum",
        Unexpected::Other(_) => "value",
    }
}

/// Renders `names` as `` `a`, `b` ``.
pub(crate) fn one_of<S: AsRef<str>>(names: &[S]) -> String {
    names
        .iter()
        .map(|name| format!("`{}`", name.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Overrides every constructor that would echo a value with a value-free message.
impl de::Error for DeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self {
            message: msg.to_string(),
            path: None,
            location: None,
        }
    }

    fn invalid_type(unexp: Unexpected<'_>, exp: &dyn de::Expected) -> Self {
        Self::custom(format_args!("expected {exp}, found {}", kind(&unexp)))
    }

    fn invalid_value(_unexp: Unexpected<'_>, exp: &dyn de::Expected) -> Self {
        Self::custom(format_args!("invalid value; expected {exp}"))
    }

    fn invalid_length(_len: usize, exp: &dyn de::Expected) -> Self {
        Self::custom(format_args!("invalid length; expected {exp}"))
    }

    fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
        Self::custom(format_args!("unknown variant; expected one of {}", one_of(expected)))
    }

    fn unknown_field(_field: &str, expected: &'static [&'static str]) -> Self {
        if expected.is_empty() {
            Self::custom("unknown field; no fields are accepted here")
        } else {
            Self::custom(format_args!("unknown field; expected one of {}", one_of(expected)))
        }
    }

    fn missing_field(field: &'static str) -> Self {
        Self::custom(format_args!("missing field `{field}`"))
    }

    fn duplicate_field(field: &'static str) -> Self {
        Self::custom(format_args!("duplicate field `{field}`"))
    }
}

impl DeError {
    /// Tags an untagged error with `path` and `location`.
    fn at(mut self, path: &str, location: Location) -> Self {
        if self.path.is_none() {
            self.path = Some(path.to_owned());
            self.location = Some(location);
        }
        self
    }
}

/// Deserializes `T` from `node`, the document root.
///
/// Errors are tagged where a child node is entered (and here, for the
/// root), so every error carries the path and location of the node it
/// came from; the root's path is empty.
///
/// # Errors
///
/// Returns the first [`DeError`], tagged with its config path and location.
pub(crate) fn from_node<T: de::DeserializeOwned>(node: &Node) -> Result<T, DeError> {
    T::deserialize(NodeDeserializer { node, path: "" }).map_err(|e| e.at("", node.referenced))
}

/// Deserializes `seed` from `node` at `path`, tagging any untagged error with them.
fn child<'de, T: DeserializeSeed<'de>>(seed: T, node: &Node, path: &str) -> Result<T::Value, DeError> {
    seed.deserialize(NodeDeserializer { node, path })
        .map_err(|e| e.at(path, node.referenced))
}

/// Deserializer over one node at config path `path`.
struct NodeDeserializer<'a> {
    node: &'a Node,
    path: &'a str,
}

impl NodeDeserializer<'_> {
    fn invalid_type<T>(&self, expected: &dyn de::Expected) -> Result<T, DeError> {
        Err(DeError::invalid_type(self.node.value.unexpected(), expected))
    }

    fn integer<'de, V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Int(v) => visitor.visit_i64(*v),
            Value::UInt(v) => visitor.visit_u64(*v),
            Value::Str(text) => match parse_integer(text) {
                Some(Ok(v)) => visitor.visit_i64(v),
                Some(Err(v)) => visitor.visit_u64(v),
                None => Err(DeError::custom("expected an integer")),
            },
            _ => self.invalid_type(&visitor),
        }
    }
}

/// Parses a decimal integer: `Ok` for values that fit `i64`, else `Err` for `u64`.
fn parse_integer(text: &str) -> Option<Result<i64, u64>> {
    text.parse::<i64>()
        .map(Ok)
        .or_else(|_| text.parse::<u64>().map(Err))
        .ok()
}

/// Parses a bool with the env layer's `BoolishValueParser` rules.
fn parse_boolish(text: &str) -> Option<bool> {
    BoolishValueParser::new()
        .parse_ref(&clap::Command::new("dbmcp"), None, OsStr::new(text))
        .ok()
}

macro_rules! deserialize_integers {
    ($($method:ident),*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
            self.integer(visitor)
        }
    )*};
}

impl<'de> Deserializer<'de> for NodeDeserializer<'_> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Null => self.deserialize_unit(visitor),
            Value::Bool(_) => self.deserialize_bool(visitor),
            Value::Int(_) | Value::UInt(_) => self.integer(visitor),
            Value::Float(_) => self.deserialize_f64(visitor),
            Value::Str(_) => self.deserialize_str(visitor),
            Value::Seq(_) => self.deserialize_seq(visitor),
            Value::Map(_) => self.deserialize_map(visitor),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Bool(v) => visitor.visit_bool(*v),
            Value::Str(text) => match parse_boolish(text) {
                Some(v) => visitor.visit_bool(v),
                None => Err(DeError::custom("expected a boolean (true/false, yes/no, on/off, 1/0)")),
            },
            _ => self.invalid_type(&visitor),
        }
    }

    deserialize_integers!(
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64
    );

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.deserialize_f64(visitor)
    }

    #[allow(clippy::cast_precision_loss, reason = "YAML integers given for a float field")]
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Float(v) => visitor.visit_f64(*v),
            Value::Int(v) => visitor.visit_f64(*v as f64),
            _ => self.invalid_type(&visitor),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Str(text) => visitor.visit_str(text),
            _ => self.invalid_type(&visitor),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Null => visitor.visit_unit(),
            _ => self.invalid_type(&visitor),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Seq(items) => visitor.visit_seq(Items {
                items: items.iter().enumerate(),
                path: self.path,
            }),
            _ => self.invalid_type(&visitor),
        }
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match &self.node.value {
            Value::Map(entries) => visitor.visit_map(MapEntries {
                entries: entries.iter(),
                value: None,
                path: self.path,
            }),
            _ => self.invalid_type(&visitor),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        // A null section (`ssh:`, `ssh: ~`) or empty document is an empty one.
        match &self.node.value {
            Value::Null => visitor.visit_map(MapEntries {
                entries: [].iter(),
                value: None,
                path: self.path,
            }),
            _ => self.deserialize_map(visitor),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        i128 u128 char bytes byte_buf unit_struct newtype_struct tuple tuple_struct enum identifier
    }
}

/// Sequence access over child nodes.
struct Items<'a, I> {
    items: I,
    path: &'a str,
}

impl<'de, 'a, I: Iterator<Item = (usize, &'a Node)>> SeqAccess<'de> for Items<'_, I> {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, DeError> {
        self.items
            .next()
            .map(|(i, node)| child(seed, node, &format!("{}[{i}]", self.path)))
            .transpose()
    }
}

/// Map access over key/value child nodes.
struct MapEntries<'a, I> {
    entries: I,
    value: Option<(&'a Node, String)>,
    path: &'a str,
}

impl<'de, 'a, I: Iterator<Item = &'a (Spanned<String>, Node)>> MapAccess<'de> for MapEntries<'a, I> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, DeError> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        let path = join(self.path, &key.value);
        let result = seed
            .deserialize(key.value.as_str().into_deserializer())
            .map_err(|e: DeError| e.at(&path, key.referenced));
        self.value = Some((value, path));
        result.map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, DeError> {
        let (node, path) = self.value.take().ok_or_else(|| DeError::custom("value without key"))?;
        child(seed, node, &path)
    }
}

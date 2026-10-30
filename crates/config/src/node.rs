//! Untyped YAML tree with source locations, parsed by `serde-saphyr`.
//!
//! The loader parses the config file into a [`Node`] tree first: `${}`
//! interpolation and the `DBMCP_*` overlay run on this tree before anything
//! is cast to its type.

use std::fmt;

use indexmap::IndexMap;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_saphyr::{Location, Spanned};

/// A YAML value whose children carry their source location.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    /// `~`, `null`, or an empty value.
    Null,
    /// A YAML 1.2 boolean (`true` / `false`).
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer too large for [`Node::Int`].
    UInt(u64),
    /// A floating-point number.
    Float(f64),
    /// A string scalar.
    Str(String),
    /// A sequence.
    Seq(Vec<Spanned<Node>>),
    /// A mapping with string keys, in file order.
    Map(IndexMap<String, Spanned<Node>>),
}

impl Node {
    /// Names the YAML type for value-free error messages.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Int(_) | Self::UInt(_) => "integer",
            Self::Float(_) => "float",
            Self::Str(_) => "string",
            Self::Seq(_) => "sequence",
            Self::Map(_) => "mapping",
        }
    }
}

/// A [`Node`] that remembers where it was written.
pub(crate) type SpannedNode = Spanned<Node>;

/// Parses YAML `text` into a tree; an empty document yields [`Node::Null`].
///
/// YAML 1.1 booleans (`yes`, `no`, `on`, `off`) stay strings, so an
/// unquoted `strict_host_key_checking: no` keeps its enum meaning.
///
/// # Errors
///
/// Returns the `serde-saphyr` error, rendered without a source snippet.
pub(crate) fn parse(text: &str) -> Result<SpannedNode, serde_saphyr::Error> {
    if text.trim().is_empty() {
        return Ok(Spanned::new(Node::Null, Location::UNKNOWN, Location::UNKNOWN));
    }
    let options = serde_saphyr::options! {
        strict_booleans: true,
        with_snippet: false,
    };
    serde_saphyr::from_str_with_options(text, options)
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a YAML value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Node, E> {
        Ok(Node::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Node, D::Error> {
        Node::deserialize(deserializer)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Node, E> {
        Ok(Node::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Node, E> {
        Ok(Node::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Node, E> {
        Ok(i64::try_from(v).map_or(Node::UInt(v), Node::Int))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Node, E> {
        Ok(Node::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Node, E> {
        Ok(Node::Str(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Node, E> {
        Ok(Node::Str(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<SpannedNode>()? {
            items.push(item);
        }
        Ok(Node::Seq(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut entries = IndexMap::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value::<SpannedNode>()?;
            entries.insert(key, value);
        }
        Ok(Node::Map(entries))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(node: &SpannedNode) -> &IndexMap<String, SpannedNode> {
        match &node.value {
            Node::Map(map) => map,
            other => panic!("expected mapping, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_mapping_with_locations() {
        let root = parse("database:\n  port: 5432\n  host: db\n").expect("valid yaml");
        let database = &map(&root)["database"];
        let port = &map(database)["port"];
        assert_eq!(port.value, Node::Int(5432));
        assert_eq!(port.referenced.line(), 2);
        assert_eq!(port.referenced.column(), 9);
        assert_eq!(map(database)["host"].value, Node::Str("db".into()));
    }

    #[test]
    fn yaml_1_1_booleans_stay_strings() {
        let root = parse("a: no\nb: yes\nc: on\nd: true\n").expect("valid yaml");
        let root = map(&root);
        assert_eq!(root["a"].value, Node::Str("no".into()));
        assert_eq!(root["b"].value, Node::Str("yes".into()));
        assert_eq!(root["c"].value, Node::Str("on".into()));
        assert_eq!(root["d"].value, Node::Bool(true));
    }

    #[test]
    fn quoted_number_stays_string() {
        let root = parse("port: \"5432\"\n").expect("valid yaml");
        assert_eq!(map(&root)["port"].value, Node::Str("5432".into()));
    }

    #[test]
    fn null_forms_parse_as_null() {
        let root = parse("a:\nb: null\nc: ~\nssh:\n").expect("valid yaml");
        for key in ["a", "b", "c", "ssh"] {
            assert_eq!(map(&root)[key].value, Node::Null, "{key}");
        }
    }

    #[test]
    fn sequences_keep_item_locations() {
        let root = parse("list:\n  - a\n  - b\n").expect("valid yaml");
        let Node::Seq(items) = &map(&root)["list"].value else {
            panic!("expected sequence");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].referenced.line(), 3);
    }

    #[test]
    fn empty_document_is_null() {
        assert_eq!(parse("").expect("empty is valid").value, Node::Null);
        assert_eq!(parse("  \n# only a comment\n").map(|n| n.value).ok(), Some(Node::Null));
    }

    #[test]
    fn syntax_error_has_location() {
        let error = parse("a: [1, 2\n").expect_err("unterminated flow sequence");
        assert!(error.location().is_some());
    }
}

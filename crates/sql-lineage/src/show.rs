//! `SHOW` forms, classified from the statement's tokens.
//!
//! The unquoted keywords after `SHOW` match one table of `MySQL` and
//! `MariaDB` forms, modifiers only at their grammar positions; anything
//! else is [`Show::Unsupported`].

use serde::{Deserialize, Serialize};
use sqlparser::tokenizer::Token;

use crate::dialect::Dialect;

/// A `SHOW` form, named by the keywords that start it.
///
/// Every listed form except `WARNINGS`, `ERRORS`, and `Unsupported` lists
/// schema objects, definitions, settings, or counters, never row values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Show {
    /// `SHOW DATABASES` or `SCHEMAS`.
    Databases,
    /// `SHOW [EXTENDED] [FULL] TABLES`.
    Tables,
    /// `SHOW OPEN TABLES`.
    OpenTables,
    /// `SHOW TABLE STATUS`.
    TableStatus,
    /// `SHOW [EXTENDED] [FULL] {COLUMNS | FIELDS}`.
    Columns,
    /// `SHOW [EXTENDED] {INDEX | INDEXES | KEYS}`.
    Index,
    /// `SHOW CREATE TABLE`.
    CreateTable,
    /// `SHOW CREATE VIEW`.
    CreateView,
    /// `SHOW CREATE TRIGGER`.
    CreateTrigger,
    /// `SHOW CREATE FUNCTION`.
    CreateFunction,
    /// `SHOW CREATE PROCEDURE`.
    CreateProcedure,
    /// `SHOW CREATE EVENT`.
    CreateEvent,
    /// `SHOW TRIGGERS`.
    Triggers,
    /// `SHOW EVENTS`.
    Events,
    /// `SHOW FUNCTION STATUS`.
    FunctionStatus,
    /// `SHOW PROCEDURE STATUS`.
    ProcedureStatus,
    /// `SHOW FUNCTION CODE`.
    FunctionCode,
    /// `SHOW PROCEDURE CODE`.
    ProcedureCode,
    /// `SHOW [GLOBAL | SESSION] VARIABLES`.
    Variables,
    /// `SHOW [GLOBAL | SESSION] STATUS`.
    Status,
    /// `SHOW CHARSET` or `CHARACTER SET`.
    Charset,
    /// `SHOW COLLATION`.
    Collation,
    /// `SHOW [STORAGE] ENGINES`.
    Engines,
    /// `SHOW PLUGINS`.
    Plugins,
    /// `SHOW PRIVILEGES`.
    Privileges,
    /// `SHOW [COUNT(*)] WARNINGS`.
    Warnings,
    /// `SHOW [COUNT(*)] ERRORS`.
    Errors,
    /// Any other form.
    Unsupported,
}

impl Show {
    /// Classifies the first unquoted `SHOW` in `sql`, tokenized as `dialect` reads it.
    ///
    /// Tokenizer errors and a missing `SHOW` are [`Show::Unsupported`].
    #[must_use]
    pub fn classify(sql: &str, dialect: Dialect) -> Self {
        dialect
            .tokenize(sql)
            .map_or(Self::Unsupported, |tokens| Self::of(&tokens))
    }

    /// Returns whether the form replays the previous statement's values.
    ///
    /// `WARNINGS` and `ERRORS` quote the values that statement touched.
    #[must_use]
    pub const fn replays_values(self) -> bool {
        match self {
            Self::Warnings | Self::Errors => true,
            Self::Databases
            | Self::Tables
            | Self::OpenTables
            | Self::TableStatus
            | Self::Columns
            | Self::Index
            | Self::CreateTable
            | Self::CreateView
            | Self::CreateTrigger
            | Self::CreateFunction
            | Self::CreateProcedure
            | Self::CreateEvent
            | Self::Triggers
            | Self::Events
            | Self::FunctionStatus
            | Self::ProcedureStatus
            | Self::FunctionCode
            | Self::ProcedureCode
            | Self::Variables
            | Self::Status
            | Self::Charset
            | Self::Collation
            | Self::Engines
            | Self::Plugins
            | Self::Privileges
            | Self::Unsupported => false,
        }
    }

    /// Classifies the first `SHOW` in `tokens`, as [`Show::classify`] does.
    pub(crate) fn of(tokens: &[Token]) -> Self {
        let words = keywords(tokens);
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["DATABASES" | "SCHEMAS", ..] => Self::Databases,
            ["TABLES", ..] | ["FULL" | "EXTENDED", "TABLES", ..] | ["EXTENDED", "FULL", "TABLES", ..] => Self::Tables,
            ["OPEN", "TABLES", ..] => Self::OpenTables,
            ["TABLE", "STATUS", ..] => Self::TableStatus,
            ["COLUMNS" | "FIELDS", ..]
            | ["FULL" | "EXTENDED", "COLUMNS" | "FIELDS", ..]
            | ["EXTENDED", "FULL", "COLUMNS" | "FIELDS", ..] => Self::Columns,
            ["INDEX" | "INDEXES" | "KEYS", ..] | ["EXTENDED", "INDEX" | "INDEXES" | "KEYS", ..] => Self::Index,
            ["CREATE", "TABLE", ..] => Self::CreateTable,
            ["CREATE", "VIEW", ..] => Self::CreateView,
            ["CREATE", "TRIGGER", ..] => Self::CreateTrigger,
            ["CREATE", "FUNCTION", ..] => Self::CreateFunction,
            ["CREATE", "PROCEDURE", ..] => Self::CreateProcedure,
            ["CREATE", "EVENT", ..] => Self::CreateEvent,
            ["TRIGGERS", ..] => Self::Triggers,
            ["EVENTS", ..] => Self::Events,
            ["FUNCTION", "STATUS", ..] => Self::FunctionStatus,
            ["PROCEDURE", "STATUS", ..] => Self::ProcedureStatus,
            ["FUNCTION", "CODE", ..] => Self::FunctionCode,
            ["PROCEDURE", "CODE", ..] => Self::ProcedureCode,
            ["VARIABLES", ..] | ["GLOBAL" | "SESSION", "VARIABLES", ..] => Self::Variables,
            ["STATUS", ..] | ["GLOBAL" | "SESSION", "STATUS", ..] => Self::Status,
            ["CHARSET", ..] | ["CHARACTER", "SET", ..] => Self::Charset,
            ["COLLATION", ..] => Self::Collation,
            ["ENGINES", ..] | ["STORAGE", "ENGINES", ..] => Self::Engines,
            ["PLUGINS", ..] => Self::Plugins,
            ["PRIVILEGES", ..] => Self::Privileges,
            ["WARNINGS", ..] | ["COUNT", "(", "*", ")", "WARNINGS", ..] => Self::Warnings,
            ["ERRORS", ..] | ["COUNT", "(", "*", ")", "ERRORS", ..] => Self::Errors,
            _ => Self::Unsupported,
        }
    }
}

/// Reports whether the `SHOW` in `tokens` filters by `WHERE` or a non-literal `LIKE`.
///
/// A `WHERE` expression, subqueries included, can test any column's value.
pub(crate) fn computed_filter(tokens: &[Token]) -> bool {
    let mut rest = after_show(tokens).filter(|token| !matches!(token, Token::Whitespace(_)));
    while let Some(token) = rest.next() {
        let Some(word) = unquoted(token) else {
            continue;
        };
        if word.eq_ignore_ascii_case("where") {
            return true;
        }
        if (word.eq_ignore_ascii_case("like") || word.eq_ignore_ascii_case("ilike"))
            && !matches!(
                rest.next(),
                Some(Token::SingleQuotedString(_) | Token::DoubleQuotedString(_))
            )
        {
            return true;
        }
    }
    false
}

/// Returns the unquoted keywords after `SHOW`, uppercased, up to the first other token.
///
/// `(`, `*`, and `)` are kept so `COUNT(*)` matches.
fn keywords(tokens: &[Token]) -> Vec<String> {
    let mut words = Vec::new();
    for token in after_show(tokens) {
        match token {
            Token::Whitespace(_) => {}
            Token::Word(word) if word.quote_style.is_none() => words.push(word.value.to_ascii_uppercase()),
            Token::LParen => words.push("(".to_string()),
            Token::Mul => words.push("*".to_string()),
            Token::RParen => words.push(")".to_string()),
            _ => break,
        }
    }
    words
}

/// Returns the tokens after the first unquoted `SHOW`; none without one.
fn after_show(tokens: &[Token]) -> impl Iterator<Item = &Token> {
    tokens
        .iter()
        .skip_while(|token| !unquoted(token).is_some_and(|word| word.eq_ignore_ascii_case("show")))
        .skip(1)
}

/// Returns an unquoted word's text.
fn unquoted(token: &Token) -> Option<&str> {
    match token {
        Token::Word(word) if word.quote_style.is_none() => Some(&word.value),
        _ => None,
    }
}

//! `SHOW` forms, classified from the statement's tokens.
//!
//! [`Show::classify`] reads the unquoted keywords after `SHOW` and matches
//! their prefix against one table of `MySQL` and `MariaDB` forms. Modifiers
//! match only at the positions the servers' grammars put them; every other
//! shape, and every form outside the table, is [`Show::Unsupported`].
//! Quoted words and literals never act as keywords. Trailing tokens never
//! change the class; [`computed_filter`] reports the filters lineage
//! cannot model.

use serde::{Deserialize, Serialize};
use sqlparser::tokenizer::Token;

use crate::dialect::Dialect;

/// A `SHOW` form, named by the keywords that start it.
///
/// Every listed form except `WARNINGS`, `ERRORS`, and `Unsupported` lists
/// schema objects, definitions, settings, or counters, never row values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Show {
    /// `SHOW DATABASES` or `SCHEMAS`: database names.
    Databases,
    /// `SHOW [EXTENDED] [FULL] TABLES`: table names and types.
    Tables,
    /// `SHOW OPEN TABLES`: open table names and lock counters.
    OpenTables,
    /// `SHOW TABLE STATUS`: table sizes, estimates, and comments.
    TableStatus,
    /// `SHOW [EXTENDED] [FULL] {COLUMNS | FIELDS}`: column definitions.
    Columns,
    /// `SHOW [EXTENDED] {INDEX | INDEXES | KEYS}`: index definitions.
    Index,
    /// `SHOW CREATE TABLE`: the table's definition.
    CreateTable,
    /// `SHOW CREATE VIEW`: the view's definition.
    CreateView,
    /// `SHOW CREATE TRIGGER`: the trigger's definition.
    CreateTrigger,
    /// `SHOW CREATE FUNCTION`: the function's definition.
    CreateFunction,
    /// `SHOW CREATE PROCEDURE`: the procedure's definition.
    CreateProcedure,
    /// `SHOW CREATE EVENT`: the event's definition.
    CreateEvent,
    /// `SHOW TRIGGERS`: trigger definitions.
    Triggers,
    /// `SHOW EVENTS`: event definitions.
    Events,
    /// `SHOW FUNCTION STATUS`: stored function metadata.
    FunctionStatus,
    /// `SHOW PROCEDURE STATUS`: stored procedure metadata.
    ProcedureStatus,
    /// `SHOW FUNCTION CODE`: compiled body, which repeats `SHOW CREATE FUNCTION`.
    FunctionCode,
    /// `SHOW PROCEDURE CODE`: compiled body, which repeats `SHOW CREATE PROCEDURE`.
    ProcedureCode,
    /// `SHOW [GLOBAL | SESSION] VARIABLES`: server settings.
    Variables,
    /// `SHOW [GLOBAL | SESSION] STATUS`: server counters.
    Status,
    /// `SHOW CHARSET` or `CHARACTER SET`: character sets.
    Charset,
    /// `SHOW COLLATION`: collations.
    Collation,
    /// `SHOW [STORAGE] ENGINES`: storage engines.
    Engines,
    /// `SHOW PLUGINS`: installed plugins.
    Plugins,
    /// `SHOW PRIVILEGES`: privilege names.
    Privileges,
    /// `SHOW [COUNT(*)] WARNINGS`: replays the previous statement's warnings, which quote values.
    Warnings,
    /// `SHOW [COUNT(*)] ERRORS`: replays the previous statement's errors, which quote values.
    Errors,
    /// Any other form or modifier shape.
    Unsupported,
}

impl Show {
    /// Classifies the first `SHOW` in `sql` by its leading keywords.
    ///
    /// `sql` is tokenized as `dialect` reads it. Tokenizer errors, a
    /// missing unquoted `SHOW`, and unknown forms are [`Show::Unsupported`].
    #[must_use]
    pub fn classify(sql: &str, dialect: Dialect) -> Self {
        let Ok(tokens) = dialect.tokenize(sql) else {
            return Self::Unsupported;
        };
        let words = keywords(&tokens);
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

/// Reports whether the `SHOW` in `sql` filters by more than a string literal.
///
/// A `WHERE` filter evaluates an expression, subqueries included, so the
/// rows returned can test any column's value; `LIKE` must be followed by a
/// string literal. Tokenizer errors count as filtering (fail closed).
pub(crate) fn computed_filter(sql: &str, dialect: Dialect) -> bool {
    let Ok(tokens) = dialect.tokenize(sql) else {
        return true;
    };
    let mut rest = after_show(&tokens).filter(|token| !matches!(token, Token::Whitespace(_)));
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

/// Returns the keywords after the first unquoted `SHOW`, ASCII-uppercased.
///
/// Whitespace and comments are skipped; `(`, `*`, and `)` are kept. The
/// sequence ends at the first other token: a quoted word, a literal, or
/// other punctuation. No unquoted `SHOW` yields an empty sequence.
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

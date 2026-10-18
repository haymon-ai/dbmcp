//! Dotenv (`.env`) files following the Docker Compose dialect.
//!
//! Each line is blank, a `#` comment, or `KEY=VALUE` / `KEY: VALUE`, with an
//! optional `export ` prefix. Unquoted values end at an inline ` #`
//! comment and are trimmed. Single-quoted values are literal. Double-quoted
//! values understand `\n`, `\r`, `\t`, `\\`, `\"`, and `\$`. Both quoted
//! forms may span lines. Unquoted and double-quoted values are interpolated
//! against the process environment, then against keys defined earlier.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::yaml;

/// One variable read from a dotenv file.
#[derive(Clone)]
pub struct DotenvVar {
    /// Expanded value.
    pub value: String,
    /// Absolute path of the file that defined it.
    pub path: PathBuf,
    /// 1-based line of its definition.
    pub line: usize,
}

impl DotenvVar {
    /// Returns the definition site as `path:line`.
    #[must_use]
    pub fn location(&self) -> String {
        format!("{}:{}", self.path.display(), self.line)
    }
}

impl std::fmt::Debug for DotenvVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DotenvVar")
            .field("value", &"[REDACTED]")
            .field("path", &self.path)
            .field("line", &self.line)
            .finish()
    }
}

/// Variables from every loaded dotenv file; later files override earlier ones.
#[derive(Clone, Debug, Default)]
pub struct Dotenv {
    vars: HashMap<String, DotenvVar>,
}

impl Dotenv {
    /// Returns the variable `name`, if any loaded file defines it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&DotenvVar> {
        self.vars.get(name)
    }

    /// Returns `true` when no loaded file defines any variable.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// Returns these variables overlaid with `process_env`, as interpolation properties.
    pub(crate) fn properties(&self, process_env: &HashMap<String, String>) -> HashMap<String, String> {
        let mut properties: HashMap<String, String> = self
            .vars
            .iter()
            .map(|(name, var)| (name.clone(), var.value.clone()))
            .collect();
        properties.extend(process_env.iter().map(|(name, value)| (name.clone(), value.clone())));
        properties
    }

    /// Parses `text`, read from `path`, on top of the variables loaded so far.
    ///
    /// Returns one `path:line: reason` message per malformed entry; the
    /// valid entries are still added.
    pub(crate) fn parse(&mut self, text: &str, path: &Path, process_env: &HashMap<String, String>) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut properties = Rc::new(self.properties(process_env));
        let mut errors = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = i + 1;
            match parse_entry(&lines, &mut i, &properties) {
                Ok(Some((key, value))) => {
                    if !process_env.contains_key(&key) {
                        Rc::make_mut(&mut properties).insert(key.clone(), value.clone());
                    }
                    self.vars.insert(
                        key,
                        DotenvVar {
                            value,
                            path: path.to_path_buf(),
                            line,
                        },
                    );
                }
                Ok(None) => {}
                Err(reason) => errors.push(format!("{}:{line}: {reason}", path.display())),
            }
            i += 1;
        }
        errors
    }
}

/// Parses the entry starting at `lines[*i]`, advancing `*i` past continuation lines.
///
/// Values interpolate against `properties`.
fn parse_entry(
    lines: &[&str],
    i: &mut usize,
    properties: &Rc<HashMap<String, String>>,
) -> Result<Option<(String, String)>, String> {
    let entry = lines[*i].trim_start();
    if entry.is_empty() || entry.starts_with('#') {
        return Ok(None);
    }
    let entry = entry.strip_prefix("export ").map_or(entry, str::trim_start);
    let sep = entry.find(['=', ':']).ok_or("expected `KEY=VALUE`")?;
    let key = entry[..sep].trim();
    if !is_valid_key(key) {
        return Err("invalid variable name".into());
    }
    let raw = entry[sep + 1..].trim_start_matches([' ', '\t']);

    let (template, interpolated) = match raw.chars().next() {
        Some('\'') => (quoted(raw, '\'', lines, i)?, false),
        Some('"') => (unescape(&quoted(raw, '"', lines, i)?), true),
        _ => (unquoted(raw).to_owned(), true),
    };
    // Without a `$` there is nothing to substitute.
    if !interpolated || !template.contains('$') {
        return Ok(Some((key.to_owned(), template)));
    }

    let value = yaml::interpolate(&template, Rc::clone(properties))?;
    Ok(Some((key.to_owned(), value)))
}

/// Returns `true` for `[A-Za-z_][A-Za-z0-9_.-]*`.
fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c == '.' || c == '-' || c.is_ascii_alphanumeric())
}

/// Returns an unquoted value without its inline comment and trailing blanks.
///
/// `raw` starts after the blanks that follow the separator, so a leading
/// `#` begins a comment too.
fn unquoted(raw: &str) -> &str {
    let end = raw
        .char_indices()
        .find(|&(i, c)| c == '#' && (i == 0 || raw[..i].ends_with([' ', '\t'])))
        .map_or(raw.len(), |(i, _)| i);
    raw[..end].trim_end()
}

/// Returns the raw text between `quote`s, joining continuation lines with `\n`.
///
/// Inside double quotes a backslash escapes the next character, so `\"`
/// does not close the value. Only blanks or a comment may follow the
/// closing quote.
fn quoted(raw: &str, quote: char, lines: &[&str], i: &mut usize) -> Result<String, String> {
    let mut text = raw[1..].to_owned();
    loop {
        if let Some(end) = closing_quote(&text, quote) {
            let tail = text[end + 1..].trim_start();
            if !tail.is_empty() && !tail.starts_with('#') {
                return Err("unexpected characters after the closing quote".into());
            }
            text.truncate(end);
            return Ok(text);
        }
        *i += 1;
        let Some(next) = lines.get(*i) else {
            return Err("unterminated quoted value".into());
        };
        text.push('\n');
        text.push_str(next);
    }
}

/// Returns the byte index of the first unescaped `quote` in `text`.
fn closing_quote(text: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote == '"' {
            escaped = true;
        } else if c == quote {
            return Some(i);
        }
    }
    None
}

/// Applies double-quote escapes; `\$` becomes `$$` so interpolation keeps it literal.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('$') => out.push_str("$$"),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process_env() -> HashMap<String, String> {
        [("HOME_DIR", "/home/deploy"), ("SHADOWED", "process")]
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    fn parse(text: &str) -> (Dotenv, Vec<String>) {
        let mut dotenv = Dotenv::default();
        let errors = dotenv.parse(text, Path::new("/cfg/.env"), &process_env());
        (dotenv, errors)
    }

    fn value<'a>(dotenv: &'a Dotenv, key: &str) -> &'a str {
        &dotenv.get(key).unwrap_or_else(|| panic!("{key} missing")).value
    }

    #[test]
    fn basic_forms_comments_and_export() {
        let (dotenv, errors) = parse(
            "# comment\n\nA=1\n  B = two words  \nexport C=3\nD: four\nE=five # trailing\nF=a#b\nG=\nH= # set me\n",
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "1");
        assert_eq!(value(&dotenv, "B"), "two words");
        assert_eq!(value(&dotenv, "C"), "3");
        assert_eq!(value(&dotenv, "D"), "four");
        assert_eq!(value(&dotenv, "E"), "five");
        assert_eq!(value(&dotenv, "F"), "a#b");
        assert_eq!(value(&dotenv, "G"), "");
        assert_eq!(
            value(&dotenv, "H"),
            "",
            "a comment right after `=` leaves the value empty"
        );
        assert_eq!(dotenv.get("A").expect("A").line, 3);
        assert_eq!(dotenv.get("A").expect("A").location(), "/cfg/.env:3");
    }

    #[test]
    fn single_quotes_are_literal() {
        let (dotenv, errors) = parse("A='$HOME_DIR \\n # not a comment'\n");
        assert!(errors.is_empty());
        assert_eq!(value(&dotenv, "A"), "$HOME_DIR \\n # not a comment");
    }

    #[test]
    fn double_quotes_unescape_and_interpolate() {
        let (dotenv, errors) = parse(r#"A="line1\nline2\t\"q\" \\ \$HOME_DIR $HOME_DIR" # c"#);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "line1\nline2\t\"q\" \\ $HOME_DIR /home/deploy");
    }

    #[test]
    fn quoted_values_span_lines() {
        let (dotenv, errors) = parse("A=\"first\nsecond\"\nB='x\ny'\nC=after\n");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "first\nsecond");
        assert_eq!(value(&dotenv, "B"), "x\ny");
        assert_eq!(dotenv.get("C").expect("C").line, 5);
    }

    #[test]
    fn interpolation_prefers_process_env_then_earlier_keys() {
        let (dotenv, errors) = parse("BASE=/srv\nSHADOWED=file\nA=${BASE}/data\nB=$SHADOWED\nC=${LATER:-d}\nLATER=x\n");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "/srv/data");
        assert_eq!(value(&dotenv, "B"), "process");
        assert_eq!(value(&dotenv, "C"), "d", "later keys are not visible yet");
    }

    #[test]
    fn compose_substitutions_expand() {
        let (dotenv, errors) = parse(concat!(
            "EMPTY=\n",
            "A=x${NOPE}y\n",
            "B=${NOPE:-d}|${EMPTY:-d}|${EMPTY-d}\n",
            "C=${HOME_DIR:+alt}|${EMPTY:+alt}|${EMPTY+alt}\n",
            "D=${NOPE:-${HOME_DIR}}\n",
            "E=pa$$word|lone $ dollar\n",
            "F=\"${HOME_DIR}\n\\t\\\"x\\\" \\\\\"\n",
            "G=true\n",
            "H=\"\u{fffe}\u{ffff}\u{85}\u{7f}$HOME_DIR\"\n",
        ));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "xy", "unset becomes empty");
        assert_eq!(value(&dotenv, "B"), "d|d|");
        assert_eq!(value(&dotenv, "C"), "alt||alt");
        assert_eq!(value(&dotenv, "D"), "/home/deploy");
        assert_eq!(value(&dotenv, "E"), "pa$word|lone $ dollar");
        assert_eq!(value(&dotenv, "F"), "/home/deploy\n\t\"x\" \\");
        assert_eq!(value(&dotenv, "G"), "true");
        assert_eq!(
            value(&dotenv, "H"),
            "\u{fffe}\u{ffff}\u{85}\u{7f}/home/deploy",
            "non-printables round-trip"
        );
    }

    #[test]
    fn substituted_values_are_not_expanded_again() {
        let mut env = process_env();
        env.insert("RAW".into(), "${HOME_DIR}\\\"".into());
        let mut dotenv = Dotenv::default();
        let errors = dotenv.parse("A=$RAW\n", Path::new("/cfg/.env"), &env);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "${HOME_DIR}\\\"");
    }

    #[test]
    fn later_definitions_override_earlier_ones() {
        let mut dotenv = Dotenv::default();
        dotenv.parse("A=first\nB=keep\n", Path::new("/a"), &process_env());
        dotenv.parse("A=second\nC=${B}\n", Path::new("/b"), &process_env());
        assert_eq!(value(&dotenv, "A"), "second");
        assert_eq!(dotenv.get("A").expect("A").path, Path::new("/b"));
        assert_eq!(value(&dotenv, "C"), "keep", "earlier files feed interpolation");
    }

    #[test]
    fn malformed_entries_report_location_without_content() {
        let (dotenv, errors) = parse(
            "secret-without-separator\n1BAD=hunter2\nA='hunter2' trailing\nB=${A:?}\nD=\"hunter2${X!y\"\nC=\"hunter2\nOK=1\n",
        );
        assert_eq!(
            errors,
            [
                "/cfg/.env:1: expected `KEY=VALUE`",
                "/cfg/.env:2: invalid variable name",
                "/cfg/.env:3: unexpected characters after the closing quote",
                "/cfg/.env:4: missing property `A`",
                "/cfg/.env:5: invalid substitution; expected `${NAME}` optionally followed by :-, -, :?, ?, :+, or +, and a closing `}`",
                "/cfg/.env:6: unterminated quoted value",
            ]
        );
        assert!(errors.iter().all(|e| !e.contains("hunter2")));
        assert!(dotenv.get("OK").is_none(), "unterminated quote swallows the rest");
    }

    #[test]
    fn debug_redacts_values() {
        let (dotenv, _) = parse("PASSWORD=hunter2\n");
        assert!(!format!("{dotenv:?}").contains("hunter2"));
    }
}

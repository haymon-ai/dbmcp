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

use crate::interpolate::{self, Lookup};
use crate::warning::ConfigWarning;

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

    /// Parses `text`, read from `path`, on top of the variables loaded so far.
    ///
    /// Returns one `path:line: reason` message per malformed entry; the
    /// valid entries are still added.
    pub(crate) fn parse(
        &mut self,
        text: &str,
        path: &Path,
        process_env: Lookup<'_>,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut errors = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = i + 1;
            let location = format!("{}:{line}", path.display());
            match self.parse_entry(&lines, &mut i, process_env, warnings, &location) {
                Ok(Some((key, value))) => {
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
                Err(reason) => errors.push(format!("{location}: {reason}")),
            }
            i += 1;
        }
        errors
    }

    /// Parses the entry starting at `lines[*i]`, advancing `*i` past continuation lines.
    fn parse_entry(
        &self,
        lines: &[&str],
        i: &mut usize,
        process_env: Lookup<'_>,
        warnings: &mut Vec<ConfigWarning>,
        location: &str,
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
        if !interpolated {
            return Ok(Some((key.to_owned(), template)));
        }

        let lookup = |name: &str| process_env(name).or_else(|| self.vars.get(name).map(|var| var.value.clone()));
        let mut unset = Vec::new();
        let value = interpolate::interpolate(&template, &lookup, &mut unset).map_err(|e| e.to_string())?;
        warnings.extend(unset.into_iter().map(|name| ConfigWarning::UnsetVariable {
            name,
            location: location.to_owned(),
        }));
        Ok(Some((key.to_owned(), value)))
    }
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

    fn process_env(name: &str) -> Option<String> {
        match name {
            "HOME_DIR" => Some("/home/deploy".into()),
            "SHADOWED" => Some("process".into()),
            _ => None,
        }
    }

    fn parse(text: &str) -> (Dotenv, Vec<String>, Vec<ConfigWarning>) {
        let mut dotenv = Dotenv::default();
        let mut warnings = Vec::new();
        let errors = dotenv.parse(text, Path::new("/cfg/.env"), &process_env, &mut warnings);
        (dotenv, errors, warnings)
    }

    fn value<'a>(dotenv: &'a Dotenv, key: &str) -> &'a str {
        &dotenv.get(key).unwrap_or_else(|| panic!("{key} missing")).value
    }

    #[test]
    fn basic_forms_comments_and_export() {
        let (dotenv, errors, _) = parse(
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
        let (dotenv, errors, warnings) = parse("A='$HOME_DIR \\n # not a comment'\n");
        assert!(errors.is_empty() && warnings.is_empty());
        assert_eq!(value(&dotenv, "A"), "$HOME_DIR \\n # not a comment");
    }

    #[test]
    fn double_quotes_unescape_and_interpolate() {
        let (dotenv, errors, _) = parse(r#"A="line1\nline2\t\"q\" \\ \$HOME_DIR $HOME_DIR" # c"#);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "line1\nline2\t\"q\" \\ $HOME_DIR /home/deploy");
    }

    #[test]
    fn quoted_values_span_lines() {
        let (dotenv, errors, _) = parse("A=\"first\nsecond\"\nB='x\ny'\nC=after\n");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "first\nsecond");
        assert_eq!(value(&dotenv, "B"), "x\ny");
        assert_eq!(dotenv.get("C").expect("C").line, 5);
    }

    #[test]
    fn interpolation_prefers_process_env_then_earlier_keys() {
        let (dotenv, errors, _) =
            parse("BASE=/srv\nSHADOWED=file\nA=${BASE}/data\nB=$SHADOWED\nC=${LATER:-d}\nLATER=x\n");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(value(&dotenv, "A"), "/srv/data");
        assert_eq!(value(&dotenv, "B"), "process");
        assert_eq!(value(&dotenv, "C"), "d", "later keys are not visible yet");
    }

    #[test]
    fn unset_variables_warn_with_location() {
        let (dotenv, _, warnings) = parse("\nA=x${NOPE}y\n");
        assert_eq!(value(&dotenv, "A"), "xy");
        assert_eq!(
            warnings,
            [ConfigWarning::UnsetVariable {
                name: "NOPE".into(),
                location: "/cfg/.env:2".into()
            }]
        );
    }

    #[test]
    fn later_definitions_override_earlier_ones() {
        let mut dotenv = Dotenv::default();
        let mut warnings = Vec::new();
        dotenv.parse("A=first\nB=keep\n", Path::new("/a"), &process_env, &mut warnings);
        dotenv.parse("A=second\nC=${B}\n", Path::new("/b"), &process_env, &mut warnings);
        assert_eq!(value(&dotenv, "A"), "second");
        assert_eq!(dotenv.get("A").expect("A").path, Path::new("/b"));
        assert_eq!(value(&dotenv, "C"), "keep", "earlier files feed interpolation");
    }

    #[test]
    fn malformed_entries_report_location_without_content() {
        let (dotenv, errors, _) =
            parse("secret-without-separator\n1BAD=hunter2\nA='hunter2' trailing\nB=$\nC=\"hunter2\nOK=1\n");
        assert_eq!(
            errors,
            [
                "/cfg/.env:1: expected `KEY=VALUE`",
                "/cfg/.env:2: invalid variable name",
                "/cfg/.env:3: unexpected characters after the closing quote",
                "/cfg/.env:4: invalid `$`; escape a literal `$` as `$$`",
                "/cfg/.env:5: unterminated quoted value",
            ]
        );
        assert!(errors.iter().all(|e| !e.contains("hunter2")));
        assert!(dotenv.get("OK").is_none(), "unterminated quote swallows the rest");
    }

    #[test]
    fn debug_redacts_values() {
        let (dotenv, _, _) = parse("PASSWORD=hunter2\n");
        assert!(!format!("{dotenv:?}").contains("hunter2"));
    }
}

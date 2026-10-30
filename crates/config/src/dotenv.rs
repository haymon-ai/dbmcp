//! Env file parser.
//!
//! Supports `#` comment lines, `KEY=VAL` or
//! `KEY: VAL`, an optional `export ` prefix, a bare `KEY` that inherits
//! the process value, unquoted values cut at ` #` and trimmed, single quotes
//! literal, double quotes with shell escapes, multi-line quoted values, and
//! `${}` expansion of unquoted and double-quoted values against the process
//! env first, then earlier keys of the same file. Errors are collected per
//! line and never echo file contents.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::template;

/// A parsed env file: each key's value and 1-based line, in file order.
#[derive(Debug, Clone)]
pub(crate) struct EnvFile {
    path: PathBuf,
    entries: IndexMap<String, (String, usize)>,
}

impl EnvFile {
    /// Returns the absolute path the file was read from.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the value of `name`.
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.entries.get(name).map(|(value, _)| value.as_str())
    }

    /// Returns the value of `name` and the line it was set on.
    pub(crate) fn get_with_line(&self, name: &str) -> Option<(&str, usize)> {
        self.entries.get(name).map(|(value, line)| (value.as_str(), *line))
    }
}

/// A parsed env file plus the `${}` warnings it produced.
#[derive(Debug)]
pub(crate) struct Parsed {
    pub(crate) file: EnvFile,
    pub(crate) warnings: Vec<String>,
}

/// Parses env file `text` read from `path`; `lookup` reads the process env.
///
/// # Errors
///
/// Returns one `line N: reason` message per bad statement. An unterminated
/// quote stops parsing, since the rest of the file cannot be split reliably.
pub(crate) fn parse(path: &Path, text: &str, lookup: template::Lookup<'_>) -> Result<Parsed, Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut entries: IndexMap<String, (String, usize)> = IndexMap::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut pos = 0;
    while let Some(start) = statement_start(text, pos) {
        let line = line_of(text, start);
        let statement = strip_export(&text[start..]);
        let consumed = text.len() - statement.len();
        let key_scan = match scan_key(statement) {
            Ok(scan) => scan,
            Err(reason) => {
                errors.push(format!("line {line}: {reason}"));
                pos = next_line(text, start);
                continue;
            }
        };
        let key = statement[..key_scan.key_end].trim_end();
        if key.contains(' ') {
            errors.push(format!("line {line}: key cannot contain a space"));
            pos = next_line(text, start);
            continue;
        }
        let value_start = consumed + key_scan.value_start;
        if key_scan.inherited {
            if let Some(value) = lookup(key) {
                entries.insert(key.to_owned(), (value, line));
            }
            pos = value_start;
            continue;
        }
        let value_start = value_start + leading_space(&text[value_start..]);
        let lookup_with_file = |name: &str| lookup(name).or_else(|| entries.get(name).map(|(value, _)| value.clone()));
        let (raw, expand, end) = match extract_value(&text[value_start..]) {
            Ok((raw, expand, len)) => (raw, expand, value_start + len),
            Err(reason) => {
                errors.push(format!("line {line}: {reason}"));
                break;
            }
        };
        pos = end;
        let value = if expand {
            let mut unset = Vec::new();
            match template::substitute(&raw, &lookup_with_file, &mut unset) {
                Ok(value) => {
                    warnings.extend(unset.iter().map(|name| unset_warning(path, line, key, name)));
                    value
                }
                Err(error) => {
                    errors.push(format!("line {line}: {key}: {error}"));
                    continue;
                }
            }
        } else {
            raw
        };
        entries.insert(key.to_owned(), (value, line));
    }
    if errors.is_empty() {
        Ok(Parsed {
            file: EnvFile {
                path: path.to_path_buf(),
                entries,
            },
            warnings,
        })
    } else {
        Err(errors)
    }
}

fn unset_warning(path: &Path, line: usize, key: &str, name: &str) -> String {
    format!(
        "{}:{line}: {key}: the \"{name}\" variable is not set; defaulting to a blank string",
        path.display()
    )
}

/// Skips whitespace and comment lines; returns the next statement's offset.
fn statement_start(text: &str, mut pos: usize) -> Option<usize> {
    loop {
        let offset = text[pos..].find(|c: char| !c.is_whitespace())?;
        pos += offset;
        if !text[pos..].starts_with('#') {
            return Some(pos);
        }
        pos += text[pos..].find('\n')?;
    }
}

fn next_line(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |n| pos + n + 1)
}

fn line_of(text: &str, pos: usize) -> usize {
    text[..pos].matches('\n').count() + 1
}

/// Strips a leading `export` followed by whitespace.
fn strip_export(statement: &str) -> &str {
    match statement.strip_prefix("export") {
        Some(rest) if rest.starts_with(char::is_whitespace) => rest.trim_start_matches(is_space),
        _ => statement,
    }
}

struct KeyScan {
    key_end: usize,
    value_start: usize,
    inherited: bool,
}

/// Finds the end of the key; `:` and `=` start a value, a newline or EOF inherits.
fn scan_key(statement: &str) -> Result<KeyScan, &'static str> {
    for (i, c) in statement.char_indices() {
        match c {
            '=' | ':' | '\n' => {
                return Ok(KeyScan {
                    key_end: i,
                    value_start: i + 1,
                    inherited: c == '\n',
                });
            }
            '_' | '.' | '-' | '[' | ']' => {}
            c if is_space(c) || c.is_alphanumeric() => {}
            _ => return Err("unexpected character in variable name"),
        }
    }
    Ok(KeyScan {
        key_end: statement.len(),
        value_start: statement.len(),
        inherited: true,
    })
}

fn leading_space(text: &str) -> usize {
    text.len() - text.trim_start_matches(is_space).len()
}

/// Reads one value; returns the raw text, whether it gets `${}`
/// expansion, and the number of bytes consumed.
fn extract_value(src: &str) -> Result<(String, bool, usize), &'static str> {
    let Some(&quote @ (b'"' | b'\'')) = src.as_bytes().first() else {
        let (line, consumed) = match src.find('\n') {
            Some(n) => (&src[..n], n + 1),
            None => (src, src.len()),
        };
        let value = line.split_once(" #").map_or(line, |(value, _)| value);
        return Ok((value.trim_end().to_owned(), true, consumed));
    };
    let bytes = src.as_bytes();
    let mut chars = Vec::new();
    let mut escaped = false;
    for (i, &c) in bytes.iter().enumerate().skip(1) {
        if c != quote {
            if !escaped && c == b'\\' {
                escaped = true;
                continue;
            }
            if escaped {
                escaped = false;
                chars.push(b'\\');
            }
            chars.push(c);
            continue;
        }
        if escaped {
            escaped = false;
            chars.push(c);
            continue;
        }
        let value = String::from_utf8(chars).expect("splits only at ASCII bytes");
        return Ok(if quote == b'"' {
            (expand_escapes(&value), true, i + 1)
        } else {
            (value, false, i + 1)
        });
    }
    Err("unterminated quoted value")
}

/// Expands `\a \b \f \n \r \t \v \" \\`, `\$` (to `$$`), and `\0NNN` octal.
fn expand_escapes(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let (replacement, len) = match tail.as_bytes().get(1) {
            Some(b'a') => ("\u{7}".to_owned(), 2),
            Some(b'b') => ("\u{8}".to_owned(), 2),
            Some(b'f') => ("\u{c}".to_owned(), 2),
            Some(b'n') => ("\n".to_owned(), 2),
            Some(b'r') => ("\r".to_owned(), 2),
            Some(b't') => ("\t".to_owned(), 2),
            Some(b'v') => ("\u{b}".to_owned(), 2),
            Some(b'"') => ("\"".to_owned(), 2),
            Some(b'\\') => ("\\".to_owned(), 2),
            Some(b'$') => ("$$".to_owned(), 2),
            Some(b'0') => octal(tail),
            _ => ("\\".to_owned(), 1),
        };
        out.push_str(&replacement);
        rest = &tail[len..];
    }
    out.push_str(rest);
    out
}

/// Expands `\0` plus exactly three octal digits; other `\0…` forms stay literal.
fn octal(tail: &str) -> (String, usize) {
    let digits = tail[2..].bytes().take(3).take_while(u8::is_ascii_digit).count();
    let matched = &tail[..2 + digits];
    let code = (digits == 3)
        .then(|| u32::from_str_radix(&tail[2..5], 8).ok())
        .flatten()
        .filter(|code| *code <= 0xff)
        .and_then(char::from_u32);
    match code {
        Some(c) => (c.to_string(), matched.len()),
        None => (matched.to_owned(), matched.len()),
    }
}

/// Blank characters other than line breaks.
fn is_space(c: char) -> bool {
    matches!(c, '\t' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn parse_str(text: &str) -> Result<Parsed, Vec<String>> {
        parse(Path::new("/test.env"), text, &no_env)
    }

    #[track_caller]
    fn single(text: &str, key: &str, expected: &str) {
        let parsed = parse_str(text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        assert_eq!(parsed.file.get(key), Some(expected), "{text}");
    }

    #[test]
    fn parsing_vectors() {
        for (text, key, expected) in [
            ("FOO=bar", "FOO", "bar"),
            ("FOO =bar", "FOO", "bar"),
            ("FOO= bar", "FOO", "bar"),
            (r#"FOO="bar""#, "FOO", "bar"),
            ("FOO='bar'", "FOO", "bar"),
            (r#"FOO="escaped\"bar""#, "FOO", r#"escaped"bar"#),
            (r#"FOO="\"quoted\"""#, "FOO", r#""quoted""#),
            (r#"FOO="'d'""#, "FOO", "'d'"),
            ("OPTION_A: 1", "OPTION_A", "1"),
            ("OPTION_A: Foo=bar", "OPTION_A", "Foo=bar"),
            ("OPTION_A=1:B", "OPTION_A", "1:B"),
            ("export OPTION_A=2", "OPTION_A", "2"),
            (r"export OPTION_B='\n'", "OPTION_B", r"\n"),
            ("export exportFoo=2", "exportFoo", "2"),
            ("exportFOO=2", "exportFOO", "2"),
            ("export_FOO =2", "export_FOO", "2"),
            ("export.FOO= 2", "export.FOO", "2"),
            ("export\tOPTION_A=2", "OPTION_A", "2"),
            ("  export OPTION_A=2", "OPTION_A", "2"),
            ("\texport OPTION_A=2", "OPTION_A", "2"),
            (r#"export OPTION_A="export A""#, "OPTION_A", "export A"),
            (r#"FOO="bar\nbaz""#, "FOO", "bar\nbaz"),
            (r"FOO=a\tb", "FOO", r"a\tb"),
            (r#"FOO="a\tb""#, "FOO", "a\tb"),
            (
                r#"KEY="Z\aZ\bZ\fZ\nZ\rZ\tZ\vZ\\Z\0123Z""#,
                "KEY",
                "Z\u{7}Z\u{8}Z\u{c}Z\nZ\rZ\tZ\u{b}Z\\ZSZ",
            ),
            ("FOO.BAR=foobar", "FOO.BAR", "foobar"),
            ("FOO=foobar=", "FOO", "foobar="),
            ("FOO=bar ", "FOO", "bar"),
            ("FOO=bar # this is foo", "FOO", "bar"),
            ("FOO=bar #this is foo", "FOO", "bar"),
            ("FOO=bar #", "FOO", "bar"),
            ("FOO=123#not-an-inline-comment", "FOO", "123#not-an-inline-comment"),
            (r#"FOO="bar#baz""#, "FOO", "bar#baz"),
            (r##"FOO="bar#baz"#"##, "FOO", "bar#baz"),
            (r#"FOO="bar#baz" # comment"#, "FOO", "bar#baz"),
            ("FOO='bar#baz' # comment", "FOO", "bar#baz"),
            (r#"FOO="bar#baz#bang" # comment"#, "FOO", "bar#baz#bang"),
            (r#"FOO="ba#r""#, "FOO", "ba#r"),
            ("FOO='ba#r'", "FOO", "ba#r"),
            (r#"FOO="bar\n\ b\az""#, "FOO", "bar\n\\ b\u{7}z"),
            (r#"FOO="bar\\\n\ b\az""#, "FOO", "bar\\\n\\ b\u{7}z"),
            (r#"FOO="bar\\r\ b\az""#, "FOO", "bar\\r\\ b\u{7}z"),
            (r#"FOO="bar\nbaz\\""#, "FOO", "bar\nbaz\\"),
            (r#"="value""#, "", "value"),
            (" KEY =value", "KEY", "value"),
            ("   KEY=value", "KEY", "value"),
            ("\tKEY=value", "KEY", "value"),
            (r#"KEY="\0123""#, "KEY", "S"),
            (r#"KEY="\x07""#, "KEY", r"\x07"),
            (r#"KEY="\u12e4""#, "KEY", r"\u12e4"),
            (r#"KEY="\U00101234""#, "KEY", r"\U00101234"),
        ] {
            single(text, key, expected);
        }
    }

    #[test]
    fn bad_key_character_is_an_error_without_echo() {
        let errors = parse_str("lol$wut").expect_err("invalid key");
        assert_eq!(errors, ["line 1: unexpected character in variable name"]);
    }

    #[test]
    fn unterminated_quotes() {
        for text in [
            r#"KEY=""#,
            r#"KEY="value"#,
            r#"KEY="value\""#,
            r#"KEY="value'"#,
            "KEY='",
            "KEY='value",
            r"KEY='value\'",
            r#"KEY='value""#,
        ] {
            let errors = parse_str(text).expect_err(text);
            assert_eq!(errors, ["line 1: unterminated quoted value"], "{text}");
        }
    }

    #[test]
    fn blank_and_comment_lines_are_skipped() {
        for text in ["\n", "\r\n", "\t\t ", "# Comment", "\t # comment"] {
            let parsed = parse_str(text).expect("valid");
            assert!(parsed.file.entries.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn keys_record_their_line() {
        let parsed = parse_str("# c\nA=1\n\nB=\"x\ny\"\nC=3\n").expect("valid");
        assert_eq!(parsed.file.get_with_line("A"), Some(("1", 2)));
        assert_eq!(parsed.file.get_with_line("B"), Some(("x\ny", 4)));
        assert_eq!(parsed.file.get_with_line("C"), Some(("3", 6)));
    }

    #[test]
    fn later_keys_override_earlier_ones() {
        single("A=1\nA=2", "A", "2");
    }

    #[test]
    fn expansion_prefers_process_env_then_earlier_keys() {
        let process = |name: &str| (name == "HOST").then(|| "process".to_owned());
        let text = "HOST=file\nPORT=5432\nURL=${HOST}:${PORT}\nRAW='${HOST}'\nESC=\"\\$HOST\"\n";
        let parsed = parse(Path::new("/t.env"), text, &process).expect("valid");
        assert_eq!(parsed.file.get("URL"), Some("process:5432"));
        assert_eq!(parsed.file.get("RAW"), Some("${HOST}"));
        assert_eq!(parsed.file.get("ESC"), Some("$HOST"));
    }

    #[test]
    fn inherited_keys_take_the_process_value() {
        let process = |name: &str| (name == "SET").then(|| "v".to_owned());
        let parsed = parse(Path::new("/t.env"), "SET\nUNSET\nLAST", &process).expect("valid");
        assert_eq!(parsed.file.get("SET"), Some("v"));
        assert_eq!(parsed.file.get("UNSET"), None);
    }

    #[test]
    fn bom_is_stripped() {
        single("\u{feff}KEY=v", "KEY", "v");
    }

    #[test]
    fn errors_are_collected_and_name_the_line() {
        let errors = parse_str("A B=1\nok=1\nC=${REQ:?set it}\nx$=2\n").expect_err("invalid");
        assert_eq!(
            errors,
            [
                "line 1: key cannot contain a space",
                "line 3: C: required variable REQ is missing a value: set it",
                "line 4: unexpected character in variable name",
            ]
        );
    }

    #[test]
    fn unset_references_warn() {
        let parsed = parse_str("A=${MISSING}x\n").expect("valid");
        assert_eq!(parsed.file.get("A"), Some("x"));
        assert_eq!(
            parsed.warnings,
            ["/test.env:1: A: the \"MISSING\" variable is not set; defaulting to a blank string"]
        );
    }
}

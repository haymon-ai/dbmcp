//! Compose-style `$VAR` / `${VAR}` interpolation.
//!
//! Supports `$VAR`, `${VAR}`, `${VAR:-default}` / `${VAR-default}`,
//! `${VAR:?message}` / `${VAR?message}`, `${VAR:+alt}` / `${VAR+alt}`,
//! nesting inside the default, message, and alternative, and `$$` as an
//! escaped `$`. The colon forms treat an empty value like an unset one.
//! Nested text is only expanded when it is used.

/// Looks up a variable's value; `None` when it is unset.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Reasons interpolation fails; none of them echo the template or a value.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InterpolateError {
    /// A `?` form named a variable that is unset (or empty, for `:?`).
    #[error("{}", required_message(.name, .message))]
    Required {
        /// Variable name.
        name: String,
        /// User-written message after the `?`, expanded.
        message: String,
    },

    /// A `$` is followed by neither a name, `{`, nor `$`.
    #[error("invalid `$`; escape a literal `$` as `$$`")]
    LoneDollar,

    /// A `${` has no matching `}`.
    #[error("unterminated `${{`")]
    Unterminated,

    /// The text inside `${...}` is not a valid substitution.
    #[error("invalid substitution `${{...}}`; expected `${{NAME}}` optionally followed by :-, -, :?, ?, :+, or +")]
    InvalidSubstitution,
}

fn required_message(name: &str, message: &str) -> String {
    if message.is_empty() {
        format!("required variable {name} is not set")
    } else {
        format!("required variable {name} is not set: {message}")
    }
}

/// Expands every substitution in `template`.
///
/// Each variable that is referenced, unset, and not covered by a default
/// becomes `""` and is appended to `unset`.
pub(crate) fn interpolate(
    template: &str,
    lookup: Lookup<'_>,
    unset: &mut Vec<String>,
) -> Result<String, InterpolateError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.chars().next() {
            Some('$') => {
                out.push('$');
                rest = &after[1..];
            }
            Some('{') => {
                let (body, tail) = split_braced(&after[1..])?;
                out.push_str(&substitute(body, lookup, unset)?);
                rest = tail;
            }
            Some(c) if is_name_start(c) => {
                let end = after.find(|c: char| !is_name_char(c)).unwrap_or(after.len());
                out.push_str(&value(&after[..end], lookup, unset));
                rest = &after[end..];
            }
            _ => return Err(InterpolateError::LoneDollar),
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn is_name_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic()
}

fn is_name_char(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric()
}

/// Splits `s` (just past a `${`) into the braced body and the text after its `}`.
fn split_braced(s: &str) -> Result<(&str, &str), InterpolateError> {
    let bytes = s.as_bytes();
    let mut depth = 1;
    let mut i = 0;
    while i < bytes.len() {
        match (bytes[i], bytes.get(i + 1)) {
            (b'$', Some(b'$')) => i += 2,
            (b'$', Some(b'{')) => {
                depth += 1;
                i += 2;
            }
            (b'}', _) => {
                depth -= 1;
                if depth == 0 {
                    return Ok((&s[..i], &s[i + 1..]));
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    Err(InterpolateError::Unterminated)
}

/// Evaluates the body of one `${...}` substitution.
fn substitute(body: &str, lookup: Lookup<'_>, unset: &mut Vec<String>) -> Result<String, InterpolateError> {
    let end = body.find(|c: char| !is_name_char(c)).unwrap_or(body.len());
    let name = &body[..end];
    if !name.starts_with(is_name_start) {
        return Err(InterpolateError::InvalidSubstitution);
    }
    let rest = &body[end..];
    if rest.is_empty() {
        return Ok(value(name, lookup, unset));
    }
    let (colon, rest) = rest.strip_prefix(':').map_or((false, rest), |rest| (true, rest));
    let mut chars = rest.chars();
    let op = chars.next().ok_or(InterpolateError::InvalidSubstitution)?;
    let arg = chars.as_str();

    let var = lookup(name);
    let present = if colon {
        var.as_deref().is_some_and(|v| !v.is_empty())
    } else {
        var.is_some()
    };
    match (op, present) {
        ('-' | '?', true) => Ok(var.unwrap_or_default()),
        ('-', false) | ('+', true) => interpolate(arg, lookup, unset),
        ('?', false) => Err(InterpolateError::Required {
            name: name.to_owned(),
            message: interpolate(arg, lookup, unset)?,
        }),
        ('+', false) => Ok(String::new()),
        _ => Err(InterpolateError::InvalidSubstitution),
    }
}

/// Returns the value of `name`, recording it in `unset` when missing.
fn value(name: &str, lookup: Lookup<'_>, unset: &mut Vec<String>) -> String {
    lookup(name).unwrap_or_else(|| {
        unset.push(name.to_owned());
        String::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(name: &str) -> Option<String> {
        match name {
            "SET" => Some("value".into()),
            "EMPTY" => Some(String::new()),
            "OTHER" => Some("other".into()),
            _ => None,
        }
    }

    fn run(template: &str) -> Result<(String, Vec<String>), InterpolateError> {
        let mut unset = Vec::new();
        interpolate(template, &vars, &mut unset).map(|out| (out, unset))
    }

    fn ok(template: &str) -> String {
        run(template).unwrap_or_else(|e| panic!("{template:?}: {e}")).0
    }

    #[test]
    fn plain_text_and_escapes() {
        assert_eq!(ok("no vars"), "no vars");
        assert_eq!(ok("cost $$5"), "cost $5");
        assert_eq!(ok("$$SET"), "$SET");
        assert_eq!(ok("$${SET}"), "${SET}");
    }

    #[test]
    fn bare_and_braced_names() {
        assert_eq!(ok("$SET/x"), "value/x");
        assert_eq!(ok("${SET}x"), "valuex");
        assert_eq!(ok("a${SET}b$OTHER"), "avaluebother");
    }

    #[test]
    fn unset_variable_becomes_empty_and_is_reported() {
        assert_eq!(
            run("[$MISSING][${ALSO}]").expect("ok"),
            ("[][]".into(), vec!["MISSING".into(), "ALSO".into()])
        );
        assert_eq!(run("${EMPTY}").expect("ok"), (String::new(), vec![]));
    }

    #[test]
    fn default_forms() {
        assert_eq!(ok("${MISSING:-d}"), "d");
        assert_eq!(ok("${EMPTY:-d}"), "d");
        assert_eq!(ok("${SET:-d}"), "value");
        assert_eq!(ok("${MISSING-d}"), "d");
        assert_eq!(ok("${EMPTY-d}"), "");
        assert_eq!(ok("${MISSING:-}"), "");
    }

    #[test]
    fn alternative_forms() {
        assert_eq!(ok("${SET:+alt}"), "alt");
        assert_eq!(ok("${EMPTY:+alt}"), "");
        assert_eq!(ok("${EMPTY+alt}"), "alt");
        assert_eq!(ok("${MISSING+alt}"), "");
    }

    #[test]
    fn required_forms() {
        assert_eq!(ok("${SET:?}"), "value");
        assert_eq!(ok("${EMPTY?}"), "");
        assert_eq!(
            run("${MISSING:?}"),
            Err(InterpolateError::Required {
                name: "MISSING".into(),
                message: String::new()
            })
        );
        let error = run("${EMPTY:?set it in .env}").expect_err("empty");
        assert_eq!(error.to_string(), "required variable EMPTY is not set: set it in .env");
        assert!(run("${MISSING?}").is_err());
    }

    #[test]
    fn nesting_is_lazy() {
        assert_eq!(ok("${MISSING:-${SET}}"), "value");
        assert_eq!(ok("${MISSING:-${ALSO:-${OTHER}}}"), "other");
        assert_eq!(ok("${SET:-${NOPE:?unused}}"), "value", "unused default is not expanded");
        assert_eq!(ok("${MISSING:-a$$b}"), "a$b");
        assert_eq!(ok("${SET:+[${OTHER}]}"), "[other]");
    }

    #[test]
    fn malformed_templates_are_rejected_without_echo() {
        for (template, error) in [
            ("price $", InterpolateError::LoneDollar),
            ("$1", InterpolateError::LoneDollar),
            ("$ x", InterpolateError::LoneDollar),
            ("${SET", InterpolateError::Unterminated),
            ("${SET:-${OTHER}", InterpolateError::Unterminated),
            ("${}", InterpolateError::InvalidSubstitution),
            ("${1A}", InterpolateError::InvalidSubstitution),
            ("${SET*x}", InterpolateError::InvalidSubstitution),
            ("${SET:}", InterpolateError::InvalidSubstitution),
        ] {
            assert_eq!(run(template), Err(error.clone()), "{template:?}");
            assert!(!error.to_string().contains("value"), "{error}");
        }
    }
}

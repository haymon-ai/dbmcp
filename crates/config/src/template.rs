//! `${}` variable substitution.
//!
//! Supports `$VAR`, `${VAR}`,
//! `${VAR:-default}` / `${VAR-default}`, `${VAR:+alt}` / `${VAR+alt}`,
//! `${VAR:?err}` / `${VAR?err}`, nesting inside the default, alternate, and
//! error text, and `$$` as a literal `$`. A `$` followed by anything else
//! stays literal; a malformed `${` is an error. Unset plain references
//! become `""` and are reported back so the caller can warn.

/// A substitution that cannot be completed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TemplateError {
    /// A `${` that does not form a valid reference.
    #[error("invalid interpolation format; escape a literal $ as $$")]
    Invalid,

    /// A `?` / `:?` reference whose variable is unset (or empty for `:?`).
    #[error("required variable {name} is missing a value{}", reason_suffix(.reason))]
    MissingRequired {
        /// Variable name.
        name: String,
        /// User-written error text, already substituted.
        reason: String,
    },
}

fn reason_suffix(reason: &str) -> String {
    if reason.is_empty() {
        String::new()
    } else {
        format!(": {reason}")
    }
}

/// Looks up a variable; `None` means unset.
pub(crate) type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Substitutes every reference in `template`.
///
/// Names of unset variables that were read without an operator are
/// appended to `unset`.
///
/// # Errors
///
/// Returns the first [`TemplateError`] in `template`.
pub(crate) fn substitute(template: &str, lookup: Lookup<'_>, unset: &mut Vec<String>) -> Result<String, TemplateError> {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut literal_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            i += 1;
            continue;
        }
        out.push_str(&template[literal_start..i]);
        match bytes.get(i + 1) {
            Some(b'$') => {
                out.push('$');
                i += 2;
            }
            Some(&c) if is_name_start(c) => {
                let end = name_end(bytes, i + 1);
                out.push_str(&plain(&template[i + 1..end], lookup, unset));
                i = end;
            }
            Some(b'{') => {
                let (value, end) = braced(template, i, lookup, unset)?;
                out.push_str(&value);
                i = end;
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
        literal_start = i;
    }
    out.push_str(&template[literal_start..]);
    Ok(out)
}

/// Resolves the `${…}` starting at `start`; returns its value and end offset.
fn braced(
    template: &str,
    start: usize,
    lookup: Lookup<'_>,
    unset: &mut Vec<String>,
) -> Result<(String, usize), TemplateError> {
    let bytes = template.as_bytes();
    let name_start = start + 2;
    if !bytes.get(name_start).copied().is_some_and(is_name_start) {
        return Err(TemplateError::Invalid);
    }
    let name_end = name_end(bytes, name_start);
    let name = &template[name_start..name_end];
    let op_start = if bytes.get(name_end) == Some(&b':') {
        name_end + 1
    } else {
        name_end
    };
    match bytes.get(op_start) {
        Some(b'}') if op_start == name_end => Ok((plain(name, lookup, unset), name_end + 1)),
        Some(b'-' | b'+' | b'?') => {
            // The operator text is greedy and stops at a newline: the match
            // runs to the last `}` on the line; text after the first balanced
            // `}` is substituted again and appended.
            let line_end = template[op_start..].find('\n').map_or(template.len(), |n| op_start + n);
            let last = template[op_start + 1..line_end]
                .rfind('}')
                .map(|n| op_start + 1 + n)
                .ok_or(TemplateError::Invalid)?;
            let matched = &template[start..=last];
            let (body, rest) = match first_brace_closing(matched) {
                Some(close) => (&matched[2..close], &matched[close + 1..]),
                None => (&matched[2..matched.len() - 1], ""),
            };
            let operator = &body[name.len()..];
            let (op, arg) = if operator.starts_with(':') {
                operator.split_at(2)
            } else {
                operator.split_at(1)
            };
            let value = apply(name, op, arg, lookup, unset)?;
            let rest = substitute(rest, lookup, unset)?;
            Ok((value + &rest, last + 1))
        }
        _ => Err(TemplateError::Invalid),
    }
}

/// Applies one operator; `arg` is substituted only when it is used.
fn apply(
    name: &str,
    op: &str,
    arg: &str,
    lookup: Lookup<'_>,
    unset: &mut Vec<String>,
) -> Result<String, TemplateError> {
    let value = lookup(name);
    let empty_or_unset = value.as_deref().is_none_or(str::is_empty);
    match op {
        ":-" if empty_or_unset => substitute(arg, lookup, unset),
        "-" if value.is_none() => substitute(arg, lookup, unset),
        ":+" if !empty_or_unset => substitute(arg, lookup, unset),
        "+" if value.is_some() => substitute(arg, lookup, unset),
        ":?" if empty_or_unset => Err(missing(name, arg, lookup, unset)?),
        "?" if value.is_none() => Err(missing(name, arg, lookup, unset)?),
        _ => Ok(value.unwrap_or_default()),
    }
}

fn missing(name: &str, arg: &str, lookup: Lookup<'_>, unset: &mut Vec<String>) -> Result<TemplateError, TemplateError> {
    Ok(TemplateError::MissingRequired {
        name: name.to_owned(),
        reason: substitute(arg, lookup, unset)?,
    })
}

/// Reads a reference without an operator; unset becomes `""` and is recorded.
fn plain(name: &str, lookup: Lookup<'_>, unset: &mut Vec<String>) -> String {
    lookup(name).unwrap_or_else(|| {
        unset.push(name.to_owned());
        String::new()
    })
}

/// Finds the `}` closing the first `{`, skipping the byte after each `{`.
fn first_brace_closing(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut open = 0_usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'}' {
            open = open.saturating_sub(1);
            if open == 0 {
                return Some(i);
            }
        }
        if bytes[i] == b'{' {
            open += 1;
            i += 1;
        }
        i += 1;
    }
    None
}

fn is_name_start(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphabetic()
}

fn name_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|c| !(*c == b'_' || c.is_ascii_alphanumeric()))
        .map_or(bytes.len(), |n| start + n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults(name: &str) -> Option<String> {
        match name {
            "FOO" => Some("first".into()),
            "BAR" => Some(String::new()),
            "JSON" => Some(r#"{"json":2}"#.into()),
            _ => None,
        }
    }

    fn sub(template: &str) -> Result<String, TemplateError> {
        substitute(template, &defaults, &mut Vec::new())
    }

    #[track_caller]
    fn ok(template: &str, expected: &str) {
        assert_eq!(sub(template).as_deref(), Ok(expected), "{template}");
    }

    #[track_caller]
    fn err(template: &str, expected: &str) {
        assert_eq!(sub(template).expect_err(template).to_string(), expected, "{template}");
    }

    #[test]
    fn escaped_and_literal_dollars() {
        ok("$${foo}", "${foo}");
        ok("foo", "foo");
        for template in ["a $ string", "^REGEX$", "$}", "$"] {
            ok(template, template);
        }
    }

    #[test]
    fn invalid_templates() {
        for template in ["${", "${}", "${ }", "${ foo}", "${foo }", "${foo!}"] {
            assert_eq!(sub(template), Err(TemplateError::Invalid), "{template}");
        }
    }

    #[test]
    fn plain_references() {
        ok("$FOO-bar", "first-bar");
        ok("This ${missing} var", "This  var");
        ok("This ${BAR} var", "This  var");
        ok("This $FOO var", "This first var");
        ok("This ${FOO} var", "This first var");
    }

    #[test]
    fn defaults_and_alternates() {
        ok("ok ${missing:-def}", "ok def");
        ok("ok ${missing-def}", "ok def");
        ok("ok ${BAR:-def}", "ok def");
        ok("ok ${FOO:-def}", "ok first");
        ok("ok ${BAR-def}", "ok ");
        ok("ok ${UNSET_VAR:+presence_value}", "ok ");
        ok("ok ${UNSET_VAR+presence_value}", "ok ");
        ok("ok ${FOO:+presence_value}", "ok presence_value");
        ok("ok ${FOO+presence_value}", "ok presence_value");
        ok("ok ${BAR+presence_value}", "ok presence_value");
        ok("ok ${BAR:+presence_value}", "ok ");
        ok("ok ${BAR:-/non:-alphanumeric}", "ok /non:-alphanumeric");
    }

    #[test]
    fn outer_text_does_not_interfere() {
        for (template, expected) in [
            ("-ok ${BAR:-defaultValue}", "-ok defaultValue"),
            ("+ok ${UNSET:-${BAR-defaultValue}}", "+ok "),
            ("-ok ${FOO:-defaultValue}", "-ok first"),
            (":-ok ${UNSET-defaultValue}", ":-ok defaultValue"),
            (":-ok ${BAR-defaultValue}", ":-ok "),
            (":?ok ${BAR-defaultValue}", ":?ok "),
            (":?ok ${BAR:-defaultValue}", ":?ok defaultValue"),
            (":+ok ${BAR:-defaultValue}", ":+ok defaultValue"),
            ("+ok ${BAR-defaultValue}", "+ok "),
            ("?ok ${BAR:-defaultValue}", "?ok defaultValue"),
        ] {
            ok(template, expected);
        }
    }

    #[test]
    fn nested_expansion_in_defaults() {
        for (template, expected) in [
            ("ok ${UNSET_VAR-$FOO}", "ok first"),
            ("ok ${UNSET_VAR-${FOO}}", "ok first"),
            ("ok ${UNSET_VAR-${FOO} ${FOO}}", "ok first first"),
            ("ok ${BAR:-$FOO}", "ok first"),
            ("ok ${BAR:-${FOO}}", "ok first"),
            ("ok ${BAR:-${FOO} ${FOO}}", "ok first first"),
            ("ok ${BAR+$FOO}", "ok first"),
            ("ok ${BAR+$FOO ${FOO:+second}}", "ok first second"),
            ("ok ${SUBDOMAIN:-redis}.${FOO:?}", "ok redis.first"),
        ] {
            ok(template, expected);
        }
    }

    #[test]
    fn required_variables() {
        let missing = "required variable UNSET_VAR is missing a value";
        err(
            "not ok ${UNSET_VAR:?Mandatory Variable Unset}",
            &format!("{missing}: Mandatory Variable Unset"),
        );
        err(
            "not ok ${BAR:?Mandatory Variable Empty}",
            "required variable BAR is missing a value: Mandatory Variable Empty",
        );
        err("not ok ${UNSET_VAR:?}", missing);
        err(
            "not ok ${UNSET_VAR?Mandatory Variable Unset}",
            &format!("{missing}: Mandatory Variable Unset"),
        );
        err("not ok ${UNSET_VAR?}", missing);
        err(
            "not ok ${UNSET_VAR:?Mandatory Variable ${FOO}}",
            &format!("{missing}: Mandatory Variable first"),
        );
        err(
            "not ok ${UNSET_VAR?Mandatory Variable ${FOO}}",
            &format!("{missing}: Mandatory Variable first"),
        );
        ok("ok ${FOO:?err}", "ok first");
        ok("ok ${FOO?err}", "ok first");
        ok("ok ${BAR?err}", "ok ");
    }

    #[test]
    fn first_operator_wins() {
        err(
            "${UNSET_VAR?bar-baz}",
            "required variable UNSET_VAR is missing a value: bar-baz",
        );
        ok("${UNSET_VAR-myerror?msg}", "myerror?msg");
        ok("${FOO?bar-baz}", "first");
        ok("${BAR:-default_value_for_empty_var}", "default_value_for_empty_var");
        ok(
            "${UNSET_VAR-default_value_for_unset_var}",
            "default_value_for_unset_var",
        );
    }

    #[test]
    fn curly_brace_defaults() {
        for template in [r#"ok ${missing:-{"json":1}}"#, r#"ok ${missing-{"json":1}}"#] {
            ok(template, r#"ok {"json":1}"#);
        }
        for template in [r#"ok ${JSON:-{"json":1}}"#, r#"ok ${JSON-{"json":1}}"#] {
            ok(template, r#"ok {"json":2}"#);
        }
    }

    #[test]
    fn unset_plain_references_are_reported() {
        let mut unset = Vec::new();
        substitute("${A:-${B}} $C ${FOO}", &defaults, &mut unset).expect("valid");
        assert_eq!(unset, ["B", "C"]);
    }

    #[test]
    fn unused_defaults_are_not_evaluated() {
        let mut unset = Vec::new();
        assert_eq!(substitute("${A:+${A},}B", &defaults, &mut unset).as_deref(), Ok("B"));
        assert!(unset.is_empty());
        let a_set = |name: &str| (name == "A").then(|| "value".to_owned());
        assert_eq!(substitute("${A?${B}}", &a_set, &mut unset).as_deref(), Ok("value"));
        assert!(unset.is_empty());
    }

    #[test]
    fn unicode_text_passes_through() {
        ok("grüße ${FOO} ✓", "grüße first ✓");
    }
}

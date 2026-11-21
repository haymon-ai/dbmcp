//! Word splitters feeding the context-aware boost in [`crate::context`].
//!
//! Two flavours:
//!
//! * [`push_key_words`] — JSON object keys. Splits on every
//!   non-alphanumeric ASCII char (`_`, `-`, `.`, whitespace, punctuation,
//!   any non-ASCII byte), splits camelCase / `PascalCase` at each new word
//!   (`socialSecurityNumber`, `SSNNumber`), and lowercases the pieces, so
//!   snake, kebab, dotted and camel keys all yield matchable words. An
//!   all-lowercase glued key like `"customerphone"` stays one word.
//! * [`words`] / [`first_word`] — free-form text inside the boost window.
//!   Manual `char::is_alphanumeric || '_'` scan that mirrors Unicode
//!   `\w+` semantics for the inputs the boost actually sees, so
//!   multibyte word characters in keys like `naïve` stay intact.

pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Split `text` into lowercase Unicode word runs (`\w+`-style).
pub(crate) fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for c in text.chars() {
        if is_word_char(c) {
            buf.extend(c.to_lowercase());
        } else if !buf.is_empty() {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// Lowercase first Unicode word run in `text`, if any.
pub(crate) fn first_word(text: &str) -> Option<String> {
    let mut buf = String::new();
    for c in text.chars() {
        if is_word_char(c) {
            buf.extend(c.to_lowercase());
        } else if !buf.is_empty() {
            break;
        }
    }
    (!buf.is_empty()).then_some(buf)
}

/// Push each lowercase word from `key` onto `path`.
///
/// Returns the number of words pushed so the caller can `truncate` the
/// path back to its pre-call length when the subtree is done.
pub(crate) fn push_key_words(path: &mut Vec<String>, key: &str) -> usize {
    let before = path.len();
    for tok in key.split(|c: char| !c.is_ascii_alphanumeric()) {
        let bytes = tok.as_bytes();
        let mut start = 0;
        for i in 1..bytes.len() {
            let (prev, cur) = (bytes[i - 1], bytes[i]);
            let next_lower = bytes.get(i + 1).is_some_and(u8::is_ascii_lowercase);
            // A word starts at a capital after a lower-case letter or a digit
            // (`customerPhone`, `line2Address`), or at the last capital of an
            // acronym that a lower-case run follows (`SSNNumber`).
            if cur.is_ascii_uppercase()
                && (prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_lower))
            {
                path.push(tok[start..i].to_ascii_lowercase());
                start = i;
            }
        }
        if start < bytes.len() {
            path.push(tok[start..].to_ascii_lowercase());
        }
    }
    path.len() - before
}

#[cfg(test)]
mod tests {
    use super::push_key_words;

    fn words(key: &str) -> Vec<String> {
        let mut out = Vec::new();
        push_key_words(&mut out, key);
        out
    }

    #[test]
    fn snake_case() {
        assert_eq!(words("customer_phone_number"), vec!["customer", "phone", "number"]);
    }

    #[test]
    fn kebab_case() {
        assert_eq!(words("customer-phone-number"), vec!["customer", "phone", "number"]);
    }

    #[test]
    fn dotted_path() {
        assert_eq!(words("user.phone.number"), vec!["user", "phone", "number"]);
    }

    #[test]
    fn screaming_snake() {
        assert_eq!(words("API_KEY_LOOKUP"), vec!["api", "key", "lookup"]);
    }

    #[test]
    fn camel_and_pascal_case_split_into_words() {
        assert_eq!(words("customerPhoneNumber"), vec!["customer", "phone", "number"]);
        assert_eq!(words("CustomerPhoneNumber"), vec!["customer", "phone", "number"]);
        assert_eq!(words("socialSecurityNumber"), vec!["social", "security", "number"]);
        assert_eq!(words("APIKey"), vec!["api", "key"]);
        assert_eq!(words("SSNNumber"), vec!["ssn", "number"]);
        assert_eq!(words("XMLHttpRequest"), vec!["xml", "http", "request"]);
        assert_eq!(words("line2Address"), vec!["line2", "address"]);
        assert_eq!(words("SSN"), vec!["ssn"]);
        assert_eq!(words("customerphone"), vec!["customerphone"]);
    }

    #[test]
    fn mixed_separators() {
        assert_eq!(
            words("user.contact-info_phone"),
            vec!["user", "contact", "info", "phone"]
        );
        assert_eq!(words("a b\tc\nd"), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn empty_segments_dropped() {
        assert_eq!(words("__foo__bar__"), vec!["foo", "bar"]);
        assert_eq!(words(""), Vec::<String>::new());
    }

    #[test]
    fn non_ascii_acts_as_separator() {
        // Non-ASCII chars aren't `is_ascii_alphanumeric`, so they split.
        assert_eq!(words("naïveAPI"), vec!["na", "ve", "api"]);
    }
}

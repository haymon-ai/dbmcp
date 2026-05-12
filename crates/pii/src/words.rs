//! Word splitters feeding the context-aware boost in [`crate::context`].
//!
//! Two flavours:
//!
//! * [`push_key_words`] — JSON object keys. Splits on every
//!   non-alphanumeric ASCII char (`_`, `-`, `.`, whitespace, punctuation,
//!   any non-ASCII byte) and lowercases the surviving pieces. camelCase /
//!   `PascalCase` are intentionally left glued: the boost runs under
//!   [`crate::ContextMatchingMode::Substring`] in production, so `"phone"`
//!   matches `"customerphone"` via `str::contains`, and acronym
//!   substrings like `"ipv4"` stay intact against `"ipv4"` keywords.
//! * [`words`] / [`first_word`] — free-form text inside the boost window.
//!   Manual `char::is_alphanumeric || '_'` scan that mirrors Unicode
//!   `\w+` semantics for the inputs the boost actually sees, so
//!   multibyte word characters in keys like `naïve` stay intact.

fn is_word_char(c: char) -> bool {
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
        if !tok.is_empty() {
            path.push(tok.to_ascii_lowercase());
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
    fn camel_and_pascal_case_stay_glued() {
        // camelCase / PascalCase do NOT split — substring matching against
        // recognizer keywords still finds `"phone"` inside `"customerphone"`.
        assert_eq!(words("customerPhoneNumber"), vec!["customerphonenumber"]);
        assert_eq!(words("CustomerPhoneNumber"), vec!["customerphonenumber"]);
        assert_eq!(words("APIKey"), vec!["apikey"]);
        assert_eq!(words("XMLHttpRequest"), vec!["xmlhttprequest"]);
        assert_eq!(words("IPv4Address"), vec!["ipv4address"]);
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
        assert_eq!(words("naïveAPI"), vec!["na", "veapi"]);
    }
}

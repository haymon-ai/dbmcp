//! `PASSWORD_HASH` recognizer (modular-crypt / PHC password-hash strings).
//!
//! Strong, value-only: the modular-crypt format is unmistakable, so no context
//! gating is needed. Covers the three families found in the wild — bcrypt
//! (`$2a$`/`$2b$`/`$2x$`/`$2y$`), the `crypt(3)` sha family (md5 `$1$`,
//! sha256 `$5$`, sha512 `$6$`), and argon2 (`$argon2i$`/`$argon2d$`/`$argon2id$`).
//! Lengths follow each scheme's spec; no lookaround, so the fast regex engine
//! is used and a hash-shaped prefix of an over-long value still redacts.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `PASSWORD_HASH` recognizer (bcrypt, sha-crypt, argon2).
///
/// # Panics
///
/// Panics only if any bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn password_hash() -> Recognizer {
    let s = Score::from_static(0.7);
    // One pass over the value: the three families share the `$…$` modular-crypt
    // shape and have mutually-exclusive scheme prefixes, so a single alternation
    // is unambiguous (and `(?x)` keeps it readable).
    let pattern = Pattern::new(
        "modular-crypt / PHC hash",
        r"(?x)
          \$ (?:
              2[abxy]\$\d{2}\$[./A-Za-z0-9]{53}                                  # bcrypt
            | [156]\$(?:rounds=\d+\$)?[./A-Za-z0-9]{1,16}\$[./A-Za-z0-9]{22,86}  # sha-crypt: md5/sha256/sha512
            | argon2(?:id|i|d)\$(?:v=\d+\$)?m=\d+,t=\d+,p=\d+
              \$[A-Za-z0-9+/]{11,64}\$[A-Za-z0-9+/]{16,86}                       # argon2
          )",
        s,
    )
    .expect("PHC hash pattern compiles");
    Recognizer::new(Entity::PasswordHash, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("PasswordHashRecognizer")
        .with_category(Category::DigitalIdentity)
}

#[cfg(test)]
mod tests {
    use super::password_hash;

    fn matches(text: &str) -> Vec<&str> {
        password_hash()
            .analyze(text)
            .into_iter()
            .map(|r| &text[r.start..r.end])
            .collect()
    }

    #[test]
    fn recognizes_bcrypt() {
        let bcrypt = format!("$2y$12${}", "a".repeat(53));
        assert_eq!(matches(&bcrypt), vec![bcrypt.as_str()]);

        let bcrypt_b = format!("$2b$10$./{}", "a".repeat(51));
        assert_eq!(matches(&bcrypt_b), vec![bcrypt_b.as_str()]);

        let embedded = format!("password={bcrypt} stored");
        assert_eq!(matches(&embedded), vec![bcrypt.as_str()]);
    }

    #[test]
    fn redacts_hash_prefix_of_overlong_value() {
        let overlong = format!("$2y$12${}", "a".repeat(60));
        let hash_prefix = &overlong[.."$2y$12$".len() + 53];
        assert_eq!(matches(&overlong), vec![hash_prefix]);
    }

    #[test]
    fn recognizes_sha_crypt() {
        let sha512 = format!("$6$rounds=5000$abcdefghijklmnop${}", "a".repeat(86));
        assert_eq!(matches(&sha512), vec![sha512.as_str()]);

        let sha256 = format!("$5$saltsalt${}", "z".repeat(43));
        assert_eq!(matches(&sha256), vec![sha256.as_str()]);

        let md5 = format!("$1$abcdefgh${}", "Q".repeat(22));
        assert_eq!(matches(&md5), vec![md5.as_str()]);
    }

    #[test]
    fn recognizes_argon2() {
        let argon = format!("$argon2id$v=19$m=65536,t=3,p=4${}${}", "c".repeat(22), "d".repeat(43));
        assert_eq!(matches(&argon), vec![argon.as_str()]);

        let argon_i = format!("$argon2i$m=4096,t=3,p=1${}${}", "e".repeat(16), "f".repeat(32));
        assert_eq!(matches(&argon_i), vec![argon_i.as_str()]);
    }

    #[test]
    fn rejects_non_hashes() {
        let truncated = format!("$2y$12${}", "a".repeat(40));
        let cases: &[&str] = &[
            "$2c$12$short",
            truncated.as_str(),
            "$3$notreal$xyz",
            "not_a_hash_at_all",
            "$2y$",
            "",
        ];
        for input in cases {
            assert!(matches(input).is_empty(), "input {input:?} should not match");
        }
    }
}

//! Secret string wrapper that keeps values out of [`Debug`] output.
//!
//! Passwords, passphrases, and tokens are stored as [`Secret`] so any struct
//! holding one can `#[derive(Debug)]` without leaking it into logs.

/// String value whose [`Debug`] output is redacted.
///
/// Deliberately implements neither `Display` nor `PartialEq`: reading the
/// value requires an explicit [`Secret::expose`] call.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Returns the wrapped value.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt("[REDACTED]", f)
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn debug_redacts_value() {
        let secret = Secret::from("hunter2");
        assert_eq!(format!("{secret:?}"), "\"[REDACTED]\"");
        assert_eq!(format!("{:?}", Some(secret)), "Some(\"[REDACTED]\")");
    }

    #[test]
    fn expose_returns_value() {
        assert_eq!(Secret::from(String::from("hunter2")).expose(), "hunter2");
    }
}

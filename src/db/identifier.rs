//! Shared identifier validation for all database backends.

use crate::error::AppError;

/// Validates that `name` is a non-empty identifier without control characters.
///
/// # Errors
///
/// Returns [`AppError::InvalidIdentifier`] if the name is empty,
/// whitespace-only, or contains control characters.
pub fn validate_identifier(name: &str) -> Result<(), AppError> {
    if name.is_empty() || name.chars().all(char::is_whitespace) {
        return Err(AppError::InvalidIdentifier(name.to_string()));
    }
    if name.chars().any(char::is_control) {
        return Err(AppError::InvalidIdentifier(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_standard_names() {
        assert!(validate_identifier("users").is_ok());
        assert!(validate_identifier("my_table").is_ok());
        assert!(validate_identifier("DB_123").is_ok());
    }

    #[test]
    fn accepts_hyphenated_names() {
        assert!(validate_identifier("eu-docker").is_ok());
        assert!(validate_identifier("access-logs").is_ok());
    }

    #[test]
    fn accepts_special_chars() {
        assert!(validate_identifier("my.db").is_ok());
        assert!(validate_identifier("123db").is_ok());
        assert!(validate_identifier("café").is_ok());
        assert!(validate_identifier("a b").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_identifier("").is_err());
    }

    #[test]
    fn rejects_whitespace_only() {
        assert!(validate_identifier("   ").is_err());
        assert!(validate_identifier("\t").is_err());
    }

    #[test]
    fn rejects_control_chars() {
        assert!(validate_identifier("test\x00db").is_err());
        assert!(validate_identifier("test\ndb").is_err());
        assert!(validate_identifier("test\x1Fdb").is_err());
    }
}

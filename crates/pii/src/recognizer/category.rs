//! Top-level category tag attached to every recognizer.
//!
//! Categories partition the catalog so consumers can request a tailored
//! recognizer subset (`Category::Financial`, `Category::Government`, …) without
//! enumerating every entity type. Used by the [`crate::analyzer::Analyzer`]
//! builder.

use std::str::FromStr;

/// Closed set of PII categories tagging every built-in recognizer.
///
/// Marked `#[non_exhaustive]` so future additions (`Healthcare`, `Industries`,
/// `International`) are non-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum Category {
    /// Names, dates of birth, employee IDs, generic personal data.
    Personal,
    /// Cards, IBANs, bank accounts, securities, sort/routing codes.
    Financial,
    /// Government-issued IDs (SSN, passport, NHS, NINO, tax IDs).
    Government,
    /// Postal addresses, postcodes, phone numbers.
    Contact,
    /// IP addresses, URLs, MAC addresses.
    Network,
    /// Social handles, API keys, JWTs, private keys.
    DigitalIdentity,
    /// Cryptocurrency wallet addresses.
    Crypto,
}

impl Category {
    /// All variants in declaration order.
    pub const ALL: &'static [Category] = &[
        Category::Personal,
        Category::Financial,
        Category::Government,
        Category::Contact,
        Category::Network,
        Category::DigitalIdentity,
        Category::Crypto,
    ];

    /// Stable kebab-case identifier used as wire format on CLI / env / config.
    #[must_use]
    pub fn as_kebab(self) -> &'static str {
        match self {
            Category::Personal => "personal",
            Category::Financial => "financial",
            Category::Government => "government",
            Category::Contact => "contact",
            Category::Network => "network",
            Category::DigitalIdentity => "digital-identity",
            Category::Crypto => "crypto",
        }
    }
}

/// Error returned by `<Category as FromStr>::from_str` on an unknown kebab string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown PII category: {0}")]
pub struct ParseCategoryError(pub String);

impl FromStr for Category {
    type Err = ParseCategoryError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "personal" => Ok(Category::Personal),
            "financial" => Ok(Category::Financial),
            "government" => Ok(Category::Government),
            "contact" => Ok(Category::Contact),
            "network" => Ok(Category::Network),
            "digital-identity" => Ok(Category::DigitalIdentity),
            "crypto" => Ok(Category::Crypto),
            other => Err(ParseCategoryError(other.to_string())),
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_kebab())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_round_trips_through_kebab() {
        for &cat in Category::ALL {
            let s = cat.as_kebab();
            assert_eq!(Category::from_str(s).expect("kebab round-trip"), cat);
        }
    }

    #[test]
    fn unknown_kebab_fails() {
        assert!(Category::from_str("healthcare").is_err());
    }

    #[test]
    fn all_has_seven_variants() {
        assert_eq!(Category::ALL.len(), 7);
    }
}

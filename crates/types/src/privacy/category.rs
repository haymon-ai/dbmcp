//! Top-level category tag attached to every recognizer.
//!
//! Categories partition the catalog so consumers can request a tailored
//! recognizer subset (`Category::Financial`, `Category::Government`, …) without
//! enumerating every entity type.

/// Closed set of PII categories tagging every built-in recognizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    pub const fn as_str(self) -> &'static str {
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

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_has_seven_variants() {
        assert_eq!(Category::ALL.len(), 7);
    }
}

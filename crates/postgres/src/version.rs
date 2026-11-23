//! `PostgreSQL` server identity from a `version()` string: fork and pre-release.

use dbmcp_sql::support::Variant;

/// Detects a fork that names itself in a `version()` string.
pub(crate) fn variant(version: &str) -> Option<Variant> {
    if version.contains("CockroachDB") {
        Some(Variant::Cockroachdb)
    } else if version.contains("-YB-") {
        Some(Variant::Yugabytedb)
    } else {
        None
    }
}

/// Whether a `version()` string reports a beta, release candidate, or development build.
///
/// Reads the word after `PostgreSQL`, such as `19beta1`, `19rc1`, or `20devel`.
pub(crate) fn pre_release(version: &str) -> bool {
    version
        .strip_prefix("PostgreSQL ")
        .and_then(|rest| rest.split_whitespace().next())
        .is_some_and(|word| ["beta", "rc", "devel"].iter().any(|tag| word.contains(tag)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_reads_forks_from_the_version_text() {
        assert_eq!(
            variant("CockroachDB CCL v24.1.0 (x86_64-pc-linux-gnu, built 2024/05/15 15:43:22, go1.22.2)"),
            Some(Variant::Cockroachdb)
        );
        assert_eq!(
            variant("PostgreSQL 15.2-YB-2.25.0.0-b0 on x86_64-pc-linux-gnu"),
            Some(Variant::Yugabytedb)
        );
        assert_eq!(variant("PostgreSQL 17.4 on x86_64-pc-linux-musl"), None);
    }

    #[test]
    fn pre_release_reads_the_version_word() {
        for version in [
            "PostgreSQL 19beta1 on x86_64-pc-linux-musl, compiled by gcc",
            "PostgreSQL 19rc1 on x86_64-pc-linux-musl",
            "PostgreSQL 20devel on x86_64-pc-linux-gnu",
        ] {
            assert!(pre_release(version), "{version}");
        }
        for version in [
            "PostgreSQL 19.0 on x86_64-pc-linux-musl",
            "PostgreSQL 15.12 (Debian 15.12-1.pgdg120+1) on x86_64-pc-linux-gnu",
            "CockroachDB CCL v24.1.0",
            "",
        ] {
            assert!(!pre_release(version), "{version}");
        }
    }
}

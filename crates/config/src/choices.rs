//! Accepted values per enum setting, in error-message order.
//!
//! Spellings come from each type's `as_str`.

use dbmcp_types::{LogLevel, Transport, database, privacy, ssh};

pub(crate) const TRANSPORT: &[(&str, Transport)] = &[
    (Transport::Stdio.as_str(), Transport::Stdio),
    (Transport::Http.as_str(), Transport::Http),
];

pub(crate) const LOG_LEVEL: &[(&str, LogLevel)] = &[
    (LogLevel::Error.as_str(), LogLevel::Error),
    (LogLevel::Warn.as_str(), LogLevel::Warn),
    (LogLevel::Info.as_str(), LogLevel::Info),
    (LogLevel::Debug.as_str(), LogLevel::Debug),
    (LogLevel::Trace.as_str(), LogLevel::Trace),
];

/// Accepted `database::Backend` setting values, in the order error messages list them.
pub(crate) const DB_BACKEND: &[(&str, database::Backend)] = &[
    (database::Backend::Mysql.as_str(), database::Backend::Mysql),
    (database::Backend::Mariadb.as_str(), database::Backend::Mariadb),
    (database::Backend::Postgres.as_str(), database::Backend::Postgres),
    (database::Backend::Sqlite.as_str(), database::Backend::Sqlite),
];

/// Accepted `privacy::Mode` setting values, in the order error messages list them.
pub(crate) const PRIVACY_MODE: &[(&str, privacy::Mode)] = &[
    (privacy::Mode::None.as_str(), privacy::Mode::None),
    (privacy::Mode::Rules.as_str(), privacy::Mode::Rules),
    (privacy::Mode::Full.as_str(), privacy::Mode::Full),
];

/// Accepted `privacy::Operator` setting values, in the order error messages list them.
pub(crate) const PRIVACY_OPERATOR: &[(&str, privacy::Operator)] = &[
    (privacy::Operator::Replace.as_str(), privacy::Operator::Replace),
    (privacy::Operator::Mask.as_str(), privacy::Operator::Mask),
    (privacy::Operator::Redact.as_str(), privacy::Operator::Redact),
    (privacy::Operator::Hash.as_str(), privacy::Operator::Hash),
];

/// Accepted `privacy::Category` setting values, in the order error messages list them.
pub(crate) const PRIVACY_CATEGORY: &[(&str, privacy::Category)] = &[
    (privacy::Category::Personal.as_str(), privacy::Category::Personal),
    (privacy::Category::Financial.as_str(), privacy::Category::Financial),
    (privacy::Category::Government.as_str(), privacy::Category::Government),
    (privacy::Category::Contact.as_str(), privacy::Category::Contact),
    (privacy::Category::Network.as_str(), privacy::Category::Network),
    (
        privacy::Category::DigitalIdentity.as_str(),
        privacy::Category::DigitalIdentity,
    ),
    (privacy::Category::Crypto.as_str(), privacy::Category::Crypto),
];

/// Accepted `ssh::StrictHostKeyChecking` setting values, in the order error messages list them.
pub(crate) const STRICT_HOST_KEY_CHECKING: &[(&str, ssh::StrictHostKeyChecking)] = &[
    (
        ssh::StrictHostKeyChecking::Yes.as_str(),
        ssh::StrictHostKeyChecking::Yes,
    ),
    (
        ssh::StrictHostKeyChecking::AcceptNew.as_str(),
        ssh::StrictHostKeyChecking::AcceptNew,
    ),
    (ssh::StrictHostKeyChecking::No.as_str(), ssh::StrictHostKeyChecking::No),
];

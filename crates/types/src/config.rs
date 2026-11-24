//! Top-level [`Config`] composing the transport and the database, http, privacy, and ssh sections.

use crate::transport::Transport;
use crate::{database, http, privacy, ssh};

/// Runtime configuration for the MCP server.
///
/// Composes the selected [`Transport`] with [`database::Config`] and the
/// optional [`http::Config`], [`privacy::Config`], and [`ssh::Config`] sections.
/// HTTP config is present only when `transport` / `DBMCP_TRANSPORT` is `http`;
/// privacy config only when `privacy.mode` / `DBMCP_PRIVACY_MODE` is not `none`; SSH
/// config only when `ssh.host` / `DBMCP_SSH_HOST` is set.
#[derive(Clone, Debug)]
pub struct Config {
    /// MCP transport the server runs on.
    pub transport: Transport,

    /// Database connection and behavior settings.
    pub database: database::Config,

    /// HTTP transport settings (present only when the HTTP transport is selected).
    pub http: Option<http::Config>,

    /// Privacy settings (`None` = privacy mode `none`).
    pub privacy: Option<privacy::Config>,

    /// SSH tunnel settings (`None` = connect to the database directly).
    pub ssh: Option<ssh::Config>,
}

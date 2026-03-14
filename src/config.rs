//! Configuration loading from environment variables and `.env` files.
//!
//! All settings have sensible defaults except `DB_USER` and `DB_PASSWORD`
//! which are required. See `.env.example` for the full list.

use std::env;

/// Runtime configuration for the MCP server.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub db_host: String,
    pub db_port: u16,
    pub db_user: String,
    pub db_password: String,
    pub db_name: Option<String>,
    pub db_charset: Option<String>,

    pub db_ssl: bool,
    pub db_ssl_ca: Option<String>,
    pub db_ssl_cert: Option<String>,
    pub db_ssl_key: Option<String>,
    pub db_ssl_verify_cert: bool,
    pub db_ssl_verify_identity: bool,

    pub read_only: bool,
    pub max_pool_size: u32,

    pub allowed_origins: Vec<String>,
    pub allowed_hosts: Vec<String>,

    pub log_level: String,
    pub log_file: String,
    pub log_max_bytes: u64,
    pub log_backup_count: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_host: "127.0.0.1".into(),
            db_port: 3306,
            db_user: String::new(),
            db_password: String::new(),
            db_name: None,
            db_charset: None,
            db_ssl: false,
            db_ssl_ca: None,
            db_ssl_cert: None,
            db_ssl_key: None,
            db_ssl_verify_cert: true,
            db_ssl_verify_identity: false,
            read_only: true,
            max_pool_size: 10,
            allowed_origins: vec![
                "http://localhost".into(),
                "http://127.0.0.1".into(),
                "https://localhost".into(),
                "https://127.0.0.1".into(),
            ],
            allowed_hosts: vec!["localhost".into(), "127.0.0.1".into()],
            log_level: "info".into(),
            log_file: "logs/mcp_server.log".into(),
            log_max_bytes: 10_485_760,
            log_backup_count: 5,
        }
    }
}

impl Config {
    /// Loads configuration from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error string if `DB_USER` or `DB_PASSWORD` is missing,
    /// or if numeric values like `DB_PORT` or `MCP_MAX_POOL_SIZE` are invalid.
    pub fn from_env() -> Result<Self, String> {
        let db_user = env::var("DB_USER")
            .map_err(|_| "DB_USER is required but not set in environment or .env file")?;
        if db_user.is_empty() {
            return Err("DB_USER is empty".into());
        }

        let db_password = env::var("DB_PASSWORD")
            .map_err(|_| "DB_PASSWORD is required but not set in environment or .env file")?;

        let max_pool_size: u32 = env::var("MCP_MAX_POOL_SIZE")
            .unwrap_or_else(|_| "10".into())
            .parse()
            .map_err(|_| "MCP_MAX_POOL_SIZE must be a positive integer")?;
        if max_pool_size == 0 {
            return Err("MCP_MAX_POOL_SIZE must be greater than 0".into());
        }

        Ok(Config {
            db_host: env::var("DB_HOST").unwrap_or_else(|_| "localhost".into()),
            db_port: env::var("DB_PORT")
                .unwrap_or_else(|_| "3306".into())
                .parse()
                .map_err(|_| "DB_PORT must be a valid port number")?,
            db_user,
            db_password,
            db_name: env::var("DB_NAME").ok().filter(|s| !s.is_empty()),
            db_charset: env::var("DB_CHARSET").ok().filter(|s| !s.is_empty()),

            db_ssl: env::var("DB_SSL")
                .unwrap_or_else(|_| "false".into())
                .to_lowercase()
                == "true",
            db_ssl_ca: env::var("DB_SSL_CA").ok().filter(|s| !s.is_empty()),
            db_ssl_cert: env::var("DB_SSL_CERT").ok().filter(|s| !s.is_empty()),
            db_ssl_key: env::var("DB_SSL_KEY").ok().filter(|s| !s.is_empty()),
            db_ssl_verify_cert: env::var("DB_SSL_VERIFY_CERT")
                .unwrap_or_else(|_| "true".into())
                .to_lowercase()
                == "true",
            db_ssl_verify_identity: env::var("DB_SSL_VERIFY_IDENTITY")
                .unwrap_or_else(|_| "false".into())
                .to_lowercase()
                == "true",

            read_only: env::var("MCP_READ_ONLY")
                .unwrap_or_else(|_| "true".into())
                .to_lowercase()
                == "true",
            max_pool_size,

            allowed_origins: env::var("ALLOWED_ORIGINS").map_or_else(
                |_| {
                    vec![
                        "http://localhost".into(),
                        "http://127.0.0.1".into(),
                        "https://localhost".into(),
                        "https://127.0.0.1".into(),
                    ]
                },
                |s| s.split(',').map(|o| o.trim().to_string()).collect(),
            ),
            allowed_hosts: env::var("ALLOWED_HOSTS").map_or_else(
                |_| vec!["localhost".into(), "127.0.0.1".into()],
                |s| s.split(',').map(|h| h.trim().to_string()).collect(),
            ),

            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
            log_file: env::var("LOG_FILE").unwrap_or_else(|_| "logs/mcp_server.log".into()),
            log_max_bytes: env::var("LOG_MAX_BYTES")
                .unwrap_or_else(|_| "10485760".into())
                .parse()
                .unwrap_or(10_485_760),
            log_backup_count: env::var("LOG_BACKUP_COUNT")
                .unwrap_or_else(|_| "5".into())
                .parse()
                .unwrap_or(5),
        })
    }
}

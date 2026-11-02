//! Command-line arguments: the config file and env file paths.
//!
//! Every setting comes from `DBMCP_*` env vars, the `--env` files, or the
//! `--config` YAML files; the CLI only names those files. [`Arguments`]
//! converts into the loader's [`LoadOptions`].

use std::collections::HashMap;
use std::path::PathBuf;

use clap::{ArgAction, Parser};
use dbmcp_config::LoadOptions;

/// Points users at the settings reference, since no setting has a flag.
const AFTER_HELP: &str = "\
Settings come from DBMCP_* environment variables, the --env files, or the
--config YAML files (process env > env files > config files > defaults).
Every setting is listed at https://dbmcp.haymon.ai/docs/configuration";

/// Top-level CLI arguments parsed by clap.
#[derive(Debug, Parser)]
#[command(about = "Database MCP Server", after_help = AFTER_HELP)]
pub(crate) struct Arguments {
    /// Path to a YAML config file; repeat to layer several, later wins [env: DBMCP_CONFIG]
    #[allow(clippy::doc_markdown, reason = "doc comment is the verbatim --help text")]
    #[arg(long = "config", value_name = "PATH", action = ArgAction::Append)]
    config: Vec<PathBuf>,

    /// Path to an env file; repeat to layer several, later wins [env: DBMCP_ENV_FILE]
    #[allow(clippy::doc_markdown, reason = "doc comment is the verbatim --help text")]
    #[arg(long = "env", value_name = "PATH", action = ArgAction::Append)]
    env: Vec<PathBuf>,
}

impl Arguments {
    /// Builds the loader inputs from the flags and the process.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the current directory cannot be read or a
    /// `DBMCP_*` variable is not valid UTF-8.
    pub(crate) fn load_options(&self) -> std::io::Result<LoadOptions> {
        let mut environment = HashMap::new();
        for (name, value) in std::env::vars_os() {
            match (name.into_string(), value.into_string()) {
                (Ok(name), Ok(value)) => {
                    environment.insert(name, value);
                }
                (Ok(name), Err(_)) if name.starts_with("DBMCP_") => {
                    return Err(std::io::Error::other(format!("{name} is not valid UTF-8")));
                }
                // Other non-UTF-8 entries cannot name a setting or a `${}` variable.
                _ => {}
            }
        }
        Ok(LoadOptions {
            config: self.config.clone(),
            env_files: self.env.clone(),
            environment,
            working_dir: std::env::current_dir()?,
            home: dbmcp_config::home_dir(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{CommandFactory, Parser};

    use super::Arguments;

    fn try_parse(args: &[&str]) -> Result<Arguments, clap::Error> {
        Arguments::try_parse_from(std::iter::once("dbmcp").chain(args.iter().copied()))
    }

    #[test]
    fn accepts_config_and_env_paths() {
        let args = try_parse(&["--config", "dbmcp.yaml", "--env", ".env"]).expect("valid");
        let options = args.load_options().expect("cwd");
        assert_eq!(options.config, [PathBuf::from("dbmcp.yaml")]);
        assert_eq!(options.env_files, [PathBuf::from(".env")]);
    }

    #[test]
    fn repeated_config_flags_keep_their_order() {
        let options = try_parse(&["--config", "b.yaml", "--config", "a.yaml"])
            .expect("valid")
            .load_options()
            .expect("cwd");
        assert_eq!(options.config, ["b.yaml", "a.yaml"].map(PathBuf::from));
    }

    #[test]
    fn repeated_env_flags_keep_their_order() {
        let options = try_parse(&["--env", "b.env", "--env", "a.env", "--env", "b.env"])
            .expect("valid")
            .load_options()
            .expect("cwd");
        assert_eq!(options.env_files, ["b.env", "a.env", "b.env"].map(PathBuf::from));
    }

    #[test]
    fn no_flags_is_valid() {
        let options = try_parse(&[]).expect("valid").load_options().expect("cwd");
        assert!(options.config.is_empty() && options.env_files.is_empty());
    }

    #[test]
    fn settings_flags_and_subcommands_are_rejected() {
        for args in [
            &["--db-backend", "sqlite"][..],
            &["--transport", "http"],
            &["--log-level", "debug"],
            &["--pii"],
            &["stdio"],
        ] {
            assert!(try_parse(args).is_err(), "{args:?} must be rejected");
        }
    }

    #[test]
    fn help_lists_env_names_and_points_to_the_docs() {
        let help = Arguments::command().render_help().to_string();
        assert!(
            help.contains("--config <PATH>") && help.contains("[env: DBMCP_CONFIG]"),
            "{help}"
        );
        assert!(
            help.contains("--env <PATH>") && help.contains("[env: DBMCP_ENV_FILE]"),
            "{help}"
        );
        assert!(help.contains("https://dbmcp.haymon.ai/docs/configuration"), "{help}");
    }
}

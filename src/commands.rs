//! ClardRs Subcommands
//!
//! This is where you specify the subcommands of your application.
//!
//! The default application comes with two subcommands:
//!
//! - `start`: launches the application
//! - `--version`: print application version
//!
//! See the `impl Configurable` below for how to specify the path to the
//! application's configuration file.

mod groups;
mod metaversion;
mod proxys;
mod traffic;

use crate::{
    commands::{
        groups::GroupsCmd, metaversion::BackendVersionCmd, proxys::ProxysCmd, traffic::TrafficCmd,
    },
    config::ClardRsConfig,
};
use abscissa_core::{Command, Configurable, FrameworkError, Runnable, config::Override};
use std::path::PathBuf;

/// ClardRs Configuration Filename
pub const CONFIG_FILE: &str = "clard_rs.toml";

/// ClardRs Subcommands
/// Subcommands need to be listed in an enum.
#[derive(clap::Parser, Command, Debug, Runnable)]
pub enum ClardRsCmd {
    /// The `backend-version` subcommand
    BackendVersion(BackendVersionCmd),
    /// Proxys
    Proxys(ProxysCmd),
    /// Groups
    Groups(GroupsCmd),
    /// Display real-time traffic of clash
    Traffic(TrafficCmd),
}

/// Entry point for the application. It needs to be a struct to allow using subcommands!
#[derive(clap::Parser, Command, Debug)]
#[command(author, about, version)]
pub struct EntryPoint {
    #[command(subcommand)]
    cmd: ClardRsCmd,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Use the specified config file
    #[arg(short, long)]
    pub config: Option<String>,
}

impl Runnable for EntryPoint {
    fn run(&self) {
        self.cmd.run()
    }
}

/// This trait allows you to define how application configuration is loaded.
impl Configurable<ClardRsConfig> for EntryPoint {
    /// Location of the configuration file
    fn config_path(&self) -> Option<PathBuf> {
        // Check if the config file exists, and if it does not, ignore it.
        // If you'd like for a missing configuration file to be a hard error
        // instead, always return `Some(CONFIG_FILE)` here.
        let filename = self
            .config
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| CONFIG_FILE.into());

        if filename.exists() {
            Some(filename)
        } else {
            None
        }
    }

    /// Apply changes to the config after it's been loaded, e.g. overriding
    /// values in a config file using command-line options.
    ///
    /// This can be safely deleted if you don't want to override config
    /// settings from command-line options.
    fn process_config(&self, config: ClardRsConfig) -> Result<ClardRsConfig, FrameworkError> {
        match &self.cmd {
            ClardRsCmd::BackendVersion(_) => Ok(config),
            ClardRsCmd::Traffic(_) => Ok(config), 
            // If you don't need special overrides for some
            // subcommands, you can just use a catch all
            _ => Ok(config),
        }
    }
}

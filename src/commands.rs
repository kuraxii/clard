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
mod proxyise;
mod traffic;

use std::{fs, path::PathBuf};

use abscissa_core::{Command, Configurable, FrameworkError, Runnable};

use crate::{
    commands::{groups::GroupsCmd, metaversion::BackendVersionCmd, proxyise::ProxiesCmd, traffic::TrafficCmd},
    config::ClardRsConfig,
};

/// ClardRs Configuration Filename
pub const CONFIG_FILE: &str = "~/.config/clard-rs/clard-rs.toml";

/// ClardRs Subcommands
/// Subcommands need to be listed in an enum.
#[derive(clap::Parser, Command, Debug, Runnable)]
pub enum ClardRsCmd {
    /// The `backend-version` subcommand
    BackendVersion(BackendVersionCmd),
    /// Proxys
    Proxies(ProxiesCmd),
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

/// 加载配置文件
impl Configurable<ClardRsConfig> for EntryPoint {
    /// 优先采用命令行参数中的配置路径，其次使用默认配置路径
    fn config_path(&self) -> Option<PathBuf> {
        let filename = self
            .config
            .as_ref()
            .map(|path| PathBuf::from(shellexpand::tilde(path).into_owned()))
            .unwrap_or_else(|| shellexpand::tilde(CONFIG_FILE).into_owned().into());

        filename.try_exists().map_or(None, |_| {
            if let Some(parent) = filename.parent() {
                fs::create_dir_all(parent).unwrap();
            }

            // 将默认配置结构体转为 TOML 字符串
            let default_toml = toml::to_string_pretty(&ClardRsConfig::default()).unwrap();

            fs::write(filename.clone(), default_toml).unwrap();
            Some(filename)
        })
    }

    /// 在配置加载后应用更改，例如使用命令行选项覆盖配置文件中的值。如果您不想使用命令行选项覆盖配置设置，可以安全地删除它。
    fn process_config(&self, config: ClardRsConfig) -> Result<ClardRsConfig, FrameworkError> {
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shellexpand() {
        let expanded = shellexpand::tilde(CONFIG_FILE).into_owned();
        println!("{}", expanded);
    }
}

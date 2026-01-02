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

use crate::{
    commands::{groups::GroupsCmd, metaversion::BackendVersionCmd, proxyise::ProxiesCmd, traffic::TrafficCmd},
    config::ClardRsConfig,
};

/// ClardRs Configuration Filename
pub const CONFIG_FILE: &str = "~/.config/clard-rs/clard-rs.toml";

/// ClardRs Subcommands
/// Subcommands need to be listed in an enum.
#[derive(clap::Parser, Debug)]
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
#[derive(clap::Parser, Debug)]
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

/// 加载配置文件
/// 优先采用命令行参数中的配置路径，其次使用默认配置路径
impl EntryPoint {
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

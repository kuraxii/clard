
use clap::{Subcommand};

use crate::{
    config::ClardRsConfig,
};

/// ClardRs Configuration Filename
pub const CONFIG_FILE: &str = "~/.config/clard-rs/clard-rs.toml";

/// ClardRs Subcommands
/// Subcommands need to be listed in an enum.
#[derive(clap::Parser, Debug)]
pub enum ClardRsCmd {
    /// 执行测试子命令
    Test(TestCmd),
}

#[derive(clap::Args, Debug)]
pub struct TestCmd{
    name: Option<String>
}


#[derive(clap::Parser, Debug)]
#[command(author, about, version)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<ClardRsCmd>,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long)]
    pub tui: bool,
}

/// 加载配置文件
/// 优先采用命令行参数中的配置路径，其次使用默认配置路径

    // fn config_path(&self) -> Option<PathBuf> {
    //     let filename = self
    //         .config
    //         .as_ref()
    //         .map(|path| PathBuf::from(shellexpand::tilde(path).into_owned()))
    //         .unwrap_or_else(|| shellexpand::tilde(CONFIG_FILE).into_owned().into());

    //     filename.try_exists().map_or(None, |_| {
    //         if let Some(parent) = filename.parent() {
    //             fs::create_dir_all(parent).unwrap();
    //         }

    //         // 将默认配置结构体转为 TOML 字符串
    //         let default_toml = toml::to_string_pretty(&ClardRsConfig::default()).unwrap();

    //         fs::write(filename.clone(), default_toml).unwrap();
    //         Some(filename)
    //     })
    // }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shellexpand() {
        let expanded = shellexpand::tilde(CONFIG_FILE).into_owned();
        println!("{}", expanded);
    }
}

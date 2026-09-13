/// ClardRs Configuration Filename
pub const CONFIG_FILE: &str = "~/.config/clard-rs/clard-rs.toml";

/// ClardRs Subcommands
/// Subcommands need to be listed in an enum.
#[derive(clap::Parser, Debug)]
pub enum ClardRsCmd {
    /// 执行测试子命令
    Test(TestCmd),
    /// 订阅配置管理：URL 导入 / 列表 / 更新 / 删除 / 当前
    Profiles(ProfilesCmd),
}

#[derive(clap::Args, Debug)]
pub struct ProfilesCmd {
    #[command(subcommand)]
    pub cmd: ProfilesSub,
}

#[derive(clap::Subcommand, Debug)]
pub enum ProfilesSub {
    /// 从 URL 导入订阅；同 URL 已存在则覆盖更新
    Import {
        /// 订阅 URL
        url: String,
        /// 配置名（缺省取 URL host）
        #[arg(long)]
        name: Option<String>,
        /// 定时更新间隔（秒，0=关闭；定时器一期不实现）
        #[arg(long, default_value_t = 0)]
        interval: u64,
    },
    /// 列出所有配置
    List,
    /// 重新下载并覆盖指定配置
    Update {
        /// 配置 uid
        uid: String,
    },
    /// 删除指定配置
    Remove {
        /// 配置 uid
        uid: String,
    },
    /// 显示当前配置
    Current,
    /// 切换当前配置
    SetCurrent {
        /// 配置 uid
        uid: String,
    },
}

#[derive(clap::Args, Debug)]
pub struct TestCmd {
    name: Option<String>,
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

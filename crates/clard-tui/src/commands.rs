//! clard CLI（`clard` 二进制入口，doc/01 §7）。
//!
//! 无子命令时进入 TUI；`profiles` 子命令为订阅管理的命令行入口（经 IPC 调 helper）。
//! 早期 clash-verge 模板残留（Test 子命令、clard-rs 配置路径等）已清理。

use clap::{Args, Parser, Subcommand};

/// 顶层子命令。
#[derive(Parser, Debug)]
pub enum ClardCmd {
    /// 订阅配置管理：URL 导入 / 列表 / 更新 / 删除 / 当前 / 切换 / 生成运行时配置
    Profiles(ProfilesCmd),
}

#[derive(Args, Debug)]
pub struct ProfilesCmd {
    #[command(subcommand)]
    pub cmd: ProfilesSub,
}

#[derive(Subcommand, Debug)]
pub enum ProfilesSub {
    /// 从 URL 导入订阅；同 URL 已存在则覆盖更新
    Import {
        /// 订阅 URL
        url: String,
        /// 配置名（缺省取 URL host）
        #[arg(long)]
        name: Option<String>,
        /// 定时更新间隔（秒，0=关闭）
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
    /// 生成运行时配置（归一化+合并+托管字段，打印到 stdout；供调试/验证）
    Gen {
        /// 配置 uid
        uid: String,
    },
}

#[derive(Parser, Debug)]
#[command(author, about, version)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<ClardCmd>,

    /// 强制以 TUI 模式启动（默认无子命令即 TUI）
    #[arg(long)]
    pub tui: bool,
}

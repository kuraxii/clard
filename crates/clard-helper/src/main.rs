//! clard-helper：root daemon + 提权子命令
//!
//! 架构约束（见 doc/01-方案设计.md §3/§4/§5）：
//! - 不链接 `clard-core`（保持 root 侧代码最小），只依赖 `clard-proto` 契约；
//! - 常驻运行：flock 单实例 → 启动自检 → unix socket RPC（`SO_PEERCRED`）→ 审计双写；
//! - 子命令：install / uninstall / cleanup-tun / authorize（仅这些需要 su/pkexec）。

#![forbid(unsafe_code)]
#![warn(
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications,
    clippy::perf,
    clippy::style,
    clippy::redundant_closure
)]

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "clard-helper", about = "Clard 特权助手：拥有 mihomo 核心与 TUN")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 常驻运行（systemd ExecStart）：flock → 启动自检 → unix socket RPC
    Run,
    /// 安装：写 authorized_uid、装 unit、enable --now（仅此命令经 pkexec）
    Install,
    /// 卸载：cleanup-tun → 停核心 → disable → 删 unit 与 /etc/clard
    Uninstall,
    /// 紧急恢复直连：幂等清理 TUN 残留（ip rule / route / 网卡）
    CleanupTun,
    /// 变更授权用户 uid（重写 authorized_uid + chown socket + 审计）
    Authorize {
        /// 新授权用户的 uid
        uid: u32,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Run => not_yet("run"),
        Command::Install => not_yet("install"),
        Command::Uninstall => not_yet("uninstall"),
        Command::CleanupTun => not_yet("cleanup-tun"),
        Command::Authorize { uid } => {
            tracing::warn!("authorize uid={uid}: 未实现（里程碑 M1）");
            std::process::exit(1);
        }
    }
}

fn not_yet(sub: &str) -> ! {
    tracing::warn!("clard-helper: 子命令 `{sub}` 尚未实现（里程碑 M1，见 doc/01 §13）");
    std::process::exit(1);
}

//! clard-helper：系统级特权助手（root daemon + 提权子命令）
//!
//! 架构约束（doc/01 §3/§4/§5）：
//! - 不链接 `clard-core`（保持 root 侧代码最小），只依赖 `clard-proto` 契约；
//! - **系统级服务、不分用户**：数据全在 /var，socket 0666 全开放，`SO_PEERCRED` 记录 actor 审计；
//! - 常驻运行：flock 单实例 → unix socket RPC → 审计双写；
//! - 安装/卸载由 RPM 包管理承担（packaging/clard.spec 的 %post/%preun，doc/01 §4.1/§4.4）。

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

mod audit;
mod autoupdate;
mod backup;
mod core;
mod daemon;
mod logs;
mod profiles;
mod rpc;
mod settings;
mod tun;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "clard-helper", about = "Clard 系统级特权助手：拥有核心、TUN 与全部数据")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 常驻运行（systemd ExecStart）：flock → unix socket RPC（0666，SO_PEERCRED 审计）
    Run,
    /// 紧急恢复直连：幂等清理 TUN 残留（ip rule / route / 网卡）
    CleanupTun,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).init();
    let cli = Cli::parse();
    match cli.cmd {
        Command::Run => {
            if let Err(e) = daemon::run().await {
                eprintln!("clard-helper: {e}");
                std::process::exit(1);
            }
        }
        Command::CleanupTun => {
            // §6.4：幂等清理（ExecStopPost / 手动 / 启动自检共用同一实现）
            let (clean, residuals) = tun::cleanup_tun(&tun::Tools::system()).await;
            if clean {
                println!("TUN 残留已清理完毕（无残留）");
            } else {
                println!("TUN 清理完成，仍有残余: {}", residuals.join(", "));
                std::process::exit(1);
            }
        }
    }
}

//! clard 入口：无子命令/`--tui` 进 TUI；`profiles` 子命令为 CLI 订阅管理。

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]
#![cfg_attr(
    test,
    allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use clap::Parser;
use clard_config::config_gen::ConfigGenOptions;
use clard_core::profiles::{HttpFetcher, SubscriptionFetcher};
use clard_proto::{ProfileImport, Request, Response};
use clard_tui::{
    commands::{ClardCmd, Cli, ProfilesSub},
    error::Result,
    rpc, start_clard,
};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    // --tui 优先
    if !cli.tui {
        match cli.cmd {
            Some(ClardCmd::Profiles(profiles)) => run_profiles_cmd(profiles.cmd).await?,
            None => start_clard().await?,
        };
    } else {
        start_clard().await?
    }

    Ok(())
}

/// 订阅配置管理子命令（临时 CLI 入口；正式界面见 doc/02 §3.2）。
/// 所有操作经 IPC 发给 helper（系统级服务，doc/01 §7）。
async fn run_profiles_cmd(cmd: ProfilesSub) -> Result<()> {
    match cmd {
        ProfilesSub::Import { url, name, interval } => {
            // 下载 → 提交原始订阅 raw，helper 侧 clard-config 转换（§8.3 A）
            let raw = HttpFetcher::new(reqwest::Client::new()).fetch(&url).await?;
            let resp = rpc::call(&Request::ProfileImport(ProfileImport {
                name,
                url,
                interval,
                yaml: raw,
                info: None,
            }))
            .await?;
            match resp {
                Response::ProfileImported { uid, updated } => {
                    if updated {
                        println!("同 URL 已覆盖更新: {uid}");
                    } else {
                        println!("已导入: {uid}");
                    }
                }
                other => return Err(rpc::unexpected(other).into()),
            }
        }
        ProfilesSub::List => {
            let resp = rpc::call(&Request::ProfileList).await?;
            match resp {
                Response::ProfileList { current, items } => {
                    if items.is_empty() {
                        println!("(无配置)");
                    }
                    for p in &items {
                        let mark = if current.as_deref() == Some(p.uid.as_str()) {
                            "*"
                        } else {
                            " "
                        };
                        println!("{mark} {}  {}  {}  updated={:?}", p.uid, p.name, p.url, p.updated_at);
                    }
                }
                other => return Err(rpc::unexpected(other).into()),
            }
        }
        ProfilesSub::Update { uid } => {
            // 取回 URL → 重新下载 → 归一化 → 同 URL 覆盖更新
            let resp = rpc::call(&Request::ProfileGet { uid: uid.clone() }).await?;
            let (url, interval) = match resp {
                Response::ProfileContent { item, .. } => (item.url, item.interval),
                other => return Err(rpc::unexpected(other).into()),
            };
            let raw = HttpFetcher::new(reqwest::Client::new()).fetch(&url).await?;
            let resp = rpc::call(&Request::ProfileImport(ProfileImport {
                name: None,
                url,
                interval,
                yaml: raw,
                info: None,
            }))
            .await?;
            match resp {
                Response::ProfileImported { uid: _uid, updated } => {
                    println!("已更新: {uid}（覆盖更新: {updated}）");
                }
                other => return Err(rpc::unexpected(other).into()),
            }
        }
        ProfilesSub::Remove { uid } => {
            let resp = rpc::call(&Request::ProfileRemove { uid: uid.clone() }).await?;
            rpc::expect_ok(resp)?;
            println!("已删除: {uid}");
        }
        ProfilesSub::Current => {
            let resp = rpc::call(&Request::ProfileList).await?;
            match resp {
                Response::ProfileList { current, items } => {
                    if let Some(cur) = current {
                        if let Some(p) = items.iter().find(|p| p.uid == cur) {
                            println!("{}  {}", p.uid, p.name);
                        } else {
                            println!("(当前配置不存在)");
                        }
                    } else {
                        println!("(无当前配置)");
                    }
                }
                other => return Err(rpc::unexpected(other).into()),
            }
        }
        ProfilesSub::SetCurrent { uid } => {
            let resp = rpc::call(&Request::ProfileSetCurrent { uid: uid.clone() }).await?;
            rpc::expect_ok(resp)?;
            println!("已切换当前配置: {uid}");
        }
        ProfilesSub::Gen { uid } => {
            // 取回原始 yaml → config_gen 生成运行时配置（打印；正式链路经 ApplyConfig 交 helper）
            let resp = rpc::call(&Request::ProfileGet { uid: uid.clone() }).await?;
            let yaml = match resp {
                Response::ProfileContent { yaml, .. } => yaml,
                other => return Err(rpc::unexpected(other).into()),
            };
            let runtime = clard_config::config_gen::generate(&yaml, None, &ConfigGenOptions::default())?;
            println!("{runtime}");
        }
    }
    Ok(())
}

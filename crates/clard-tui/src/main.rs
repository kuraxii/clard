//! Main entry point for ClardRs

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]

use clap::Parser;
use clard_core::config_gen::ConfigGenOptions;
use clard_core::profiles::{HttpFetcher, ImportOutcome, ProfilesStore, default_config_dir};
use clard_tui::{
    commands::{ClardRsCmd, Cli, ProfilesSub},
    error::Result,
    start_clard,
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
            Some(cmd) => match cmd {
                ClardRsCmd::Test(name) => println!("Test! {:?}", name),
                ClardRsCmd::Profiles(profiles) => run_profiles_cmd(profiles.cmd).await?,
            },
            None => start_clard().await?,
        };
    } else {
        start_clard().await?
    }

    Ok(())
}

/// 订阅配置管理子命令（临时 CLI 入口；正式界面见 doc/02 §3.2）。
async fn run_profiles_cmd(cmd: ProfilesSub) -> Result<()> {
    let mut store = ProfilesStore::open(default_config_dir())?;
    let fetcher = HttpFetcher::new(reqwest::Client::new());
    match cmd {
        ProfilesSub::Import {
            url,
            name,
            interval,
        } => match store.import(&url, name.as_deref(), interval, &fetcher).await? {
            ImportOutcome::Created { uid } => println!("已导入: {uid}"),
            ImportOutcome::Updated { uid } => println!("同 URL 已覆盖更新: {uid}"),
        },
        ProfilesSub::List => {
            if store.list().is_empty() {
                println!("(无配置)");
            }
            for p in store.list() {
                println!("{}  {}  {}  updated={:?}", p.uid, p.name, p.url, p.updated_at);
            }
        }
        ProfilesSub::Update { uid } => {
            store.update(&uid, &fetcher).await?;
            println!("已更新: {uid}");
        }
        ProfilesSub::Remove { uid } => {
            store.remove(&uid)?;
            println!("已删除: {uid}");
        }
        ProfilesSub::Current => match store.current() {
            Some(p) => println!("{}  {}", p.uid, p.name),
            None => println!("(无当前配置)"),
        },
        ProfilesSub::SetCurrent { uid } => {
            store.set_current(&uid)?;
            println!("已切换当前配置: {uid}");
        }
        ProfilesSub::Gen { uid } => {
            let path = store
                .content_path(&uid)
                .ok_or_else(|| clard_core::profiles::ProfilesError::NotFound { uid: uid.clone() })?;
            let content = std::fs::read_to_string(&path)?;
            let runtime =
                clard_core::config_gen::generate(&content, None, &ConfigGenOptions::default())?;
            println!("{runtime}");
        }
    }
    Ok(())
}

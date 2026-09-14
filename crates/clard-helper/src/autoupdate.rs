//! 订阅自动更新（doc/05 §2 R2.8 / doc/01 §7.1）：helper 全局定时。
//!
//! 间隔存 `clard.toml`（默认 6 小时，0=关）；每 60s 检查一次，到点串行重拉所有
//! **到期**（`updated_at + interval <= now`）的 remote 订阅：下载内容**原样落盘**
//! 覆盖（config_gen 生成时统一归一化）→ 解析 `subscription-userinfo` 更新流量/到期
//! → 刷新 `updated_at`（下次 = updated_at + 间隔）→ 审计 `profile.update`；失败跳过本轮记审计。
//!
//! 锁序约定：**先 store 后 settings**（与 daemon RPC 路径一致，避免死锁）。

use std::{sync::Arc, time::Duration};

use clard_proto::SubscriptionInfo;
use tokio::sync::Mutex;

use crate::audit::{Actor, Audit};
use crate::profiles::{ProfileKind, ProfilesStore};
use crate::settings::SettingsStore;

/// 检查周期（秒）。
const TICK_SECS: u64 = 60;
/// 单次下载超时。
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// 启动自动更新后台任务（daemon 常驻期间一直运行）。
pub fn spawn(store: Arc<Mutex<ProfilesStore>>, settings: Arc<Mutex<SettingsStore>>, audit: Arc<Audit>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(TICK_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            run_once(&store, &settings, &audit).await;
        }
    });
}

async fn run_once(store: &Mutex<ProfilesStore>, settings: &Mutex<SettingsStore>, audit: &Audit) {
    let interval_hours = {
        let s = settings.lock().await;
        s.get().auto_update_interval_hours
    };
    if interval_hours == 0 {
        return;
    }
    let interval = interval_hours * 3600;

    let due: Vec<(String, String)> = {
        let st = store.lock().await;
        let now = now_unix();
        st.list()
            .iter()
            .filter(|p| p.kind == ProfileKind::Remote)
            .filter(|p| p.updated_at.is_none_or(|t| t + interval as i64 <= now))
            .map(|p| (p.url.clone(), p.name.clone()))
            .collect()
    };

    for (url, name) in due {
        let op_id = audit.intent("profile.update", &Actor::system(), &format!("auto update {name}"));
        match fetch_subscription(&url).await {
            Ok((yaml, info)) => {
                let mut st = store.lock().await;
                match st.auto_update(&url, &yaml, Some(info)) {
                    Ok(true) => audit.result("profile.update", &op_id, &Actor::system(), "ok", None, None),
                    Ok(false) => audit.result("profile.update", &op_id, &Actor::system(), "missing", None, None),
                    Err(e) => audit.result("profile.update", &op_id, &Actor::system(), "error", Some(&e.to_string()), None),
                }
            }
            Err(e) => {
                tracing::warn!("auto update {name} failed: {e}");
                audit.result("profile.update", &op_id, &Actor::system(), "error", Some(&e.to_string()), None);
            }
        }
    }
}

/// 下载订阅原样内容 + 解析 `subscription-userinfo`。
async fn fetch_subscription(url: &str) -> Result<(String, SubscriptionInfo), String> {
    let resp = reqwest::Client::new()
        .get(url)
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let info = resp
        .headers()
        .get("subscription-userinfo")
        .and_then(|v| v.to_str().ok())
        .map(parse_subscription_userinfo)
        .unwrap_or_default();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok((body, info))
}

/// 解析 `subscription-userinfo`（helper 不链接 clard-core，此处为最小重复实现）。
fn parse_subscription_userinfo(header: &str) -> SubscriptionInfo {
    let mut info = SubscriptionInfo::default();
    for pair in header.split(';') {
        let Some((k, v)) = pair.trim().split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "upload" => info.upload = v.parse().unwrap_or(0),
            "download" => info.download = v.parse().unwrap_or(0),
            "total" => info.total = v.parse().unwrap_or(0),
            "expire" => info.expire = v.parse().ok().filter(|n| *n > 0),
            _ => {}
        }
    }
    info
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{parse_subscription_userinfo, SubscriptionInfo};

    #[test]
    fn parse_userinfo_extracts_fields() {
        let info = parse_subscription_userinfo("upload=1; download=2; total=3; expire=1700000000");
        assert_eq!((info.upload, info.download, info.total), (1, 2, 3));
        assert_eq!(info.expire, Some(1_700_000_000));
    }

    #[test]
    fn parse_userinfo_zero_expire_and_garbage() {
        assert_eq!(parse_subscription_userinfo("expire=0").expire, None);
        assert_eq!(parse_subscription_userinfo("garbage"), SubscriptionInfo::default());
    }
}

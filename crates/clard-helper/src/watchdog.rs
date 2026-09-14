//! 崩溃自愈 watchdog（doc/01 §5.4/§6.5）。
//!
//! - **核心崩溃退避重启**：核心在窗口 600s 内崩溃 ≥10 次 → 超限：cleanup-tun（fail-open）+
//!   审计 `watchdog.failopen` + `Degraded` 事件，不再自动重启（用户可手动 StartCore）。
//! - **TUN 健康检查**：每 3s 检查（`GET /version` + `ip link clard0 UP` + table 2023 +
//!   rule 9100），连续 3 次不满足 → 同上 fail-open。
//!
//! 原则：TUN 故障必须 fail-open（恢复直连），绝不 fail-closed（全机断网）。

use std::sync::Arc;
use std::time::Duration;

use clard_proto::Event;
use tokio::sync::{broadcast, Mutex};

use crate::audit::{Actor, Audit};
use crate::core::CoreManager;
use crate::settings::SettingsStore;
use crate::tun;

/// 核心崩溃检测间隔。
const CORE_CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// TUN 健康检查间隔（§6.5：每 3s）。
const TUN_CHECK_INTERVAL: Duration = Duration::from_secs(3);
/// TUN 连续不健康次数阈值（≈9s）。
const TUN_FAIL_THRESHOLD: u32 = 3;

/// 启动两个 watchdog 常驻任务。
pub fn spawn(
    settings: Arc<Mutex<SettingsStore>>,
    core: Arc<Mutex<CoreManager>>,
    audit: Arc<Audit>,
    events: broadcast::Sender<Event>,
) {
    // 核心崩溃退避重启
    tokio::spawn({
        let core = core.clone();
        let audit = audit.clone();
        let events = events.clone();
        async move {
            loop {
                tokio::time::sleep(CORE_CHECK_INTERVAL).await;
                check_core_crash(&core, &audit, &events).await;
            }
        }
    });
    // TUN 健康 watchdog（核心运行且 TUN 启用时）
    tokio::spawn({
        let core = core.clone();
        let settings = settings.clone();
        let audit = audit.clone();
        let events = events.clone();
        async move {
            let mut fail_count = 0u32;
            let mut degraded = false;
            loop {
                tokio::time::sleep(TUN_CHECK_INTERVAL).await;
                if check_tun_health(&core, &settings).await {
                    fail_count = 0;
                    degraded = false;
                    continue;
                }
                fail_count += 1;
                if fail_count >= TUN_FAIL_THRESHOLD && !degraded {
                    degraded = true;
                    fail_open(&audit, &events, "tun-health").await;
                }
            }
        }
    });
}

/// 核心崩溃检测：期望运行但已停止 → 退避重启；超限 → fail-open。
async fn check_core_crash(
    core: &Mutex<CoreManager>,
    audit: &Audit,
    events: &broadcast::Sender<Event>,
) {
    enum Action {
        Restart(Duration),
        FailOpen,
    }
    let action = {
        let mut core = core.lock().await;
        if !core.is_want_running() || core.state() == "running" {
            return;
        }
        match core.crash_backoff() {
            Some(delay) => Action::Restart(delay),
            None => {
                core.set_want_running(false);
                Action::FailOpen
            }
        }
    };
    match action {
        Action::Restart(delay) => {
            tokio::time::sleep(delay).await;
            let mut core = core.lock().await;
            if let Err(e) = core.start().await {
                tracing::warn!("watchdog 重启核心失败: {e}");
            }
            let _ = events.send(Event::CoreStatusChanged);
        }
        Action::FailOpen => fail_open(audit, events, "core-crash").await,
    }
}

/// TUN 健康：核心运行 + TUN 启用时才检查（§6.5 判据：/version 可通 + 网卡 UP + 规则 + 路由）。
async fn check_tun_health(core: &Mutex<CoreManager>, settings: &Mutex<SettingsStore>) -> bool {
    {
        let settings = settings.lock().await;
        if !settings.get().tun_enabled {
            return true; // TUN 未启用 → 健康（不检查）
        }
    }
    {
        let mut core = core.lock().await;
        if core.state() != "running" {
            return true; // 核心未运行 → TUN 不应存在（由核心崩溃 watchdog 处理）
        }
    }
    let tools = tun::Tools::system();
    let link_ok = tun::link_up(&tools, tun::TUN_DEVICE).await;
    let rule_ok = tun::rule_range_present(&tools).await;
    let route_ok = tun::table_has_route(&tools).await;
    link_ok && rule_ok && route_ok
}

/// fail-open（§6.5）：cleanup-tun 恢复直连 → 审计 `watchdog.failopen`（net 前后快照）→
/// `Degraded` 事件（TUI 红色提示）。任何一步失败继续，绝不 fail-closed。
async fn fail_open(audit: &Audit, events: &broadcast::Sender<Event>, reason: &str) {
    let op_id = audit.intent("watchdog.failopen", &Actor::system(), reason);
    let (clean, residuals) = tun::cleanup_tun(&tun::Tools::system()).await;
    let msg = residuals.join(", ");
    let (result, err): (&str, Option<&str>) = if clean {
        ("ok", None)
    } else {
        ("partial", Some(msg.as_str()))
    };
    audit.result("watchdog.failopen", &op_id, &Actor::system(), result, err, None);
    tracing::warn!("watchdog: fail-open ({reason}) clean={clean} residuals={residuals:?}");
    let _ = events.send(Event::Degraded);
    let _ = events.send(Event::CoreStatusChanged);
}

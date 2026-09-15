//! 配置拼装与应用（doc/01 §8）：统一入口 `regenerate`（R1–R7）+ 设置→托管字段映射。
//!
//! 职责：
//! - `options_from`：`clard.toml`（Settings）→ clard-config 托管选项（原 TUI `config_options` 迁入）；
//! - `regenerate(来源, ctx)`：读 current profile + settings → 拼装 → 校验 → 原子写 →
//!   热重载 + 回读校验（Runtime）→ 成功审计 / 失败回滚。Startup 上下文仅落盘（R5/R6 跳过）。
//!
//! 约定：调用方（daemon 单锁串行）传入 `&ProfilesStore`/`&Settings`/`&mut CoreManager`，
//! 本模块不自行加锁（§8.5 不引入多锁）；`core_running` 由调用方按核心状态传入。

use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use clard_config::config_gen::{self, ConfigGenOptions, TunOptions};
use clard_proto::{Event, Settings};

use crate::audit::{Actor, Audit};
use crate::core::CoreManager;
use crate::profiles::ProfilesStore;

/// 应用上下文（§8.2）：运行中变更（热重载+回读校验）／启动自愈（仅落盘）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    /// 核心运行中的变更（A/B/C/D）：走完 R1–R7。
    #[allow(dead_code)] // 阶段 3（入口收敛）接入 RPC 后启用
    Runtime,
    /// helper 启动自愈（E）：跳过 R5 热重载与 R6 回读校验（核心未起，无端可查）。
    Startup,
}

/// regenerate 结果。
#[derive(Debug)]
pub struct Regen {
    pub cfg_sha256: String,
    #[allow(dead_code)] // 阶段 3（入口收敛）接入 RPC 后消费
    pub hot_reloaded: bool,
}

/// 从系统设置构造拼装选项（§8.1/§8.4：TUI `config_options` 迁入 helper）。
/// 空列表字段 = 用 clard-config 默认值（与托管约定一致）。
pub fn options_from(settings: &Settings, log_level: &str) -> ConfigGenOptions {
    ConfigGenOptions {
        mixed_port: settings.mixed_port,
        log_level: if log_level.trim().is_empty() {
            "info".to_string()
        } else {
            log_level.trim().to_string()
        },
        tun: if settings.tun_enabled {
            let d = TunOptions::default();
            Some(TunOptions {
                stack: if settings.tun_stack.is_empty() {
                    d.stack
                } else {
                    settings.tun_stack.clone()
                },
                dns_hijack: if settings.dns_hijack.is_empty() {
                    d.dns_hijack
                } else {
                    settings.dns_hijack.clone()
                },
                dns_mode: if settings.tun_dns_mode.is_empty() {
                    d.dns_mode
                } else {
                    settings.tun_dns_mode.clone()
                },
                route_exclude_address: if settings.route_exclude_address.is_empty() {
                    d.route_exclude_address
                } else {
                    settings.route_exclude_address.clone()
                },
                exclude_uid: settings.exclude_uid.clone(),
                exclude_interface: settings.exclude_interface.clone(),
                exclude_dst_port: settings.exclude_dst_port.clone(),
                strict_route: settings.strict_route,
                auto_redirect: settings.auto_redirect,
                ..d
            })
        } else {
            None
        },
        ..ConfigGenOptions::default()
    }
}

/// 统一拼装应用（§8.2 R1–R7）。
///
/// - `core_running`：核心进程是否在运行（R5 热重载判据，由调用方按状态传入）；
/// - `ctx=Startup` 时无论 `core_running` 都跳过 R5/R6。
#[allow(clippy::too_many_arguments)] // 统一入口需全量输入（与 audit::write 同例）
pub async fn regenerate(
    store: &ProfilesStore,
    settings: &Settings,
    core: &mut CoreManager,
    core_running: bool,
    audit: &Audit,
    events: &broadcast::Sender<Event>,
    actor: &Actor,
    source: &str,
    ctx: Ctx,
) -> Result<Regen, String> {
    // R1 输入：current profile 原始 yaml + settings + log_level
    let uid = store
        .current()
        .map(|p| p.uid.clone())
        .ok_or_else(|| "无当前配置（尚未导入/切换）".to_string())?;
    let profile = store.content(&uid).map_err(|e| e.to_string())?;
    let log_level = crate::helper_config::global().log_level.clone();

    // R2 拼装（深合并 + 托管注入）
    let options = options_from(settings, &log_level);
    let runtime = config_gen::generate(&profile, None, &options)
        .map_err(|e| format!("配置拼装失败: {e}"))?;

    // R3 校验产物：关键固定标识存在（§8.2 R3，兜底）
    validate_runtime(&runtime, settings)?;

    // cfg_sha256（审计 R6.3）
    let mut h = Sha256::new();
    h.update(runtime.as_bytes());
    let cfg_sha256 = format!("{:x}", h.finalize());

    let op_id = audit.intent("config.apply", actor, source);

    // R4 原子写（tmp + rename；备份旧内容用于回滚）
    let path = core.runtime_config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let old = std::fs::read_to_string(&path).ok();
    write_yaml_atomic(&path, &runtime).map_err(|e| e.to_string())?;

    // R5 热重载 + R6 回读校验（仅 Runtime 且核心运行）
    let sock = crate::core::core_sock_path();
    let hot = matches!(ctx, Ctx::Runtime) && core_running;
    let apply = if hot {
        match crate::core::reload_config(&sock, &runtime).await {
            Ok(()) => verify_runtime(&sock, settings, &log_level).await,
            Err(e) => Err(e),
        }
    } else {
        Ok(())
    };

    match apply {
        Ok(()) => {
            audit.result("config.apply", &op_id, actor, "ok", None, Some(&cfg_sha256));
            let _ = events.send(Event::CoreStatusChanged);
            Ok(Regen {
                cfg_sha256,
                hot_reloaded: hot,
            })
        }
        Err(e) => {
            // R7 回滚：恢复旧 config.yaml + 热重载旧内容；设置由调用方负责不落盘（§7.2）
            if let Some(old) = old {
                let _ = write_yaml_atomic(&path, &old);
                if hot {
                    let _ = crate::core::reload_config(&sock, &old).await;
                }
            }
            audit.result("config.apply", &op_id, actor, "error", Some(&e), Some(&cfg_sha256));
            Err(format!("配置应用失败: {e}"))
        }
    }
}

/// R3 产物校验：托管固定标识（§6.2/§8.2）必须存在且为预期值。
fn validate_runtime(runtime: &str, settings: &Settings) -> Result<(), String> {
    use serde_yaml_ng::Value;
    let doc: Value =
        serde_yaml_ng::from_str(runtime).map_err(|e| format!("产物解析失败: {e}"))?;
    let m = doc
        .as_mapping()
        .ok_or_else(|| "产物根必须是 mapping".to_string())?;
    let get = |k: &str| m.get(Value::String(k.into()));
    if get("external-controller-unix").and_then(|v| v.as_str()) != Some("/run/clard/core.sock") {
        return Err("托管字段缺失: external-controller-unix".into());
    }
    if get("mixed-port").and_then(|v| v.as_i64()) != Some(i64::from(settings.mixed_port)) {
        return Err("托管字段缺失: mixed-port".into());
    }
    if settings.tun_enabled {
        let tun = get("tun")
            .and_then(|v| v.as_mapping())
            .ok_or_else(|| "TUN 开启但产物缺 tun 块".to_string())?;
        let tg = |k: &str| tun.get(Value::String(k.into()));
        if tg("device").and_then(|v| v.as_str()) != Some("clard0") {
            return Err("固定标识缺失: tun.device=clard0".into());
        }
        if tg("iproute2-table-index").and_then(|v| v.as_i64()) != Some(2023) {
            return Err("固定标识缺失: iproute2-table-index=2023".into());
        }
        if tg("iproute2-rule-index").and_then(|v| v.as_i64()) != Some(9100) {
            return Err("固定标识缺失: iproute2-rule-index=9100".into());
        }
    }
    Ok(())
}

/// R6 回读校验（仅 Runtime）：普通字段 GET /configs 比对 + TUN 网卡/规则双向校验。
async fn verify_runtime(sock: &std::path::Path, settings: &Settings, log_level: &str) -> Result<(), String> {
    let cfg = crate::core::get_configs(sock).await?;
    let mut errs = Vec::new();
    if cfg["mixed-port"].as_i64() != Some(i64::from(settings.mixed_port)) {
        errs.push(format!("mixed-port 不一致（期望 {}）", settings.mixed_port));
    }
    let want_lvl = if log_level.trim().is_empty() {
        "info"
    } else {
        log_level.trim()
    };
    if cfg["log-level"].as_str() != Some(want_lvl) {
        errs.push(format!("log-level 不一致（期望 {want_lvl}）"));
    }
    if cfg["tun"]["enable"].as_bool() != Some(settings.tun_enabled) {
        errs.push(format!("tun.enable 不一致（期望 {}）", settings.tun_enabled));
    }
    if !errs.is_empty() {
        return Err(format!("回读校验不通过: {}", errs.join(", ")));
    }
    // TUN 物理层校验：开启时确认网卡/规则已建立（mihomo 异步创建，轮询）；
    // 关闭时仅配置层（GET /configs tun.enable=false 已确认），物理层消失由
    // SetTun/cleanup 路径承担（避免 mihomo 异步消失窗口的误报）。
    if settings.tun_enabled {
        let tools = crate::tun::Tools::system();
        let mut ok = false;
        for _ in 0..20 {
            if crate::tun::link_up(&tools, crate::tun::TUN_DEVICE).await
                && crate::tun::rule_range_present(&tools).await
            {
                ok = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        if !ok {
            return Err("TUN 网卡/规则未按预期建立".into());
        }
    }
    Ok(())
}

/// 原子写（tmp + rename）：断电/崩溃任一瞬间磁盘上是完整旧或完整新文件。
fn write_yaml_atomic(path: &std::path::Path, yaml: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, yaml)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    #![allow(unsafe_code)] // 测试设置 env（edition 2024 set_var/remove_var 为 unsafe fn）
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// 串行化依赖环境变量（CLARD_CORE_SOCK / CLARD_LOG_DIR）的测试，避免并行互踩。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// edition 2024：env 可变 API 为 unsafe，集中封装（unsafe 仅限测试）。
    fn set_env(k: &str, v: impl AsRef<std::ffi::OsStr>) {
        unsafe { std::env::set_var(k, v) };
    }
    fn rm_env(k: &str) {
        unsafe { std::env::remove_var(k) };
    }

    fn tmp_state(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "clard-config-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_store(state: &std::path::Path) -> ProfilesStore {
        let mut store = ProfilesStore::open(state).unwrap();
        let yaml = "proxies:\n  - name: n1\n    type: socks5\n    server: 1.2.3.4\n    port: 1080\n";
        store
            .import("https://example.com/a", Some("p1"), 3600, yaml, None)
            .unwrap();
        let uid = store.list()[0].uid.clone();
        store.set_current(&uid).unwrap();
        store
    }

    fn audit_at() -> Audit {
        crate::helper_config::init(); // log_level 读取（OnceLock 幂等）
        Audit::open() // log_dir() 由 CLARD_LOG_DIR 控制，调用前设置
    }

    // ---- options_from ----

    #[test]
    fn options_from_tun_disabled_is_none() {
        let s = Settings::default(); // tun_enabled=false
        let o = options_from(&s, "");
        assert!(o.tun.is_none());
        assert_eq!(o.mixed_port, 7890);
        assert_eq!(o.log_level, "info");
    }

    #[test]
    fn options_from_tun_enabled_uses_defaults_for_empty_fields() {
        let mut s = Settings::default();
        s.tun_enabled = true;
        // 空字段 → 默认（gvisor / fake-ip / 私网排除段）
        let o = options_from(&s, "debug").unwrap_tun();
        assert_eq!(o.stack, "gvisor");
        assert_eq!(o.dns_mode, "fake-ip");
        assert_eq!(o.device, "clard0");
        assert_eq!(o.table_index, 2023);
        assert_eq!(o.rule_index, 9100);
        assert!(!o.route_exclude_address.is_empty());
        assert_eq!(options_from(&s, "debug").log_level(), "debug");
    }

    #[test]
    fn options_from_custom_fields_override() {
        let mut s = Settings::default();
        s.tun_enabled = true;
        s.tun_stack = "system".into();
        s.tun_dns_mode = "redir-host".into();
        s.route_exclude_address = vec!["10.0.0.0/8".into()];
        let o = options_from(&s, "").unwrap_tun();
        assert_eq!(o.stack, "system");
        assert_eq!(o.dns_mode, "redir-host");
        assert_eq!(o.route_exclude_address, vec!["10.0.0.0/8"]);
    }

    // ---- validate_runtime ----

    #[test]
    fn validate_accepts_generated_runtime() {
        let s = Settings::default();
        let runtime = config_gen::generate("proxies: []\n", None, &options_from(&s, "info")).unwrap();
        assert!(validate_runtime(&runtime, &s).is_ok());
    }

    #[test]
    fn validate_rejects_missing_managed_field() {
        let s = Settings::default();
        let bad = "mixed-port: 9999\nexternal-controller-unix: /tmp/x\n";
        assert!(validate_runtime(bad, &s).is_err());
    }

    // ---- regenerate ----

    #[tokio::test]
    async fn regenerate_startup_writes_runtime_config() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = tmp_state("startup");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let store = make_store(&state);
        let mut core = CoreManager::new(&state);
        let (tx, _rx) = broadcast::channel::<Event>(8);
        let audit = audit_at();

        let r = regenerate(
            &store,
            &Settings::default(),
            &mut core,
            false, // 核心未起
            &audit,
            &tx,
            &Actor::system(),
            "startup",
            Ctx::Startup,
        )
        .await
        .unwrap();

        assert!(!r.hot_reloaded);
        assert_eq!(r.cfg_sha256.len(), 64);
        let out = std::fs::read_to_string(state.join("runtime/config.yaml")).unwrap();
        assert!(out.contains("mixed-port: 7890"));
        assert!(out.contains("mode: rule"));
        assert!(out.contains("proxies:"));
        rm_env("CLARD_LOG_DIR");
    }

    #[tokio::test]
    async fn regenerate_startup_with_tun_enabled_injects_block() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = tmp_state("tun");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let store = make_store(&state);
        let mut core = CoreManager::new(&state);
        let (tx, _rx) = broadcast::channel::<Event>(8);
        let audit = audit_at();
        let mut settings = Settings::default();
        settings.tun_enabled = true;

        regenerate(
            &store,
            &settings,
            &mut core,
            false,
            &audit,
            &tx,
            &Actor::system(),
            "startup",
            Ctx::Startup,
        )
        .await
        .unwrap();

        let out = std::fs::read_to_string(state.join("runtime/config.yaml")).unwrap();
        assert!(out.contains("device: clard0"));
        assert!(out.contains("iproute2-table-index: 2023"));
        assert!(out.contains("iproute2-rule-index: 9100"));
        rm_env("CLARD_LOG_DIR");
    }

    #[tokio::test]
    async fn regenerate_no_current_errors() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = tmp_state("nocurrent");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let store = ProfilesStore::open(&state).unwrap(); // 空，无 current
        let mut core = CoreManager::new(&state);
        let (tx, _rx) = broadcast::channel::<Event>(8);
        let audit = audit_at();

        let err = regenerate(
            &store,
            &Settings::default(),
            &mut core,
            false,
            &audit,
            &tx,
            &Actor::system(),
            "startup",
            Ctx::Startup,
        )
        .await
        .unwrap_err();
        assert!(err.contains("无当前配置"));
        rm_env("CLARD_LOG_DIR");
    }

    #[tokio::test]
    async fn regenerate_runtime_reloads_and_verifies() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = tmp_state("runtime");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let sock = std::env::temp_dir().join(format!("clard-config-mock-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock);
        set_env("CLARD_CORE_SOCK", &sock);

        let store = make_store(&state);
        let mut core = CoreManager::new(&state);
        let (tx, _rx) = broadcast::channel::<Event>(8);
        let audit = audit_at();
        // settings 默认（tun 关，避免依赖真实网卡/规则；回读 GET /configs 必须与之匹配）
        let settings = Settings::default();

        // mock core.sock：GET /configs 返回与托管字段一致的 general；PUT /configs 204
        let sock2 = sock.clone();
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let srv = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = s.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&buf);
                let req_line = text.lines().next().unwrap_or_default();
                let method = req_line.split(' ').next().unwrap_or("");
                if method == "GET" {
                    let body = r#"{"mixed-port":7890,"log-level":"info","tun":{"enable":false}}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    s.write_all(resp.as_bytes()).await.unwrap();
                } else {
                    s.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .await
                        .unwrap();
                }
                let _ = s.shutdown().await;
            }
            drop(sock2);
        });

        let r = regenerate(
            &store,
            &settings,
            &mut core,
            true, // 核心运行
            &audit,
            &tx,
            &Actor::system(),
            "switch",
            Ctx::Runtime,
        )
        .await
        .unwrap();

        assert!(r.hot_reloaded);
        srv.await.unwrap();
        rm_env("CLARD_LOG_DIR");
        rm_env("CLARD_CORE_SOCK");
        let _ = std::fs::remove_file(&sock);
    }

    #[tokio::test]
    async fn regenerate_runtime_failure_rolls_back() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = tmp_state("rollback");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let sock = std::env::temp_dir().join(format!("clard-config-mock-fail-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock);
        set_env("CLARD_CORE_SOCK", &sock);

        let store = make_store(&state);
        let mut core = CoreManager::new(&state);
        let (tx, _rx) = broadcast::channel::<Event>(8);
        let audit = audit_at();
        let settings = Settings::default();
        // 预置一份"旧"运行态配置（将被回滚恢复）
        let old_yaml = "mixed-port: 8888\nmode: direct\nproxies: []\n";
        std::fs::create_dir_all(state.join("runtime")).unwrap();
        std::fs::write(state.join("runtime/config.yaml"), old_yaml).unwrap();

        // mock core.sock：PUT 始终 500（热重载失败 → 回滚旧内容并再次 PUT）
        let sock2 = sock.clone();
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let srv = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = s.read(&mut tmp).await.unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let body = "{\"error\":\"boom\"}";
                let resp = format!(
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(resp.as_bytes()).await.unwrap();
                let _ = s.shutdown().await;
            }
            drop(sock2);
        });

        let err = regenerate(
            &store,
            &settings,
            &mut core,
            true,
            &audit,
            &tx,
            &Actor::system(),
            "switch",
            Ctx::Runtime,
        )
        .await
        .unwrap_err();
        assert!(err.contains("配置应用失败") || err.contains("热重载"));

        // 回滚后磁盘恢复旧内容
        let out = std::fs::read_to_string(state.join("runtime/config.yaml")).unwrap();
        assert_eq!(out, old_yaml);

        srv.await.unwrap();
        rm_env("CLARD_LOG_DIR");
        rm_env("CLARD_CORE_SOCK");
        let _ = std::fs::remove_file(&sock);
    }

    // ---- 测试辅助 ----

    /// 测试便捷：取 TunOptions（测试预设 tun_enabled=true 后调用）。
    trait UnwrapTun {
        fn unwrap_tun(self) -> TunOptions;
        fn log_level(self) -> String;
    }
    impl UnwrapTun for ConfigGenOptions {
        fn unwrap_tun(self) -> TunOptions {
            self.tun.expect("tun 应开启")
        }
        fn log_level(self) -> String {
            self.log_level
        }
    }
}

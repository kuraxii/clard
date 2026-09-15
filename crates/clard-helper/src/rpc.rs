//! RPC 请求分发：`clard-proto` 契约 → helper 领域操作（doc/01 §5.6/§7）。
//!
//! 审计（R6.3）：每次操作 intent 先记（net_before 快照），执行后 result 再记
//! （net_after 快照 + result/err + cfg_sha256），同 op_id 配对。

use std::path::Path;

use clard_proto::{Event, ProfileItem, Request, Response, SettingsPatch};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::audit::{Actor, Audit};
use crate::config::Ctx;
use crate::core::CoreManager;
use crate::profiles::{ImportOutcome, ProfilesError, ProfilesStore};
use crate::settings::SettingsStore;

/// 处理一个请求。`actor` 来自 `SO_PEERCRED`，intent/result 双记录审计；
/// 状态变化类操作成功后经 `events` 广播事件（§5.6 Subscribe）。
/// 核心相关操作异步执行（spawn/就绪探测/热重载）。
pub async fn handle(
    req: Request,
    store: &mut ProfilesStore,
    settings: &mut SettingsStore,
    core: &mut CoreManager,
    audit: &Audit,
    events: &broadcast::Sender<Event>,
    actor: &Actor,
) -> Response {
    // 阶段一：intent（操作前，含 net_before 快照）
    let (op, intent) = op_and_intent(&req);
    let op_id = audit.intent(op, actor, intent);

    // 阶段二：执行
    let (op, resp, cfg_sha256) = match req {
        Request::Hello => ("rpc.hello", Response::Hello {
            helper_version: env!("CARGO_PKG_VERSION").to_string(),
            proto_version: clard_proto::PROTO_VERSION,
        }, None),
        Request::Status => {
            let tun_active = crate::tun::tun_active(&crate::tun::Tools::system()).await;
            let state = store.root();
            // 版本：运行中取 GET /version；未运行回退安装记录（R7.3 展示）
            let core_version = core
                .version()
                .map(str::to_string)
                .or_else(|| {
                    std::fs::read_to_string(crate::core::core_version_path(state))
                        .ok()
                        .map(|s| s.trim().to_string())
                });
            let core_sha256 = std::fs::read_to_string(crate::core::core_sha256_path(state))
                .ok()
                .map(|s| s.trim().to_string());
            (
                "rpc.status",
                Response::Status {
                    core_state: core.state().to_string(),
                    core_pid: core.pid(),
                    core_version,
                    tun_active,
                    core_sha256,
                },
                None,
            )
        }
        Request::StartCore => match core.start().await {
            Ok(()) => ("core.start", Response::Ok, None),
            Err(e) => ("core.start", Response::err(e), None),
        },
        Request::StopCore => match core.stop().await {
            Ok(()) => ("core.stop", Response::Ok, None),
            Err(e) => ("core.stop", Response::err(e), None),
        },
        Request::RestartCore => match core.restart().await {
            Ok(()) => ("core.restart", Response::Ok, None),
            Err(e) => ("core.restart", Response::err(e), None),
        },
        Request::ApplyConfig { yaml } => {
            // cfg_sha256（R6.3：字段级审计用）
            let mut h = Sha256::new();
            h.update(yaml.as_bytes());
            let cfg = Some(format!("{:x}", h.finalize()));
            match core.apply_config(&yaml).await {
                Ok(()) => ("config.apply", Response::Ok, cfg),
                Err(e) => ("config.apply", Response::err(e), cfg),
            }
        }
        Request::BackupCreate { name } => match crate::backup::create(
            &crate::backup::backup_dir(),
            store.root(),
            name.as_deref(),
        ) {
            Ok(item) => ("backup.create", Response::BackupCreated { item }, None),
            Err(e) => ("backup.create", Response::err(e.to_string()), None),
        },
        Request::BackupList => match crate::backup::list(&crate::backup::backup_dir()) {
            Ok(backups) => ("backup.list", Response::BackupList { backups }, None),
            Err(e) => ("backup.list", Response::err(e.to_string()), None),
        },
        Request::BackupDelete { name } => match crate::backup::delete(&crate::backup::backup_dir(), &name) {
            Ok(()) => ("backup.delete", Response::Ok, None),
            Err(e) => ("backup.delete", Response::err(e.to_string()), None),
        },
        Request::BackupRestore { name } => match crate::backup::restore(
            &crate::backup::backup_dir(),
            store.root(),
            &name,
        ) {
            Ok(()) => {
                // 恢复后重载内存中的 stores（避免索引/设置与磁盘不一致）
                let _ = store.reload();
                let _ = settings.reload();
                ("backup.restore", Response::Ok, None)
            }
            Err(e) => ("backup.restore", Response::err(e.to_string()), None),
        },
        Request::SettingsGet => {
            let settings = settings.get().clone();
            ("settings.get", Response::Settings { settings }, None)
        }
        Request::SettingsSet(patch) => {
            if patch_affects_config(&patch) {
                // §8.4 白名单字段：TUN 开启前置检查（能力/冲突/残留）→ 内存 apply →
                // regenerate（热重载+回读）→ 成功才持久化；失败恢复内存（§7.2 设置不静默变）。
                let core_running = core.state() == "running";
                let force = patch.force_tun == Some(true);
                let precheck = if patch.tun_enabled == Some(true) {
                    crate::tun::precheck_tun_enable(core_running, force, &crate::tun::Tools::system()).await
                } else {
                    Ok(Vec::new())
                };
                match precheck {
                    Err(e) => ("settings.set", Response::err(e), None),
                    // 其他 TUN 共存警告：不静默开，返回结构化冲突由 TUI 二次确认（§6.2）
                    Ok(warnings) if !warnings.is_empty() => {
                        ("settings.set", Response::TunConflict { devices: warnings }, None)
                    }
                    Ok(_) => {
                        let backup = settings.get().clone();
                        if let Err(e) = settings.apply_in_memory(&patch) {
                            ("settings.set", Response::err(e.to_string()), None)
                        } else {
                            let ctx = if core_running { Ctx::Runtime } else { Ctx::Startup };
                            match crate::config::regenerate(
                                store,
                                settings.get(),
                                core,
                                core_running,
                                audit,
                                events,
                                actor,
                                "settings",
                                ctx,
                            )
                            .await
                            {
                                Ok(_) => match settings.save() {
                                    Ok(()) => ("settings.set", Response::Ok, None),
                                    Err(e) => {
                                        settings.restore(backup);
                                        ("settings.set", Response::err(e.to_string()), None)
                                    }
                                },
                                Err(e) => {
                                    settings.restore(backup);
                                    ("settings.set", Response::err(e), None)
                                }
                            }
                        }
                    }
                }
            } else {
                match settings.patch(&patch) {
                    Ok(()) => ("settings.set", Response::Ok, None),
                    Err(e) => ("settings.set", Response::err(e.to_string()), None),
                }
            }
        }
        Request::ProfileList => {
            let items = store
                .list()
                .iter()
                .map(|p| ProfileItem {
                    uid: p.uid.clone(),
                    name: p.name.clone(),
                    url: p.url.clone(),
                    updated_at: p.updated_at,
                    interval: p.interval,
                    upload: p.upload,
                    download: p.download,
                    total: p.total,
                    expire: p.expire,
                    selected: p
                        .selected
                        .iter()
                        .map(|s| clard_proto::NodeSelection { group: s.group.clone(), node: s.node.clone() })
                        .collect(),
                })
                .collect();
            (
                "profile.list",
                Response::ProfileList {
                    current: store.current().map(|p| p.uid.clone()),
                    items,
                },
                None,
            )
        }
        Request::ProfileImport(import) => {
            // §8.3 A：订阅原始内容（raw）→ clard-config 转换 → 标准 yaml 落盘
            match clard_config::config_gen::subscription_to_yaml(&import.yaml) {
                Err(e) => ("profile.import", Response::err(format!("订阅解析失败: {e}")), None),
                Ok(converted) => {
                    // 命中 current 判定：同 URL 覆盖更新且目标即 current → 事务化（§8.3 A：
                    // regenerate 成功才正式替换原始层，失败恢复旧内容+索引，防污染）
                    let existing = store
                        .list()
                        .iter()
                        .find(|p| p.url == import.url)
                        .map(|p| p.uid.clone());
                    let hitting_current = existing.as_deref().is_some_and(|uid| {
                        store.current().is_some_and(|c| c.uid == *uid)
                    });
                    if hitting_current {
                        let idx_backup = store.index_snapshot();
                        let old_content = existing.as_deref().and_then(|uid| store.content(uid).ok());
                        match store.import(
                            &import.url,
                            import.name.as_deref(),
                            import.interval,
                            &converted,
                            import.info,
                        ) {
                            Ok(ImportOutcome::Updated { uid }) => {
                                let core_running = core.state() == "running";
                                let ctx = if core_running { Ctx::Runtime } else { Ctx::Startup };
                                match crate::config::regenerate(
                                    store,
                                    settings.get(),
                                    core,
                                    core_running,
                                    audit,
                                    events,
                                    actor,
                                    "import",
                                    ctx,
                                )
                                .await
                                {
                                    Ok(_) => (
                                        "profile.import",
                                        Response::ProfileImported { uid, updated: true },
                                        None,
                                    ),
                                    Err(e) => {
                                        if let Some(old) = old_content {
                                            let _ = store.set_content(&uid, &old);
                                        }
                                        let _ = store.restore_index(idx_backup);
                                        ("profile.import", Response::err(e), None)
                                    }
                                }
                            }
                            Ok(other) => {
                                let _ = store.restore_index(idx_backup);
                                ("profile.import", Response::err(format!("意外结果: {other:?}")), None)
                            }
                            Err(e) => ("profile.import", Response::err(e.to_string()), None),
                        }
                    } else {
                        match store.import(
                            &import.url,
                            import.name.as_deref(),
                            import.interval,
                            &converted,
                            import.info,
                        ) {
                            Ok(ImportOutcome::Created { uid }) => (
                                "profile.import",
                                Response::ProfileImported { uid, updated: false },
                                None,
                            ),
                            Ok(ImportOutcome::Updated { uid }) => (
                                "profile.import",
                                Response::ProfileImported { uid, updated: true },
                                None,
                            ),
                            Err(e) => ("profile.import", Response::err(e.to_string()), None),
                        }
                    }
                }
            }
        }
        Request::ProfileGet { uid } => match get_item_and_content(store, &uid) {
            Ok((item, yaml)) => (
                "profile.get",
                Response::ProfileContent { item, yaml },
                None,
            ),
            Err(e) => ("profile.get", Response::err(e.to_string()), None),
        },
        Request::ProfileRemove { uid } => match store.remove(&uid) {
            Ok(()) => ("profile.remove", Response::Ok, None),
            Err(e) => ("profile.remove", Response::err(e.to_string()), None),
        },
        Request::ProfileSetCurrent { uid } => {
            // §8.3 C：标记 current（事务）→ regenerate 拼装应用 → 失败回滚 current
            let previous = store.current().map(|p| p.uid.clone());
            match store.set_current(&uid) {
                Ok(()) => {
                    let core_running = core.state() == "running";
                    let ctx = if core_running { Ctx::Runtime } else { Ctx::Startup };
                    match crate::config::regenerate(
                        store,
                        settings.get(),
                        core,
                        core_running,
                        audit,
                        events,
                        actor,
                        "switch",
                        ctx,
                    )
                    .await
                    {
                        Ok(_) => ("profile.switch", Response::Ok, None),
                        Err(e) => {
                            if let Some(prev) = previous {
                                let _ = store.set_current(&prev);
                            }
                            ("profile.switch", Response::err(e), None)
                        }
                    }
                }
                Err(e) => ("profile.switch", Response::err(e.to_string()), None),
            }
        }
        Request::ProfileRename { uid, name } => match store.rename(&uid, &name) {
            Ok(()) => ("profile.rename", Response::Ok, None),
            Err(e) => ("profile.rename", Response::err(e.to_string()), None),
        },
        Request::ProfileMove { uid, up } => match store.move_item(&uid, up) {
            Ok(()) => ("profile.move", Response::Ok, None),
            Err(e) => ("profile.move", Response::err(e.to_string()), None),
        },
        Request::ProfileHistory { uid } => match store.history(&uid) {
            Ok(versions) => ("profile.history", Response::ProfileHistory { versions }, None),
            Err(e) => ("profile.history", Response::err(e.to_string()), None),
        },
        Request::ProfileRestore { uid, version } => match store.restore(&uid, version) {
            Ok(()) => ("profile.restore", Response::Ok, None),
            Err(e) => ("profile.restore", Response::err(e.to_string()), None),
        },
        Request::ProfileMemorize { group, node } => match store.memorize(&group, &node) {
            Ok(()) => ("profile.memorize", Response::Ok, None),
            Err(e) => ("profile.memorize", Response::err(e.to_string()), None),
        },
        Request::HelperConfigGet => {
            let cfg = crate::helper_config::global();
            (
                "helper.config",
                Response::HelperConfig {
                    config: clard_proto::HelperConfig {
                        log_level: cfg.log_level.clone(),
                        app_log_max_bytes: cfg.app_log_max_bytes,
                        app_log_keep: cfg.app_log_keep,
                        audit_keep: cfg.audit_keep,
                        audit_dual_write: cfg.audit_dual_write,
                    },
                },
                None,
            )
        }
        Request::LogSubmit { line } => match crate::logs::append_tui_log(&line) {
            Ok(()) => ("log.submit", Response::Ok, None),
            Err(e) => ("log.submit", Response::err(e.to_string()), None),
        },
        Request::LogTail { source, cursor } => {
            let src = match source.as_str() {
                "tui" => crate::logs::LogSource::Tui,
                _ => crate::logs::LogSource::Core,
            };
            match crate::logs::tail(src, cursor) {
                Ok((cursor, lines)) => ("log.tail", Response::LogTail { cursor, lines }, None),
                Err(e) => ("log.tail", Response::err(e.to_string()), None),
            }
        }
        Request::AuditQuery { cursor } => match crate::logs::audit_query(cursor) {
            Ok((cursor, records)) => ("audit.query", Response::AuditQuery { cursor, records }, None),
            Err(e) => ("audit.query", Response::err(e.to_string()), None),
        },
        Request::CleanupTun => {
            let (clean, residuals) = crate::tun::cleanup_tun(&crate::tun::Tools::system()).await;
            ("cleanup.tun", Response::CleanupResult { clean, residuals }, None)
        }
        Request::InstallCore {
            inbox_path,
            sha256,
            version,
        } => {
            let state = store.root().to_path_buf();
            let targets = crate::core::InstallTargets::production();
            match crate::core::install_core(&state, &targets, Path::new(&inbox_path), &sha256) {
                Ok(()) => {
                    // 记录版本（核心未运行时 Status 回退展示）
                    let _ = std::fs::write(crate::core::core_version_path(&state), version);
                    // 替换成功 → 重启核心（短暂中断，R7.3）
                    if core.state() == "running"
                        && let Err(e) = core.restart().await
                    {
                        (
                            "core.install",
                            Response::err(format!("二进制已更新，但重启核心失败: {e}")),
                            None,
                        )
                    } else {
                        ("core.install", Response::Ok, None)
                    }
                }
                Err(e) => ("core.install", Response::err(e), None),
            }
        }
        Request::UpdateGeoData {
            kind,
            inbox_path,
            sha256,
        } => {
            let geodata_dir = std::path::Path::new("/var/clard/lib/runtime");
            let inbox_root = std::path::Path::new("/run/clard/inbox");
            match crate::core::install_geodata(geodata_dir, inbox_root, kind, Path::new(&inbox_path), &sha256) {
                Ok(()) => {
                    // geo 数据在核心启动/重载时读入；更新后重启核心生效（与 InstallCore 一致）
                    if core.state() == "running"
                        && let Err(e) = core.restart().await
                    {
                        (
                            "geodata.update",
                            Response::err(format!("geo 数据已更新，但重启核心失败: {e}")),
                            None,
                        )
                    } else {
                        ("geodata.update", Response::Ok, None)
                    }
                }
                Err(e) => ("geodata.update", Response::err(e), None),
            }
        }
        other => {
            let op = "rpc.unimplemented";
            (op, Response::err(format!("方法未实现: {other:?}")), None)
        }
    };

    // 阶段三：result（操作后，含 net_after 快照 + err + cfg_sha256）
    let (result, err) = classify(&resp);
    audit.result(op, &op_id, actor, result, err, cfg_sha256.as_deref());

    // 状态变化事件推送（仅成功时；Degraded 由 watchdog 单独推）
    if result == "ok" {
        match op {
            "core.start" | "core.stop" | "core.restart" | "core.install" => {
                let _ = events.send(Event::CoreStatusChanged);
            }
            "tun.enable" | "cleanup.tun" => {
                let _ = events.send(Event::TunChanged);
            }
            _ => {}
        }
    }
    resp
}

/// 请求 → (op 名, 意图描述)（审计 intent 记录用）。
fn op_and_intent(req: &Request) -> (&'static str, &'static str) {
    match req {
        Request::Hello => ("rpc.hello", "hello handshake"),
        Request::Status => ("rpc.status", "query status"),
        Request::SettingsGet => ("settings.get", "read settings"),
        Request::SettingsSet(_) => ("settings.set", "update settings"),
        Request::ProfileList => ("profile.list", "list profiles"),
        Request::ProfileImport(_) => ("profile.import", "import profile"),
        Request::ProfileGet { .. } => ("profile.get", "fetch profile content"),
        Request::ProfileRemove { .. } => ("profile.remove", "remove profile"),
        Request::ProfileSetCurrent { .. } => ("profile.switch", "switch current profile"),
        Request::ProfileRename { .. } => ("profile.rename", "rename profile"),
        Request::ProfileMove { .. } => ("profile.move", "reorder profile"),
        Request::ProfileHistory { .. } => ("profile.history", "view profile history"),
        Request::ProfileRestore { .. } => ("profile.restore", "restore profile version"),
        Request::ProfileMemorize { .. } => ("profile.memorize", "memorize node selection"),
        Request::HelperConfigGet => ("helper.config", "read helper config"),
        Request::ApplyConfig { .. } => ("config.apply", "apply runtime config"),
        Request::BackupCreate { .. } => ("backup.create", "create backup"),
        Request::BackupList => ("backup.list", "list backups"),
        Request::BackupDelete { .. } => ("backup.delete", "delete backup"),
        Request::BackupRestore { .. } => ("backup.restore", "restore backup"),
        Request::StartCore => ("core.start", "start core"),
        Request::StopCore => ("core.stop", "stop core"),
        Request::RestartCore => ("core.restart", "restart core"),
        Request::CleanupTun => ("cleanup.tun", "cleanup TUN residuals"),
        Request::AuditQuery { .. } => ("audit.query", "query audit log"),
        Request::LogTail { .. } => ("log.tail", "tail log file"),
        Request::LogSubmit { .. } => ("log.submit", "submit app log line"),
        Request::Subscribe => ("rpc.subscribe", "subscribe to events"),
        Request::InstallCore { .. } => ("core.install", "install/upgrade core binary"),
        Request::UpdateGeoData { .. } => ("geodata.update", "update geo data (geoip/geosite)"),
        _ => ("rpc.unimplemented", "unimplemented request"),
    }
}

/// §8.4 设置白名单：影响 yaml 的字段（触发 regenerate）。非白名单（language/theme/
/// interval/test_url）仅落盘。
fn patch_affects_config(patch: &SettingsPatch) -> bool {
    patch.mixed_port.is_some()
        || patch.tun_enabled.is_some()
        || patch.tun_stack.is_some()
        || patch.tun_dns_mode.is_some()
        || patch.dns_hijack.is_some()
        || patch.route_exclude_address.is_some()
        || patch.exclude_uid.is_some()
        || patch.exclude_interface.is_some()
        || patch.exclude_dst_port.is_some()
        || patch.strict_route.is_some()
        || patch.auto_redirect.is_some()
}

/// 响应 → (result 分类, 错误信息)。
fn classify(resp: &Response) -> (&'static str, Option<&str>) {
    match resp {
        Response::Error { message } => ("error", Some(message)),
        Response::CleanupResult { clean: false, .. } => ("partial", None),
        _ => ("ok", None),
    }
}

fn get_item_and_content(
    store: &ProfilesStore,
    uid: &str,
) -> Result<(ProfileItem, String), ProfilesError> {
    let p = store
        .get(uid)
        .ok_or_else(|| ProfilesError::NotFound { uid: uid.into() })?;
    let item = ProfileItem {
        uid: p.uid.clone(),
        name: p.name.clone(),
        url: p.url.clone(),
        updated_at: p.updated_at,
        interval: p.interval,
        upload: p.upload,
        download: p.download,
        total: p.total,
        expire: p.expire,
        selected: p
            .selected
            .iter()
            .map(|s| clard_proto::NodeSelection { group: s.group.clone(), node: s.node.clone() })
            .collect(),
    };
    let yaml = store.content(uid)?;
    Ok((item, yaml))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clard_proto::ProfileImport;
    use crate::testutil::{env_guard, rm_env, set_env};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    const URL_A: &str = "https://example.com/a";
    const URL_B: &str = "https://example.com/b";
    const YAML_A: &str = "proxies:\n  - name: n1\n    type: socks5\n    server: 1.2.3.4\n    port: 1080\n";
    const YAML_B: &str = "proxies:\n  - name: n2\n    type: socks5\n    server: 9.9.9.9\n    port: 1080\n";

    fn tmp_state(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("clard-rpc-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// store：A（current）+ B 两条配置。
    fn make_store(state: &std::path::Path) -> ProfilesStore {
        let mut store = ProfilesStore::open(state).unwrap();
        store.import(URL_A, Some("p1"), 3600, YAML_A, None).unwrap();
        store.import(URL_B, Some("p2"), 3600, YAML_B, None).unwrap();
        let uid_a = store.list().iter().find(|p| p.url == URL_A).unwrap().uid.clone();
        store.set_current(&uid_a).unwrap();
        store
    }

    fn uid_of(store: &ProfilesStore, url: &str) -> String {
        store.list().iter().find(|p| p.url == url).unwrap().uid.clone()
    }

    /// mock core.sock：GET 返回 get_body，PUT 返回 put_status（"204" / "500"）。
    fn spawn_mock_sock(sock: &std::path::Path, get_body: &'static str, put_status: &'static str) -> tokio::task::JoinHandle<()> {
        let _ = std::fs::remove_file(sock);
        let listener = std::sync::Arc::new(tokio::net::UnixListener::bind(sock).unwrap());
        tokio::spawn(async move {
            loop {
                let (mut s, _) = match listener.accept().await {
                    Ok(x) => x,
                    Err(_) => break,
                };
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
                let method = text.lines().next().unwrap_or_default().split(' ').next().unwrap_or("");
                let resp = if method == "GET" {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{get_body}",
                        get_body.len()
                    )
                } else if put_status == "204" {
                    "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    format!(
                        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{{\"error\":\"boom\"}}",
                        16usize
                    )
                };
                s.write_all(resp.as_bytes()).await.unwrap();
                let _ = s.shutdown().await;
            }
        })
    }

    fn general_body(mixed_port: u16, tun: bool) -> String {
        format!(r#"{{"mixed-port":{mixed_port},"log-level":"info","tun":{{"enable":{tun}}}}}"#)
    }

    async fn dispatch(
        req: Request,
        state: &std::path::Path,
        store: &mut ProfilesStore,
        settings: &mut SettingsStore,
    ) -> Response {
        crate::helper_config::init(); // log_level 读取（OnceLock 幂等）
        let mut core = CoreManager::new(state);
        let audit = Audit::open();
        let (tx, _rx) = broadcast::channel::<Event>(8);
        handle(req, store, settings, &mut core, &audit, &tx, &Actor::system()).await
    }

    #[tokio::test]
    async fn switch_success_updates_current_and_applies() {
        let _g = env_guard();
        let state = tmp_state("switch-ok");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let sock = std::env::temp_dir().join(format!("clard-rpc-switch-ok-{}.sock", std::process::id()));
        set_env("CLARD_CORE_SOCK", &sock);
        let srv = spawn_mock_sock(&sock, Box::leak(general_body(7890, false).into_boxed_str()), "204");

        let mut store = make_store(&state);
        let mut settings = SettingsStore::open(&state).unwrap();
        let uid_b = uid_of(&store, URL_B);
        let resp = dispatch(Request::ProfileSetCurrent { uid: uid_b.clone() }, &state, &mut store, &mut settings).await;
        assert!(matches!(resp, Response::Ok), "{resp:?}");
        assert_eq!(store.current().unwrap().uid, uid_b);
        assert!(state.join("runtime/config.yaml").exists());

        srv.abort();
        rm_env("CLARD_LOG_DIR");
        rm_env("CLARD_CORE_SOCK");
    }

    #[tokio::test]
    async fn switch_failure_rolls_back_current() {
        let _g = env_guard();
        let state = tmp_state("switch-fail");
        set_env("CLARD_LOG_DIR", state.join("log"));

        let mut store = make_store(&state);
        let mut settings = SettingsStore::open(&state).unwrap();
        let uid_a = uid_of(&store, URL_A);
        let uid_b = uid_of(&store, URL_B);
        // 目标配置内容非法 → regenerate 拼装失败 → current 应回滚
        store.set_content(&uid_b, "").unwrap();
        let resp = dispatch(
            Request::ProfileSetCurrent { uid: uid_b },
            &state,
            &mut store,
            &mut settings,
        )
        .await;
        assert!(matches!(resp, Response::Error { .. }), "{resp:?}");
        assert_eq!(store.current().unwrap().uid, uid_a, "current 应回滚");
        rm_env("CLARD_LOG_DIR");
    }

    #[tokio::test]
    async fn settings_whitelist_persists_after_regen() {
        let _g = env_guard();
        let state = tmp_state("settings-ok");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let sock = std::env::temp_dir().join(format!("clard-rpc-settings-ok-{}.sock", std::process::id()));
        set_env("CLARD_CORE_SOCK", &sock);
        let srv = spawn_mock_sock(&sock, Box::leak(general_body(8080, false).into_boxed_str()), "204");

        let mut store = make_store(&state);
        let mut settings = SettingsStore::open(&state).unwrap();
        let patch = SettingsPatch { mixed_port: Some(8080), ..Default::default() };
        let resp = dispatch(Request::SettingsSet(patch), &state, &mut store, &mut settings).await;
        assert!(matches!(resp, Response::Ok), "{resp:?}");
        assert_eq!(settings.get().mixed_port, 8080, "内存已更新");
        let disk = std::fs::read_to_string(state.join("clard.toml")).unwrap();
        assert!(disk.contains("mixed_port = 8080"), "已持久化: {disk}");
        assert!(state.join("runtime/config.yaml").exists());

        srv.abort();
        rm_env("CLARD_LOG_DIR");
        rm_env("CLARD_CORE_SOCK");
    }

    #[tokio::test]
    async fn settings_whitelist_failure_restores_memory() {
        let _g = env_guard();
        let state = tmp_state("settings-fail");
        set_env("CLARD_LOG_DIR", state.join("log"));

        let mut store = make_store(&state);
        let mut settings = SettingsStore::open(&state).unwrap();
        // current 内容非法 → 白名单设置触发 regenerate 拼装失败 → 设置恢复内存（§7.2）
        let uid_a = uid_of(&store, URL_A);
        store.set_content(&uid_a, "").unwrap();
        let patch = SettingsPatch { mixed_port: Some(8080), ..Default::default() };
        let resp = dispatch(Request::SettingsSet(patch), &state, &mut store, &mut settings).await;
        assert!(matches!(resp, Response::Error { .. }), "{resp:?}");
        assert_eq!(settings.get().mixed_port, 7890, "失败恢复内存（§7.2）");
        let disk = std::fs::read_to_string(state.join("clard.toml")).unwrap_or_default();
        assert!(!disk.contains("mixed_port = 8080"), "磁盘未写入新值: {disk}");
        rm_env("CLARD_LOG_DIR");
    }

    #[tokio::test]
    async fn settings_non_whitelist_only_persists() {
        let _g = env_guard();
        let state = tmp_state("settings-non");
        set_env("CLARD_LOG_DIR", state.join("log"));
        // 不设 CLARD_CORE_SOCK：若误触发 regenerate 会连默认 /run/clard/core.sock（不存在→失败）
        let mut store = make_store(&state);
        let mut settings = SettingsStore::open(&state).unwrap();
        let patch = SettingsPatch { language: Some("zh".into()), ..Default::default() };
        let resp = dispatch(Request::SettingsSet(patch), &state, &mut store, &mut settings).await;
        assert!(matches!(resp, Response::Ok), "{resp:?}");
        assert_eq!(settings.get().language, "zh");
        rm_env("CLARD_LOG_DIR");
    }

    #[tokio::test]
    async fn import_current_success_updates_content() {
        let _g = env_guard();
        let state = tmp_state("import-ok");
        set_env("CLARD_LOG_DIR", state.join("log"));
        let sock = std::env::temp_dir().join(format!("clard-rpc-import-ok-{}.sock", std::process::id()));
        set_env("CLARD_CORE_SOCK", &sock);
        let srv = spawn_mock_sock(&sock, Box::leak(general_body(7890, false).into_boxed_str()), "204");

        let mut store = make_store(&state);
        let mut settings = SettingsStore::open(&state).unwrap();
        let uid_a = uid_of(&store, URL_A);
        // raw 订阅（节点列表）→ helper 转换 → 落盘
        let raw = "n2: socks5://9.9.9.9:1080";
        let resp = dispatch(
            Request::ProfileImport(ProfileImport {
                name: Some("p1".into()),
                url: URL_A.into(),
                interval: 3600,
                yaml: raw.into(),
                info: None,
            }),
            &state,
            &mut store,
            &mut settings,
        )
        .await;
        assert!(matches!(resp, Response::ProfileImported { updated: true, .. }), "{resp:?}");
        let content = store.content(&uid_a).unwrap();
        assert!(content.contains("9.9.9.9"), "内容已更新: {content}");

        srv.abort();
        rm_env("CLARD_LOG_DIR");
        rm_env("CLARD_CORE_SOCK");
    }
}


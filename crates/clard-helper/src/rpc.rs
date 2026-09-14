//! RPC 请求分发：`clard-proto` 契约 → helper 领域操作（doc/01 §5.6/§7）。
//!
//! 审计（R6.3）：每次操作 intent 先记（net_before 快照），执行后 result 再记
//! （net_after 快照 + result/err + cfg_sha256），同 op_id 配对。

use std::path::Path;

use clard_proto::{Event, ProfileItem, Request, Response};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::audit::{Actor, Audit};
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
        Request::SettingsSet(patch) => match settings.patch(&patch) {
            Ok(()) => ("settings.set", Response::Ok, None),
            Err(e) => ("settings.set", Response::err(e.to_string()), None),
        },
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
            match store.import(&import.url, import.name.as_deref(), import.interval, &import.yaml, import.info) {
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
        Request::ProfileSetCurrent { uid } => match store.set_current(&uid) {
            Ok(()) => ("profile.switch", Response::Ok, None),
            Err(e) => ("profile.switch", Response::err(e.to_string()), None),
        },
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
        Request::SetTun { enable } => {
            let s = settings.get().clone();
            let path = core.runtime_config_path();
            let sock = crate::core::core_sock_path();
            let running = core.state() == "running";
            match crate::tun::set_tun(
                enable,
                &s,
                &path,
                &sock,
                running,
                &crate::tun::Tools::system(),
            )
            .await
            {
                Ok(apply) => {
                    // 设置持久化（失败时 tun 块已回退，设置不落盘：§7.2 设置不被静默修改）
                    let patch = clard_proto::SettingsPatch {
                        tun_enabled: Some(enable),
                        ..Default::default()
                    };
                    match settings.patch(&patch) {
                        Ok(()) => (
                            "tun.enable",
                            Response::TunSet {
                                hot_reloaded: apply.hot_reloaded,
                                verified: apply.verified,
                            },
                            None,
                        ),
                        Err(e) => ("tun.enable", Response::err(e.to_string()), None),
                    }
                }
                Err(e) => ("tun.enable", Response::err(e), None),
            }
        }
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
        Request::ApplyConfig { .. } => ("config.apply", "apply runtime config"),
        Request::BackupCreate { .. } => ("backup.create", "create backup"),
        Request::BackupList => ("backup.list", "list backups"),
        Request::BackupDelete { .. } => ("backup.delete", "delete backup"),
        Request::BackupRestore { .. } => ("backup.restore", "restore backup"),
        Request::StartCore => ("core.start", "start core"),
        Request::StopCore => ("core.stop", "stop core"),
        Request::RestartCore => ("core.restart", "restart core"),
        Request::SetTun { enable } => (
            "tun.enable",
            if *enable { "enable TUN" } else { "disable TUN" },
        ),
        Request::CleanupTun => ("cleanup.tun", "cleanup TUN residuals"),
        Request::AuditQuery { .. } => ("audit.query", "query audit log"),
        Request::LogTail { .. } => ("log.tail", "tail log file"),
        Request::LogSubmit { .. } => ("log.submit", "submit app log line"),
        Request::Subscribe => ("rpc.subscribe", "subscribe to events"),
        Request::InstallCore { .. } => ("core.install", "install/upgrade core binary"),
        _ => ("rpc.unimplemented", "unimplemented request"),
    }
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

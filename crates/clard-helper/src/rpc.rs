//! RPC 请求分发：`clard-proto` 契约 → helper 领域操作（doc/01 §5.6/§7）。

use std::path::Path;

use clard_proto::{ProfileItem, Request, Response};

use crate::audit::{Actor, Audit};
use crate::core::CoreManager;
use crate::profiles::{ImportOutcome, ProfilesError, ProfilesStore};
use crate::settings::SettingsStore;

/// 处理一个请求。`actor` 来自 `SO_PEERCRED`，随结果写审计。
/// 核心相关操作异步执行（spawn/就绪探测/热重载）。
pub async fn handle(
    req: Request,
    store: &mut ProfilesStore,
    settings: &mut SettingsStore,
    core: &mut CoreManager,
    audit: &Audit,
    actor: &Actor,
) -> Response {
    let (op, resp) = match req {
        Request::Hello => ("rpc.hello", Response::Hello {
            helper_version: env!("CARGO_PKG_VERSION").to_string(),
            proto_version: clard_proto::PROTO_VERSION,
        }),
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
            )
        }
        Request::StartCore => match core.start().await {
            Ok(()) => ("core.start", Response::Ok),
            Err(e) => ("core.start", Response::err(e)),
        },
        Request::StopCore => match core.stop().await {
            Ok(()) => ("core.stop", Response::Ok),
            Err(e) => ("core.stop", Response::err(e)),
        },
        Request::RestartCore => match core.restart().await {
            Ok(()) => ("core.restart", Response::Ok),
            Err(e) => ("core.restart", Response::err(e)),
        },
        Request::ApplyConfig { yaml } => match core.apply_config(&yaml).await {
            Ok(()) => ("config.apply", Response::Ok),
            Err(e) => ("config.apply", Response::err(e)),
        },
        Request::BackupCreate { name } => match crate::backup::create(
            &crate::backup::backup_dir(),
            store.root(),
            name.as_deref(),
        ) {
            Ok(item) => ("backup.create", Response::BackupCreated { item }),
            Err(e) => ("backup.create", Response::err(e.to_string())),
        },
        Request::BackupList => match crate::backup::list(&crate::backup::backup_dir()) {
            Ok(backups) => ("backup.list", Response::BackupList { backups }),
            Err(e) => ("backup.list", Response::err(e.to_string())),
        },
        Request::BackupDelete { name } => match crate::backup::delete(&crate::backup::backup_dir(), &name) {
            Ok(()) => ("backup.delete", Response::Ok),
            Err(e) => ("backup.delete", Response::err(e.to_string())),
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
                ("backup.restore", Response::Ok)
            }
            Err(e) => ("backup.restore", Response::err(e.to_string())),
        },
        Request::SettingsGet => {
            let settings = settings.get().clone();
            ("settings.get", Response::Settings { settings })
        }
        Request::SettingsSet(patch) => match settings.patch(&patch) {
            Ok(()) => ("settings.set", Response::Ok),
            Err(e) => ("settings.set", Response::err(e.to_string())),
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
                })
                .collect();
            (
                "profile.list",
                Response::ProfileList {
                    current: store.current().map(|p| p.uid.clone()),
                    items,
                },
            )
        }
        Request::ProfileImport(import) => {
            match store.import(&import.url, import.name.as_deref(), import.interval, &import.yaml, import.info) {
                Ok(ImportOutcome::Created { uid }) => (
                    "profile.import",
                    Response::ProfileImported { uid, updated: false },
                ),
                Ok(ImportOutcome::Updated { uid }) => (
                    "profile.import",
                    Response::ProfileImported { uid, updated: true },
                ),
                Err(e) => ("profile.import", Response::err(e.to_string())),
            }
        }
        Request::ProfileGet { uid } => match get_item_and_content(store, &uid) {
            Ok((item, yaml)) => (
                "profile.get",
                Response::ProfileContent { item, yaml },
            ),
            Err(e) => ("profile.get", Response::err(e.to_string())),
        },
        Request::ProfileRemove { uid } => match store.remove(&uid) {
            Ok(()) => ("profile.remove", Response::Ok),
            Err(e) => ("profile.remove", Response::err(e.to_string())),
        },
        Request::ProfileSetCurrent { uid } => match store.set_current(&uid) {
            Ok(()) => ("profile.switch", Response::Ok),
            Err(e) => ("profile.switch", Response::err(e.to_string())),
        },
        Request::ProfileRename { uid, name } => match store.rename(&uid, &name) {
            Ok(()) => ("profile.rename", Response::Ok),
            Err(e) => ("profile.rename", Response::err(e.to_string())),
        },
        Request::ProfileMove { uid, up } => match store.move_item(&uid, up) {
            Ok(()) => ("profile.move", Response::Ok),
            Err(e) => ("profile.move", Response::err(e.to_string())),
        },
        Request::ProfileHistory { uid } => match store.history(&uid) {
            Ok(versions) => ("profile.history", Response::ProfileHistory { versions }),
            Err(e) => ("profile.history", Response::err(e.to_string())),
        },
        Request::ProfileRestore { uid, version } => match store.restore(&uid, version) {
            Ok(()) => ("profile.restore", Response::Ok),
            Err(e) => ("profile.restore", Response::err(e.to_string())),
        },
        Request::LogSubmit { line } => match crate::logs::append_tui_log(&line) {
            Ok(()) => ("log.submit", Response::Ok),
            Err(e) => ("log.submit", Response::err(e.to_string())),
        },
        Request::LogTail { source, cursor } => {
            let src = match source.as_str() {
                "tui" => crate::logs::LogSource::Tui,
                _ => crate::logs::LogSource::Core,
            };
            match crate::logs::tail(src, cursor) {
                Ok((cursor, lines)) => ("log.tail", Response::LogTail { cursor, lines }),
                Err(e) => ("log.tail", Response::err(e.to_string())),
            }
        }
        Request::AuditQuery { cursor } => match crate::logs::audit_query(cursor) {
            Ok((cursor, records)) => ("audit.query", Response::AuditQuery { cursor, records }),
            Err(e) => ("audit.query", Response::err(e.to_string())),
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
                        ),
                        Err(e) => ("tun.enable", Response::err(e.to_string())),
                    }
                }
                Err(e) => ("tun.enable", Response::err(e)),
            }
        }
        Request::CleanupTun => {
            let (clean, residuals) = crate::tun::cleanup_tun(&crate::tun::Tools::system()).await;
            ("cleanup.tun", Response::CleanupResult { clean, residuals })
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
                        )
                    } else {
                        ("core.install", Response::Ok)
                    }
                }
                Err(e) => ("core.install", Response::err(e)),
            }
        }
        other => {
            let op = "rpc.unimplemented";
            (op, Response::err(format!("方法未实现: {other:?}")))
        }
    };
    let result = match &resp {
        Response::Error { .. } => "error",
        Response::CleanupResult { clean, .. } if !*clean => "partial",
        _ => "ok",
    };
    audit.record(op, actor, result);
    resp
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
    };
    let yaml = store.content(uid)?;
    Ok((item, yaml))
}

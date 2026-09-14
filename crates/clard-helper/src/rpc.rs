//! RPC 请求分发：`clard-proto` 契约 → helper 领域操作（doc/01 §5.6/§7）。

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
        Request::Status => (
            "rpc.status",
            Response::Status {
                core_state: core.state().to_string(),
                core_pid: core.pid(),
                core_version: core.version().map(str::to_string),
                tun_active: false,
            },
        ),
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
        other => {
            let op = "rpc.unimplemented";
            (op, Response::err(format!("方法未实现: {other:?}")))
        }
    };
    let result = if matches!(resp, Response::Error { .. }) { "error" } else { "ok" };
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

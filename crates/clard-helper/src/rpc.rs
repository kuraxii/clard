//! RPC 请求分发：`clard-proto` 契约 → helper 领域操作（doc/01 §5.6/§7）。

use clard_proto::{ProfileItem, Request, Response};

use crate::audit::{Actor, Audit};
use crate::profiles::{ImportOutcome, ProfilesError, ProfilesStore};

/// 处理一个请求。`actor` 来自 `SO_PEERCRED`，随结果写审计。
pub fn handle(
    req: Request,
    store: &mut ProfilesStore,
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
                core_state: "not-managed-yet".into(),
                tun_active: false,
            },
        ),
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

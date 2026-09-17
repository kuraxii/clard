//! daemon 主体：flock 单实例 → unix socket（0666 系统级）→ 逐连接处理请求。
//!
//! 访问控制（doc/01 §4.2）：默认全开放，`SO_PEERCRED` 只记录 actor 供审计。
//! 帧格式：u32 BE 长度前缀 + JSON（doc/01 §11）。
//! 并发：所有状态变更经同一把 `Mutex` 串行（doc/01 §9 单写者）。

use std::{io, path::PathBuf, sync::Arc};

use clard_proto::{Request, Response};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Mutex,
};

use crate::audit::{Actor, Audit};
use crate::autoupdate;
use crate::core::CoreManager;
use crate::profiles::{ProfilesStore, state_dir};
use crate::rpc;
use crate::settings::SettingsStore;

/// RPC socket 路径：`CLARD_SOCKET` 覆盖，默认 `/run/clard/helper.sock`。
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLARD_SOCKET") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/run/clard/helper.sock")
}

/// inbox 中转目录（TUI → helper 的资产中转，0733+sticky；`CLARD_INBOX_DIR` 覆盖用于测试）。
/// doc/01 §4/§5.5：TUI 写普通文件，helper `O_NOFOLLOW` 复核哈希后 copy（不信任 rename）。
pub fn inbox_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLARD_INBOX_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from("/run/clard/inbox")
}

/// 常驻运行：flock → 建目录 → 绑定 socket（0666）→ accept 循环。
pub async fn run() -> io::Result<()> {
    let state = state_dir();
    let sock = socket_path();
    // helper 系统配置（/etc/clard/helper.toml；日志轮转/双写/核心日志级别，R7.5）
    crate::helper_config::init();

    // 单实例（flock 非阻塞排它锁；锁随 run() 栈上句柄存活到进程退出）
    let dir = sock.parent().unwrap_or(std::path::Path::new("/run/clard"));
    std::fs::create_dir_all(dir)?;
    // 单实例（flock 非阻塞排它锁；`_lock` 存活到 run() 返回，daemon 常驻即持有锁）
    let dir = sock.parent().unwrap_or(std::path::Path::new("/run/clard"));
    std::fs::create_dir_all(dir)?;
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("helper.lock"))?;
    let _lock = match nix::fcntl::Flock::lock(lock_file, nix::fcntl::FlockArg::LockExclusiveNonblock) {
        Ok(flock) => flock,
        Err((_, nix::errno::Errno::EWOULDBLOCK)) => {
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "已有实例在运行"));
        }
        Err((_, e)) => return Err(io::Error::other(e.to_string())),
    };

    std::fs::create_dir_all(&state)?;
    // inbox 中转目录（0733 + sticky，TUI 普通用户可写；doc/01 §4/§5.5）
    std::fs::create_dir_all(inbox_dir())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(inbox_dir(), std::fs::Permissions::from_mode(0o1733))?;
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock)?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o666))?;
    tracing::info!("clard-helper 就绪: socket={} state={}", sock.display(), state.display());

    let audit = Arc::new(Audit::open());
    let store = Arc::new(Mutex::new(
        ProfilesStore::open(&state).map_err(|e| io::Error::other(format!("打开配置索引失败: {e}")))?,
    ));
    let settings = Arc::new(Mutex::new(
        SettingsStore::open(&state).map_err(|e| io::Error::other(format!("打开设置失败: {e}")))?,
    ));
    let core = Arc::new(Mutex::new(CoreManager::new(&state)));
    // 事件广播（§5.6 Subscribe：状态变化/Degraded 推给订阅连接）
    let (events_tx, _) = tokio::sync::broadcast::channel::<clard_proto::Event>(64);

    // §5.3 启动自检：TUN 残留扫描 → 有残留即 cleanup-tun（fail-open，幂等）
    {
        let audit = audit.clone();
        let core = core.clone();
        let store = store.clone();
        let settings = settings.clone();
        let events = events_tx.clone();
        tokio::spawn(async move {
            // §6.3 geo 数据检查：随 RPM 分发到核心 -d 目录（/var/clard/lib/runtime），缺失时
            // 告警——GEOIP/geosite 规则会失效，且 mihomo 自更新已关闭（防 github 被墙卡启动）
            for name in ["geoip.metadb", "geosite.dat"] {
                let p = std::path::Path::new("/var/clard/lib/runtime").join(name);
                if !p.exists() {
                    tracing::warn!(
                        "启动自检：geo 数据缺失 {}（GEOIP/geosite 规则不可用，可在 TUI Core 页更新）",
                        p.display()
                    );
                }
            }
            // §5.2 启动自检：TUN 残留清理（fail-open，幂等）
            let (clean, residuals) = crate::tun::cleanup_tun(&crate::tun::Tools::system()).await;
            if !clean {
                let msg = residuals.join(", ");
                let op_id = audit.intent("cleanup.tun", &Actor::system(), "startup residual cleanup");
                audit.result("cleanup.tun", &op_id, &Actor::system(), "partial", Some(&msg), None);
                tracing::warn!("启动自检：TUN 残留已清理，仍有残余: {msg}");
            }
            // §8.3 E 启动自愈：按 current + settings 重新拼装运行态配置（R1–R4，Startup
            // 上下文跳过热重载/回读；核心尚未启动）。无当前配置/拼装失败不阻断启动。
            {
                let store = store.lock().await;
                let settings = settings.lock().await;
                let mut core = core.lock().await;
                match crate::config::regenerate(
                    &store,
                    settings.get(),
                    &mut core,
                    false,
                    &audit,
                    &events,
                    &Actor::system(),
                    "startup",
                    crate::config::Ctx::Startup,
                )
                .await
                {
                    Ok(r) => {
                        tracing::info!("启动自检：运行态配置已生成 (sha256={})", r.cfg_sha256);
                    }
                    Err(e) => {
                        tracing::warn!("启动自检：运行态配置跳过（无当前配置或拼装失败）: {e}");
                    }
                }
            }
            // §5.2 生命周期：helper 起 → 核心自动拉起（运行态配置存在时，与用户
            // 之前开启的核心状态一致）。失败不阻塞（全新安装未应用配置/核心二进制
            // 异常时仅告警，等 TUI 指令）；watchdog 随后接管崩溃退避重启。
            let mut core = core.lock().await;
            match core.start().await {
                Ok(()) => {
                    tracing::info!("启动自检：核心已自动拉起");
                    let _ = events.send(clard_proto::Event::CoreStatusChanged);
                }
                Err(e) => {
                    tracing::warn!("启动自检：自动拉起核心失败（运行态配置缺失或核心异常）: {e}");
                }
            }
        });
    }

    // 订阅自动更新定时器（R2.8；0=关，见 clard.toml）
    autoupdate::spawn(
        store.clone(),
        settings.clone(),
        core.clone(),
        audit.clone(),
        events_tx.clone(),
    );

    // §5.4/§6.5 watchdog：核心崩溃退避重启 + TUN 健康 fail-open
    crate::watchdog::spawn(settings.clone(), core.clone(), audit.clone(), events_tx.clone());

    // §8.4 F：helper.toml 变更感知（30s 轮询 mtime）→ reload → 核心 log-level 字段级 PATCH 热更
    {
        let core = core.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                if !crate::helper_config::reload_if_changed() {
                    continue;
                }
                let level = crate::helper_config::global().log_level;
                let mut core = core.lock().await;
                if core.state() == "running" {
                    let sock = crate::core::core_sock_path();
                    match crate::core::patch_log_level(&sock, &level).await {
                        Ok(()) => tracing::info!("helper.toml 变更：核心 log-level 已热更为 {level}"),
                        Err(e) => tracing::warn!("log-level 热更失败: {e}"),
                    }
                }
            }
        });
    }

    // §5.2 优雅退出：SIGTERM → 停核心 + cleanup-tun（fail-open）→ 退出。
    // 否则 helper 被 systemctl restart / kill -TERM 时 mihomo 子进程会成孤儿。
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(io::Error::other)?;

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("收到 SIGTERM，优雅退出：停核心 + cleanup-tun");
                let mut core = core.lock().await;
                if let Err(e) = core.stop().await {
                    tracing::warn!("退出时停核心失败: {e}");
                }
                let (clean, residuals) = crate::tun::cleanup_tun(&crate::tun::Tools::system()).await;
                if !clean {
                    tracing::warn!("退出时 TUN 残留: {}", residuals.join(", "));
                }
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let actor = peer_cred(&stream);
                let audit = audit.clone();
                let store = store.clone();
                let settings = settings.clone();
                let core = core.clone();
                let events = events_tx.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_connection(stream, &store, &settings, &core, &audit, &events, actor).await {
                        tracing::warn!("连接处理失败: {e}");
                    }
                });
            }
        }
    }
}

/// 单连接：循环读请求直到 EOF。锁改为**每请求临时获取**——
/// Subscribe 长连接不持有锁，避免阻塞其他连接（§9 单写者仍满足）。
async fn serve_connection(
    mut stream: UnixStream,
    store: &Mutex<ProfilesStore>,
    settings: &Mutex<SettingsStore>,
    core: &Mutex<CoreManager>,
    audit: &Audit,
    events: &tokio::sync::broadcast::Sender<clard_proto::Event>,
    actor: Actor,
) -> io::Result<()> {
    loop {
        let Some(req) = read_frame(&mut stream).await? else {
            return Ok(()); // EOF
        };
        // Subscribe：切换为事件长连接（TUI 侧先 Status 全量同步再订阅，doc/01 §5.6）
        if matches!(req, Request::Subscribe) {
            return subscribe_loop(stream, events).await;
        }
        let mut store = store.lock().await;
        let mut settings = settings.lock().await;
        let mut core = core.lock().await;
        let resp = rpc::handle(req, &mut store, &mut settings, &mut core, audit, events, &actor).await;
        drop(store);
        drop(settings);
        drop(core);
        write_frame(&mut stream, &resp).await?;
    }
}

/// 订阅循环：把 helper 全局事件广播转发给对端，直到对端断开。
async fn subscribe_loop(
    mut stream: UnixStream,
    events: &tokio::sync::broadcast::Sender<clard_proto::Event>,
) -> io::Result<()> {
    let mut rx = events.subscribe();
    loop {
        tokio::select! {
            // 对端关闭/发数据检测（Subscribe 期间对端不应再发请求）
            r = read_frame(&mut stream) => {
                match r {
                    Ok(None) | Err(_) => return Ok(()),
                    Ok(Some(_)) => continue,
                }
            }
            e = rx.recv() => {
                match e {
                    Ok(ev) => write_event(&mut stream, &ev).await?,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return Ok(()),
                }
            }
        }
    }
}

async fn write_event(stream: &mut UnixStream, ev: &clard_proto::Event) -> io::Result<()> {
    let buf = serde_json::to_vec(ev).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    stream.write_all(&(buf.len() as u32).to_be_bytes()).await?;
    stream.write_all(&buf).await?;
    Ok(())
}

/// 读一帧：u32 BE 长度 + JSON；EOF 返回 None。
async fn read_frame(stream: &mut UnixStream) -> io::Result<Option<Request>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "帧过大"));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let req = serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(Some(req))
}

async fn write_frame(stream: &mut UnixStream, resp: &Response) -> io::Result<()> {
    let buf = serde_json::to_vec(resp).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    stream.write_all(&(buf.len() as u32).to_be_bytes()).await?;
    stream.write_all(&buf).await?;
    Ok(())
}

/// `SO_PEERCRED`：记录连接者 uid/pid（只记录、不拒绝，doc/01 §4.2）。
fn peer_cred(stream: &UnixStream) -> Actor {
    use nix::sys::socket::{getsockopt, sockopt};
    match getsockopt(stream, sockopt::PeerCredentials) {
        Ok(cred) => Actor {
            uid: cred.uid(),
            pid: cred.pid(),
        },
        Err(_) => Actor { uid: u32::MAX, pid: -1 },
    }
}

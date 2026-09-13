//! daemon 主体：flock 单实例 → unix socket（0666 系统级）→ 逐连接处理请求。
//!
//! 访问控制（doc/01 §4.2）：默认全开放，`SO_PEERCRED` 只记录 actor 供审计。
//! 帧格式：u32 BE 长度前缀 + JSON（doc/01 §11）。
//! 并发：所有状态变更经同一把 `Mutex` 串行（doc/01 §9 单写者）。

use std::{
    io,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::Arc,
};

use clard_proto::{Request, Response};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Mutex,
};

use crate::audit::{Actor, Audit};
use crate::profiles::{ProfilesStore, state_dir};
use crate::rpc;

/// RPC socket 路径：`CLARD_SOCKET` 覆盖，默认 `/run/clard/helper.sock`。
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLARD_SOCKET") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/run/clard/helper.sock")
}

/// 常驻运行：flock → 建目录 → 绑定 socket（0666）→ accept 循环。
pub async fn run() -> io::Result<()> {
    let state = state_dir();
    let sock = socket_path();

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
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock)?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o666))?;
    tracing::info!("clard-helper 就绪: socket={} state={}", sock.display(), state.display());

    let audit = Arc::new(Audit::open());
    let store = Arc::new(Mutex::new(
        ProfilesStore::open(&state).map_err(|e| {
            io::Error::other(format!("打开配置索引失败: {e}"))
        })?,
    ));

    loop {
        let (stream, _) = listener.accept().await?;
        let actor = peer_cred(&stream);
        let audit = audit.clone();
        let store = store.clone();
        tokio::spawn(async move {
            let mut store = store.lock().await;
            if let Err(e) = serve_connection(stream, &mut store, &audit, actor).await {
                tracing::warn!("连接处理失败: {e}");
            }
        });
    }
}

/// 单连接：循环读请求直到 EOF（CLI 一次一个请求后关闭）。
async fn serve_connection(
    mut stream: UnixStream,
    store: &mut ProfilesStore,
    audit: &Audit,
    actor: Actor,
) -> io::Result<()> {
    loop {
        let Some(req) = read_frame(&mut stream).await? else {
            return Ok(()); // EOF
        };
        let resp = rpc::handle(req, store, audit, &actor);
        write_frame(&mut stream, &resp).await?;
    }
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
    let req = serde_json::from_slice(&buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(Some(req))
}

async fn write_frame(stream: &mut UnixStream, resp: &Response) -> io::Result<()> {
    let buf = serde_json::to_vec(resp)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
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

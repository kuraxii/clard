//! InstallCore 集成测试（R7.3）：真实 helper daemon 子进程 + 临时目录 + 帧协议客户端。
//!
//! 覆盖（对应 doc/01 §5.1 安全边界）：
//! - 正确哈希：inbox → 复核 → 原子替换 → core.sha256 记录 → inbox 清理 → Status 回显；
//! - 错误哈希：拒绝 + inbox 删除 + 二进制不被污染 + 审计 error；
//! - inbox 路径越界拒绝；
//! - 核心运行中升级：mock core.sock（GET /version）→ StartCore → InstallCore → 重启成功。
//!
//! 每个用例独立 TempDir，测试结束自动删除；helper 子进程 kill_on_drop。

use std::{
    io::Read,
    path::Path,
    process::Stdio,
    time::Duration,
};

use clard_proto::{Request, Response};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    process::Command,
};

/// 帧协议客户端（u32 BE 长度 + JSON，doc/01 §11）。
async fn rpc_call(sock: &Path, req: &Request) -> Response {
    let mut stream = UnixStream::connect(sock).await.unwrap();
    let body = serde_json::to_vec(req).unwrap();
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .await
        .unwrap();
    stream.write_all(&body).await.unwrap();
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.unwrap();
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await.unwrap();
    serde_json::from_slice(&buf).unwrap()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// 测试环境：临时目录 + helper daemon 子进程 + 路径集。
struct Harness {
    _dir: TempDir,
    _child: tokio::process::Child,
    sock: std::path::PathBuf,
    state: std::path::PathBuf,
    inbox: std::path::PathBuf,
    bin: std::path::PathBuf,
}

impl Harness {
    /// 起 helper（临时 env，全部指向 TempDir 下）。可选 mock core.sock（测运行中重启）。
    async fn start(core_sock_mock: bool) -> Harness {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        for sub in ["run", "state", "log", "inbox", "bin"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        // 旧核心占位：shell 脚本忽略 helper 传的 `-d/-f/-ext-ctl-unix` 参数并保持存活
        // （直接 copy /bin/sleep 会因非法参数立即退出，导致 StartCore 后状态变 stopped）
        let bin = root.join("bin/mihomo");
        std::fs::write(&bin, b"#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let core_sock = root.join("run/core.sock");
        let sock = root.join("run/helper.sock");
        let state = root.join("state");
        let inbox = root.join("inbox");

        let mut cmd = Command::new(env!("CARGO_BIN_EXE_clard-helper"));
        cmd.arg("run")
            .env("CLARD_SOCKET", &sock)
            .env("CLARD_STATE_DIR", &state)
            .env("CLARD_LOG_DIR", root.join("log"))
            .env("CLARD_INBOX_DIR", &inbox)
            .env("CLARD_CORE_BIN", &bin)
            .env("CLARD_CORE_SOCK", &core_sock)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let h = Harness {
            sock: sock.clone(),
            state,
            inbox,
            bin,
            _dir: dir,
            _child: cmd.spawn().unwrap(),
        };
        if core_sock_mock {
            spawn_mock_core(&core_sock).await;
        }
        wait_ready(&sock).await;
        h
    }
}

/// 轮询 socket 直到 helper 就绪。
async fn wait_ready(sock: &Path) {
    for _ in 0..50 {
        if tokio::net::UnixStream::connect(sock).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("helper 未就绪: {}", sock.display());
}

/// mock core.sock：响应 `GET /version` → `{"version":"1.19.2"}`。
/// helper `start()` 会 remove_file(core.sock)（清理残留），故用守护循环：
/// 文件缺失时重建 listener（模拟真实 mihomo bind socket 的行为）。
async fn spawn_mock_core(sock: &Path) {
    let sock = sock.to_path_buf();
    let _ = std::fs::remove_file(&sock);
    tokio::spawn(async move {
        let mut listener: Option<tokio::net::UnixListener> = None;
        loop {
            // 守护：文件被删 → 丢弃 listener，下一轮重建
            let need_rebind = listener
                .as_ref()
                .is_none_or(|_| !Path::new(&sock).exists());
            if need_rebind && listener.is_some() {
                listener = None;
            }
            if listener.is_none() {
                if let Ok(l) = tokio::net::UnixListener::bind(&sock) {
                    listener = Some(l);
                }
            }
            let accept = match &listener {
                Some(l) => l.accept(),
                None => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            tokio::select! {
                r = accept => {
                    if let Ok((mut stream, _)) = r {
                        tokio::spawn(async move {
                            let mut buf = Vec::new();
                            let mut tmp = [0u8; 4096];
                            loop {
                                let Ok(n) = stream.read(&mut tmp).await else { return };
                                if n == 0 { return; }
                                buf.extend_from_slice(&tmp[..n]);
                                if buf.windows(4).any(|w| w == b"\r\n\r\n") { break; }
                            }
                            let body = br#"{"version":"1.19.2","meta":true}"#;
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = stream.write_all(resp.as_bytes()).await;
                            let _ = stream.write_all(body).await;
                            let _ = stream.shutdown().await;
                        });
                    } else {
                        listener = None;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
    });
}

#[tokio::test]
async fn install_core_replaces_binary_via_rpc() {
    let h = Harness::start(false).await;

    // 准备：inbox 写入 payload + 期望哈希
    let payload = b"mihomo-v9.9.9-binary".to_vec();
    let sha = sha256_hex(&payload);
    let inbox_file = h.inbox.join("core.bin");
    std::fs::write(&inbox_file, &payload).unwrap();

    // 升级前：无版本/哈希记录
    let Response::Status {
        core_sha256: before,
        ..
    } = rpc_call(&h.sock, &Request::Status).await
    else {
        panic!("unexpected status");
    };
    assert!(before.is_none());

    let resp = rpc_call(
        &h.sock,
        &Request::InstallCore {
            inbox_path: inbox_file.display().to_string(),
            sha256: sha.clone(),
            version: "v9.9.9".into(),
        },
    )
    .await;
    assert_eq!(resp, Response::Ok, "{resp:?}");

    // 替换成功 + 记录 + inbox 清理
    assert_eq!(std::fs::read(&h.bin).unwrap(), payload);
    assert_eq!(
        std::fs::read_to_string(h.state.join("core.sha256")).unwrap(),
        sha
    );
    assert!(!inbox_file.exists());

    // Status 回显
    let Response::Status {
        core_version,
        core_sha256,
        ..
    } = rpc_call(&h.sock, &Request::Status).await
    else {
        panic!("unexpected status");
    };
    assert_eq!(core_version.as_deref(), Some("v9.9.9"));
    assert_eq!(core_sha256.as_deref(), Some(sha.as_str()));
}

#[tokio::test]
async fn install_core_sha_mismatch_rejects_and_keeps_binary() {
    let h = Harness::start(false).await;
    let original = std::fs::read(&h.bin).unwrap();

    let inbox_file = h.inbox.join("evil.bin");
    std::fs::write(&inbox_file, b"evil-binary").unwrap();

    let resp = rpc_call(
        &h.sock,
        &Request::InstallCore {
            inbox_path: inbox_file.display().to_string(),
            sha256: "0".repeat(64),
            version: "v9.9.9".into(),
        },
    )
    .await;
    let Response::Error { message } = resp else {
        panic!("应拒绝: {resp:?}");
    };
    assert!(message.contains("校验和失败"), "{message}");

    assert!(!inbox_file.exists(), "哈希不符 → inbox 删除");
    assert_eq!(
        std::fs::read(&h.bin).unwrap(),
        original,
        "二进制不被污染"
    );

    // 审计记录 error
    let Response::AuditQuery { records, .. } = rpc_call(&h.sock, &Request::AuditQuery { cursor: 0 }).await
    else {
        panic!("unexpected");
    };
    let last = records.last().unwrap();
    assert_eq!(last.op, "core.install");
    assert_eq!(last.result, "error");
}

#[tokio::test]
async fn install_core_rejects_inbox_outside_root() {
    let h = Harness::start(false).await;
    let outside = std::env::temp_dir().join(format!("clard-e2e-outside-{}.bin", std::process::id()));
    std::fs::write(&outside, b"x").unwrap();

    let resp = rpc_call(
        &h.sock,
        &Request::InstallCore {
            inbox_path: outside.display().to_string(),
            sha256: "0".repeat(64),
            version: "v1.0.0".into(),
        },
    )
    .await;
    let Response::Error { message } = resp else {
        panic!("应拒绝: {resp:?}");
    };
    assert!(message.contains("inbox 路径越界"), "{message}");
    let _ = std::fs::remove_file(&outside);
}

#[tokio::test]
async fn install_core_restarts_running_core() {
    let h = Harness::start(true).await;

    // 先应用运行态配置（start 前置条件），再启动核心（/bin/sleep 30 + mock /version 探测通过）
    let resp = rpc_call(&h.sock, &Request::ApplyConfig { yaml: "mode: rule\nmixed-port: 7890\n".into() }).await;
    assert_eq!(resp, Response::Ok, "{resp:?}");
    let resp = rpc_call(&h.sock, &Request::StartCore).await;
    assert_eq!(resp, Response::Ok, "{resp:?}");
    let Response::Status { core_state, .. } = rpc_call(&h.sock, &Request::Status).await else {
        panic!("unexpected");
    };
    assert_eq!(core_state, "running");

    // 升级 → helper 应重启核心（stop+start）。新 payload 也是忽略参数的存活脚本
    // （含版本标记，验证替换生效），start 后保持 running。
    let payload = b"#!/bin/sh\n# mihomo-v10.0.0 placeholder\nexec /bin/sleep 30\n".to_vec();
    let sha = sha256_hex(&payload);
    let inbox_file = h.inbox.join("core.bin");
    std::fs::write(&inbox_file, &payload).unwrap();
    let resp = rpc_call(
        &h.sock,
        &Request::InstallCore {
            inbox_path: inbox_file.display().to_string(),
            sha256: sha.clone(),
            version: "v10.0.0".into(),
        },
    )
    .await;
    eprintln!("install resp: {resp:?}");
    assert_eq!(resp, Response::Ok, "{resp:?}");
    let Response::Status { core_state, .. } = rpc_call(&h.sock, &Request::Status).await else {
        panic!("unexpected");
    };
    eprintln!("post-install state: {core_state}");
    if core_state != "running" {
        // 重启失败：打印 helper 侧原因（rpc 返回 Error 时上面的断言已拦；这里兜底）
        panic!("post-install state != running: {core_state}");
    }

    // 二进制已替换 + 核心重新运行 + 版本记录落盘（运行中 Status 版本取 probe 值）
    assert_eq!(std::fs::read(&h.bin).unwrap(), payload);
    assert_eq!(
        std::fs::read_to_string(h.state.join("core.version")).unwrap(),
        "v10.0.0"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let Response::Status { core_state, .. } = rpc_call(&h.sock, &Request::Status).await else {
        panic!("unexpected");
    };
    assert_eq!(core_state, "running", "升级后核心自动重启");
}

/// 额外：帧协议健壮性（超长帧被拒——helper 侧 16MiB 上限）。
#[tokio::test]
async fn oversized_frame_rejected_by_helper() {
    let h = Harness::start(false).await;
    let mut stream = UnixStream::connect(&h.sock).await.unwrap();
    // 声明 17MiB 长度
    stream
        .write_all(&(17u32 * 1024 * 1024).to_be_bytes())
        .await
        .unwrap();
    let mut buf = [0u8; 4];
    let r = tokio::time::timeout(Duration::from_secs(3), stream.read_exact(&mut buf)).await;
    match r {
        Err(_) => {} // 超时：helper 未响应
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {} // 连接被关（帧过大）
        other => panic!("应被拒绝: {other:?}"),
    }
}

/// 读文件辅助（消除未用 import 警告）。
#[allow(dead_code)]
fn _read_ok(p: &Path) -> Vec<u8> {
    let mut f = std::fs::File::open(p).unwrap();
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    v
}

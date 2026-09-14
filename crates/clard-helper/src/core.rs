//! 核心进程生命周期（doc/01 §5，功能子集：start/stop/restart/apply/status）。
//!
//! 边界：本期实现核心的启动/停止/重启、运行态配置落盘与 `PUT /configs` 内联热重载、
//! 就绪探测（`GET /version` 经 core.sock）；watchdog 退避重启 / 启动自检 / fail-open
//! 属后续里程碑（doc/01 §5.3/§5.4），TUN 相关一律不在此模块。
//!
//! 核心二进制只从固定路径执行（root 拥有，doc/01 §5.1）；路径可用 `CLARD_CORE_BIN`、
//! `CLARD_CORE_SOCK` 覆盖（开发/测试）。

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use std::process::Stdio;
use tokio::process::{Child, Command};

/// 核心二进制默认路径（root 拥有，非 symlink）。
fn core_bin_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLARD_CORE_BIN") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/usr/libexec/clard/mihomo")
}

/// 核心 ext-ctl unix socket 路径。
pub fn core_sock_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLARD_CORE_SOCK") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/run/clard/core.sock")
}

/// 核心进程管理器（helper 单写者经 daemon 锁串行访问）。
pub struct CoreManager {
    runtime_dir: PathBuf,
    config_path: PathBuf,
    core_sock: PathBuf,
    core_bin: PathBuf,
    child: Option<Child>,
    pid: Option<u32>,
    version: Option<String>,
}

impl CoreManager {
    pub fn new(state: &Path) -> Self {
        let runtime_dir = state.join("runtime");
        Self {
            runtime_dir: runtime_dir.clone(),
            config_path: runtime_dir.join("config.yaml"),
            core_sock: core_sock_path(),
            core_bin: core_bin_path(),
            child: None,
            pid: None,
            version: None,
        }
    }

    /// 当前状态：`running` / `stopped`（顺带回收已退出子进程）。
    pub fn state(&mut self) -> &'static str {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    self.pid = None;
                    self.version = None;
                    "stopped"
                }
                Ok(None) => "running",
                Err(_) => "stopped",
            }
        } else {
            "stopped"
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// 启动核心（幂等）：需先存在运行态配置；就绪探测成功才算启动成功。
    pub async fn start(&mut self) -> Result<(), String> {
        if self.state() == "running" {
            return Ok(());
        }
        if !self.config_path.exists() {
            return Err("运行态配置不存在，请先应用配置".to_string());
        }
        std::fs::create_dir_all(&self.runtime_dir).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&self.core_sock);

        let child = Command::new(&self.core_bin)
            .arg("-d")
            .arg(&self.runtime_dir)
            .arg("-f")
            .arg(&self.config_path)
            .arg("-ext-ctl-unix")
            .arg(&self.core_sock)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("启动核心失败: {e}"))?;
        self.pid = child.id();
        self.child = Some(child);

        match self.wait_ready().await {
            Ok(version) => {
                self.version = Some(version);
                Ok(())
            }
            Err(e) => {
                self.stop().await?;
                Err(format!("核心就绪探测失败: {e}"))
            }
        }
    }

    /// 停止核心（幂等）：SIGTERM 优雅退出，超时兜底。
    pub async fn stop(&mut self) -> Result<(), String> {
        if let Some(pid) = self.pid {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid as i32),
                nix::sys::signal::Signal::SIGTERM,
            );
        }
        if let Some(mut child) = self.child.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
        }
        self.pid = None;
        self.version = None;
        let _ = std::fs::remove_file(&self.core_sock);
        Ok(())
    }

    pub async fn restart(&mut self) -> Result<(), String> {
        self.stop().await?;
        self.start().await
    }

    /// 投递运行态配置：落盘 `/var/lib/clard/runtime/config.yaml`（原子），
    /// 核心运行中则 `PUT /configs?force=true`（内联 payload）热重载。
    pub async fn apply_config(&mut self, yaml: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.runtime_dir).map_err(|e| e.to_string())?;
        atomic_write(&self.config_path, yaml.as_bytes()).map_err(|e| e.to_string())?;
        if self.state() == "running" {
            reload_config(&self.core_sock, yaml).await?;
        }
        Ok(())
    }

    async fn wait_ready(&self) -> Result<String, String> {
        for _ in 0..20 {
            match probe_version(&self.core_sock).await {
                Ok(v) => return Ok(v),
                Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
        Err("就绪探测超时".into())
    }
}

/// 就绪探测：`GET /version`（经 core.sock），返回核心版本。
async fn probe_version(sock: &Path) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .unix_socket(sock)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("http://localhost/version")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    #[derive(serde::Deserialize)]
    struct Version {
        version: String,
    }
    let v: Version = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v.version)
}

/// 热重载：`PUT /configs?force=true`（内联 payload，doc/01 §5.5）。
async fn reload_config(sock: &Path, yaml: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .unix_socket(sock)
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "payload": yaml });
    let resp = client
        .put("http://localhost/configs")
        .query(&[("force", "true")])
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status != 200 && status != 204 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(format!("热重载失败: HTTP {status} {msg}"));
    }
    Ok(())
}

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    use super::*;

    /// 起一个 unix socket mock HTTP 服务，响应指定 status/body，并记录请求。
    async fn spawn_mock(
        path: PathBuf,
        status: &str,
        body: &str,
    ) -> tokio::task::JoinHandle<(String, String, String)> {
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let status = status.to_string();
        let body = body.to_string();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = sock.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&buf);
            let mut lines = text.lines();
            let req_line = lines.next().unwrap_or_default().to_string();
            let (method, path_and_query) = req_line.split_once(' ').unwrap_or(("", ""));
            let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query, ""));

            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            let _ = sock.shutdown().await;
            (method.to_string(), path.to_string(), format!("{}?{}", path, query))
        })
    }

    fn mock_sock(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("clard-mock-{}-{name}.sock", std::process::id()))
    }

    #[tokio::test]
    async fn probe_version_parses_version() {
        let path = mock_sock("probe");
        let handle = spawn_mock(path.clone(), "200 OK", r#"{"version":"1.19.2","meta":true}"#).await;
        let v = probe_version(&path).await.unwrap();
        assert_eq!(v, "1.19.2");
        let _ = handle.await.unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn probe_version_non_200_is_error() {
        let path = mock_sock("probe500");
        let _handle = spawn_mock(path.clone(), "500 Internal Server Error", r#"{"message":"x"}"#).await;
        assert!(probe_version(&path).await.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn reload_config_posts_inline_payload() {
        let path = mock_sock("reload");
        let handle = spawn_mock(path.clone(), "204 No Content", "").await;
        reload_config(&path, "mixed-port: 7890\n").await.unwrap();
        let (method, path, _query) = handle.await.unwrap();
        assert_eq!(method, "PUT");
        assert_eq!(path, "/configs");
        let _ = std::fs::remove_file(&path);
    }
}

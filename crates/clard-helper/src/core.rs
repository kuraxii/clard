//! 核心进程生命周期（doc/01 §5，功能子集：start/stop/restart/apply/status）。
//!
//! 边界：本期实现核心的启动/停止/重启、运行态配置落盘与 `PUT /configs` 内联热重载、
//! 就绪探测（`GET /version` 经 core.sock）；watchdog 退避重启 / 启动自检 / fail-open
//! 属后续里程碑（doc/01 §5.3/§5.4），TUN 相关一律不在此模块。
//!
//! 核心二进制只从固定路径执行（root 拥有，doc/01 §5.1）；路径可用 `CLARD_CORE_BIN`、
//! `CLARD_CORE_SOCK` 覆盖（开发/测试）。

use std::{
    fs,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use std::process::Stdio;
use tokio::process::{Child, Command};

/// 核心二进制默认路径（/var/clard/bin，root 拥有，动态升级；doc/01 §5.1）。
/// 放在 /var 而非 /usr/libexec：属于动态变化的外部二进制，随数据目录统一管理。
fn core_bin_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLARD_CORE_BIN") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/var/clard/bin/mihomo")
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
    /// 用户是否期望核心运行（watchdog 据此自动重启；StopCore 清除）
    want_running: bool,
    /// 崩溃时刻（退避计算，窗口 600s）
    crash_times: Vec<Instant>,
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
            want_running: false,
            crash_times: Vec::new(),
        }
    }

    /// 用户是否期望核心运行（watchdog 自动重启依据）。
    pub fn is_want_running(&self) -> bool {
        self.want_running
    }

    /// 设置期望运行（watchdog 超限后清除，停止自动重启）。
    pub fn set_want_running(&mut self, want: bool) {
        self.want_running = want;
    }

    /// 核心崩溃退避（§5.4）：窗口 600s 内崩溃 ≥10 次返回 None（watchdog 超限 fail-open）。
    /// 否则记录本次崩溃并返回退避时长（2^崩溃次数 秒，上限 30s）。
    pub fn crash_backoff(&mut self) -> Option<Duration> {
        const WINDOW: Duration = Duration::from_secs(600);
        const MAX_RESTARTS: usize = 10;
        const MAX_BACKOFF: u64 = 30;
        let now = Instant::now();
        self.crash_times.retain(|t| now.duration_since(*t) < WINDOW);
        if self.crash_times.len() >= MAX_RESTARTS {
            return None;
        }
        let n = self.crash_times.len();
        self.crash_times.push(now);
        let secs = (1u64 << n.min(5)).min(MAX_BACKOFF);
        Some(Duration::from_secs(secs))
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

    /// 运行态配置路径（SetTun 编辑入口）。
    pub fn runtime_config_path(&self) -> PathBuf {
        self.config_path.clone()
    }

    /// 启动核心（幂等）：需先存在运行态配置；就绪探测成功才算启动成功。
    /// 启动前校验二进制：存在、root 拥有、非 symlink、非 group/other 可写（§5.1）。
    pub async fn start(&mut self) -> Result<(), String> {
        if self.state() == "running" {
            return Ok(());
        }
        if !self.config_path.exists() {
            return Err("运行态配置不存在，请先应用配置".to_string());
        }
        verify_core_binary(&self.core_bin)?;
        self.want_running = true;
        fs::create_dir_all(&self.runtime_dir).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&self.core_sock);

        let mut child = Command::new(&self.core_bin)
            .arg("-d")
            .arg(&self.runtime_dir)
            .arg("-f")
            .arg(&self.config_path)
            .arg("-ext-ctl-unix")
            .arg(&self.core_sock)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("启动核心失败: {e}"))?;
        // 核心 stdout → core.log（管道逐行转储 + 10MB×5 轮转，doc/01 §10/R6.1）
        if let Some(mut out) = child.stdout.take() {
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut reader = tokio::io::BufReader::new(&mut out);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let _ = crate::logs::append_core_log(line.trim_end());
                        }
                    }
                }
            });
        }
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
        self.want_running = false;
        let _ = fs::remove_file(&self.core_sock);
        Ok(())
    }

    pub async fn restart(&mut self) -> Result<(), String> {
        self.stop().await?;
        self.start().await
    }

    /// 投递运行态配置：落盘 `/var/clard/lib/runtime/config.yaml`（原子），
    /// 核心运行中则 `PUT /configs?force=true`（内联 payload）热重载。
    pub async fn apply_config(&mut self, yaml: &str) -> Result<(), String> {
        fs::create_dir_all(&self.runtime_dir).map_err(|e| e.to_string())?;
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

/// 校验核心二进制（§5.1）：存在、非 symlink、root 拥有、非 group/other 可写。
fn verify_core_binary(bin: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(bin).map_err(|e| {
        format!("核心二进制缺失（{}），请先在设置页「Core」安装核心: {e}", bin.display())
    })?;
    if meta.file_type().is_symlink() {
        return Err(format!("核心二进制是符号链接，拒绝执行: {}", bin.display()));
    }
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if meta.uid() != 0 {
        return Err(format!("核心二进制属主非 root: {}", bin.display()));
    }
    let mode = meta.permissions().mode();
    if mode & 0o022 != 0 {
        return Err(format!("核心二进制 group/other 可写（mode {mode:o}），拒绝执行: {}", bin.display()));
    }
    Ok(())
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
pub(crate) async fn reload_config(sock: &Path, yaml: &str) -> Result<(), String> {
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

/// 字段级热更新：`PATCH /configs`（doc/04 §3，TUN 开关走这里，无需重启核心）。
pub(crate) async fn patch_configs(sock: &Path, body: &serde_json::Value) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .unix_socket(sock)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .patch("http://localhost/configs")
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if status != 200 && status != 204 {
        let msg = resp.text().await.unwrap_or_default();
        return Err(format!("字段级热更新失败: HTTP {status} {msg}"));
    }
    Ok(())
}

/// 回读当前生效配置：`GET /configs`（doc/04 §3，回读校验取数点）。
pub(crate) async fn get_configs(sock: &Path) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .unix_socket(sock)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("http://localhost/configs")
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// 核心已安装 sha256 记录（R7.3 展示，InstallCore 后写入）。
pub fn core_sha256_path(state: &Path) -> PathBuf {
    state.join("core.sha256")
}

/// 核心已安装版本记录（核心未运行时 Status 回退展示）。
pub fn core_version_path(state: &Path) -> PathBuf {
    state.join("core.version")
}

/// 核心安装的目标路径组（测试可直接构造，避免 env 全局互扰）。
#[derive(Debug, Clone)]
pub struct InstallTargets {
    pub inbox_root: PathBuf,
    pub bin_path: PathBuf,
}

impl InstallTargets {
    /// 生产路径：inbox=/run/clard/inbox、bin=CLARD_CORE_BIN 或 /var/clard/bin/mihomo。
    pub fn production() -> Self {
        Self {
            inbox_root: crate::daemon::inbox_dir(),
            bin_path: core_bin_path(),
        }
    }
}

/// 复核 inbox 文件哈希并原子替换核心二进制（doc/01 §5.1，R7.3）。
///
/// 安全要点：
/// - inbox 路径必须位于 inbox 根目录下（防穿越）；
/// - `O_NOFOLLOW` 打开（拒绝符号链接，防 TOCTOU/提权）；
/// - 流式 sha256 复核，不匹配即删除 inbox 文件并拒绝；
/// - copy（不信任 rename）到 `mihomo.tmp` → fsync → `rename` 原子替换 → chmod 0755。
pub fn install_core(
    state: &Path,
    targets: &InstallTargets,
    inbox_path: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    if !inbox_path.starts_with(&targets.inbox_root) {
        return Err(format!(
            "inbox 路径越界（{} 不在 {} 下）",
            inbox_path.display(),
            targets.inbox_root.display()
        ));
    }

    // O_NOFOLLOW 打开 + 流式 sha256
    let f = nix::fcntl::open(
        inbox_path,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|e| format!("打开 inbox 文件失败: {e}"))?;
    let mut f = fs::File::from(f);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("读取 inbox 文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if actual != expected {
        let _ = fs::remove_file(inbox_path);
        return Err(format!("校验和失败：期望 {expected}，实际 {actual}（已删除 inbox 文件）"));
    }

    // 原子替换：copy → tmp → fsync → rename → chmod 0755
    let bin = &targets.bin_path;
    let parent = bin.parent().ok_or("核心二进制路径无父目录")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join("mihomo.tmp");
    {
        let mut out = fs::File::create(&tmp).map_err(|e| format!("写临时文件失败: {e}"))?;
        f.seek(io::SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        io::copy(&mut f, &mut out).map_err(|e| format!("拷贝安装包失败: {e}"))?;
        out.sync_all().map_err(|e| format!("fsync 失败: {e}"))?;
    }
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &bin).map_err(|e| format!("原子替换失败: {e}"))?;

    // 记录（供展示与下次安装对比）
    atomic_write(&core_sha256_path(state), expected.as_bytes()).map_err(|e| e.to_string())?;

    // 清理 inbox
    let _ = fs::remove_file(inbox_path);
    Ok(())
}

fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)?;
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
        let _ = fs::remove_file(&path);
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
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn probe_version_non_200_is_error() {
        let path = mock_sock("probe500");
        let _handle = spawn_mock(path.clone(), "500 Internal Server Error", r#"{"message":"x"}"#).await;
        assert!(probe_version(&path).await.is_err());
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn reload_config_posts_inline_payload() {
        let path = mock_sock("reload");
        let handle = spawn_mock(path.clone(), "204 No Content", "").await;
        reload_config(&path, "mixed-port: 7890\n").await.unwrap();
        let (method, path, _query) = handle.await.unwrap();
        assert_eq!(method, "PUT");
        assert_eq!(path, "/configs");
        let _ = fs::remove_file(&path);
    }

    // ---- install_core（R7.3，§5.1）----

    /// 准备 temp state + inbox + bin 路径（TempDir 随返回值存活，目录不被提前删除）。
    fn install_env() -> (tempfile::TempDir, PathBuf, PathBuf, InstallTargets) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let inbox = dir.path().join("inbox");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&inbox).unwrap();
        let targets = InstallTargets {
            inbox_root: inbox.clone(),
            bin_path: dir.path().join("bin").join("mihomo"),
        };
        (dir, state, inbox, targets)
    }

    #[test]
    fn install_core_replaces_binary_atomically_and_records_sha() {
        let (_dir, state, inbox, targets) = install_env();
        let payload = b"mihomo-v2.0.0".to_vec();
        let sha = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(&payload);
            format!("{:x}", h.finalize())
        };
        let inbox_file = inbox.join("core.bin");
        fs::write(&inbox_file, &payload).unwrap();

        install_core(&state, &targets, &inbox_file, &sha).unwrap();

        // 替换目标存在、内容一致、0755、root 拥有
        assert_eq!(fs::read(&targets.bin_path).unwrap(), payload);
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = fs::metadata(&targets.bin_path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
        assert_eq!(meta.uid(), 0);
        // sha 记录
        assert_eq!(fs::read_to_string(core_sha256_path(&state)).unwrap(), sha);
        // inbox 已清理
        assert!(!inbox_file.exists());
    }

    #[test]
    fn install_core_sha_mismatch_rejects_and_removes_inbox() {
        let (_dir, state, inbox, targets) = install_env();
        let inbox_file = inbox.join("core.bin");
        fs::write(&inbox_file, b"evil").unwrap();

        let e = install_core(&state, &targets, &inbox_file, &"0".repeat(64)).unwrap_err();
        assert!(e.contains("校验和失败"), "{e}");
        assert!(!inbox_file.exists(), "哈希不符 → 删除 inbox 文件");
    }

    #[test]
    fn install_core_rejects_path_traversal_outside_inbox() {
        let (_dir, state, _inbox, targets) = install_env();
        let outside = std::env::temp_dir().join(format!("clard-outside-inbox-{}.bin", std::process::id()));
        fs::write(&outside, b"x").unwrap();
        let e = install_core(&state, &targets, &outside, &"0".repeat(64)).unwrap_err();
        assert!(e.contains("inbox 路径越界"), "{e}");
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn install_core_rejects_symlink_inbox() {
        let (_dir, state, inbox, targets) = install_env();
        let target = std::env::temp_dir().join(format!("clard-symlink-target-{}.bin", std::process::id()));
        fs::write(&target, b"payload").unwrap();
        let link = inbox.join("core.bin");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let e = install_core(&state, &targets, &link, &"0".repeat(64)).unwrap_err();
        assert!(e.contains("打开 inbox 文件失败"), "O_NOFOLLOW 拒绝 symlink: {e}");
        assert!(!fs::read_to_string(&target).map(|s| s.is_empty()).unwrap_or(true), "目标未被读取/影响");
        let _ = fs::remove_file(&target);
    }

    #[test]
    fn verify_core_binary_checks_presence_owner_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("mihomo");
        // 缺失
        assert!(verify_core_binary(&bin).unwrap_err().contains("核心二进制缺失"));
        // 正常 0755
        fs::write(&bin, b"ELF").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(verify_core_binary(&bin).is_ok());
        // group/other 可写拒绝
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o775)).unwrap();
        assert!(verify_core_binary(&bin).unwrap_err().contains("group/other 可写"));
        // symlink 拒绝
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&bin, &link).unwrap();
        assert!(verify_core_binary(&link).unwrap_err().contains("符号链接"));
    }

    #[test]
    fn crash_backoff_counts_within_window_then_limits() {
        let dir = tempfile::tempdir().unwrap();
        let mut cm = CoreManager::new(dir.path());
        // 窗口 600s 内前 10 次崩溃 → Some（max_restarts=10，各自退避重启），退避 ≤ 30s
        for _ in 0..10 {
            let d = cm.crash_backoff().unwrap();
            assert!(d.as_secs() <= 30);
        }
        // 第 11 次 → 超限（watchdog fail-open，§5.4）
        assert!(cm.crash_backoff().is_none());
    }
}

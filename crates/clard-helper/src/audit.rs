//! 操作审计（最小实现，doc/01 §10）。
//!
//! 系统级操作由 helper 记录：op + actor（uid/pid）+ result。双写：
//! - stdout `KEY=VALUE` 行（systemd 自动解析为 journald 字段）；
//! - JSON lines 文件（`/var/clard/log/audit.log`，`CLARD_LOG_DIR` 可覆盖用于开发/测试）。
//!
//! 完整规格（intent+result 双记录、net 前后快照、cfg_sha256 等）随 M1/M2 里程碑补全。

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

/// 连接对端身份（`SO_PEERCRED` 记录，doc/01 §4.2）
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub uid: u32,
    pub pid: i32,
}

impl Actor {
    /// 系统自身触发的操作（如自动更新定时器）使用的占位身份。
    pub fn system() -> Self {
        Self {
            uid: u32::MAX,
            pid: -1,
        }
    }
}

/// 审计写入器
pub struct Audit {
    log_path: PathBuf,
}

/// 日志目录：`CLARD_LOG_DIR` 覆盖，默认 `/var/clard/log`。
pub fn log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CLARD_LOG_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    PathBuf::from("/var/clard/log")
}

impl Audit {
    pub fn open() -> Self {
        let dir = log_dir();
        let log_path = dir.join("audit.log");
        Self { log_path }
    }

    /// 记录一次操作结果。
    pub fn record(&self, op: &str, actor: &Actor, result: &str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        // journald：stdout KEY=VALUE（systemd 解析为字段，journalctl -u clard-helper CLARD_OP=... 可查）
        println!("CLARD_TS={ts} CLARD_OP={op} CLARD_ACTOR_UID={} CLARD_ACTOR_PID={} CLARD_RESULT={result}", actor.uid, actor.pid);
        // 自有文件：JSON lines（10MB×5 轮转，doc/01 §10）
        let line = format!(
            r#"{{"ts":{ts},"op":"{op}","actor":{{"uid":{},"pid":{}}},"result":"{result}"}}"#,
            actor.uid, actor.pid
        );
        let _ = crate::logs::append_rotated(
            &self.log_path,
            &line,
            crate::logs::AUDIT_CORE_MAX_BYTES,
            crate::logs::KEEP_FILES,
        );
    }
}

//! 操作审计（doc/01 §10，R6.3）：intent/result 双记录、net 前后快照、cfg_sha256。
//!
//! 系统级操作由 helper 记录。同一次操作写两条 JSON line（相同 `op_id`）：
//! - `phase=intent`（操作前）：op + actor + 意图描述 + net_before 快照；
//! - `phase=result`（操作后）：op + op_id + actor + result(ok/error/partial) + err +
//!   net_after 快照 + cfg_sha256（配置类操作）。
//!
//! 双写：stdout `KEY=VALUE`（systemd 解析为 journald 字段）+ JSON lines 文件
//! （`/var/clard/log/audit.log`，10MB×5 轮转）。

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
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
    seq: AtomicU64,
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
        Self {
            log_path,
            seq: AtomicU64::new(0),
        }
    }

    /// 记录操作意图（操作前），返回 `op_id` 供 result 关联。
    pub fn intent(&self, op: &str, actor: &Actor, intent_desc: &str) -> String {
        let ts = now_millis();
        let op_id = format!("{ts}-{:06}", self.seq.fetch_add(1, Ordering::Relaxed));
        let net = net_snapshot();
        self.write(
            op,
            &op_id,
            "intent",
            actor,
            "pending",
            intent_desc,
            None,
            net,
            None,
        );
        op_id
    }

    /// 记录操作结果（操作后）。
    #[allow(clippy::too_many_arguments)]
    pub fn result(
        &self,
        op: &str,
        op_id: &str,
        actor: &Actor,
        result: &str,
        err: Option<&str>,
        cfg_sha256: Option<&str>,
    ) {
        let net = net_snapshot();
        self.write(op, op_id, "result", actor, result, "", err, net, cfg_sha256);
    }

    /// 单条写入（journald KEY=VALUE + JSON lines 文件，10MB×5 轮转）。
    #[allow(clippy::too_many_arguments)]
    fn write(
        &self,
        op: &str,
        op_id: &str,
        phase: &str,
        actor: &Actor,
        result: &str,
        intent: &str,
        err: Option<&str>,
        net: Option<String>,
        cfg_sha256: Option<&str>,
    ) {
        let ts = now_millis();
        let err_s = err.unwrap_or("");
        let net_s = net.as_deref().unwrap_or("");
        let cfg_s = cfg_sha256.unwrap_or("");
        let json = serde_json::json!({
            "ts": ts,
            "op": op,
            "op_id": op_id,
            "phase": phase,
            "actor": {"uid": actor.uid, "pid": actor.pid},
            "result": result,
            "intent": intent,
            "err": err,
            "net": net,
            "cfg_sha256": cfg_sha256,
        });
        // 双写 journald（helper.toml `audit_dual_write`，默认开；R7.5 可关）
        if crate::helper_config::global().audit_dual_write {
            println!(
                "CLARD_TS={ts} CLARD_OP={op} CLARD_OP_ID={op_id} CLARD_PHASE={phase} \
                 CLARD_ACTOR_UID={} CLARD_ACTOR_PID={} CLARD_RESULT={result} CLARD_INTENT={intent} \
                 CLARD_ERR={err_s} CLARD_NET={net_s} CLARD_CFG_SHA256={cfg_s}",
                actor.uid, actor.pid
            );
        }
        let _ = crate::logs::append_rotated(
            &self.log_path,
            &json.to_string(),
            crate::logs::AUDIT_CORE_MAX_BYTES,
            crate::helper_config::global().audit_keep,
        );
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 网络快照（§10.2 net 前后快照）：clard0 网卡状态、table 2023 路由、rule 9100 段。
/// shell out `ip`（同步、低频，审计专用）；任何一步失败仅记为局部快照。
pub fn net_snapshot() -> Option<String> {
    use std::process::Command;
    fn run(args: &[&str]) -> String {
        Command::new("ip")
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }
    let link = run(&["-o", "link", "show", "clard0"]);
    let route4 = run(&["-4", "route", "show", "table", "2023"]);
    let rule = run(&["rule"]);
    let has_rule = rule.lines().any(|l| {
        l.trim_start()
            .split(':')
            .next()
            .and_then(|p| p.trim().parse::<i64>().ok())
            .is_some_and(|p| (9100..9110).contains(&p))
    });
    Some(serde_json::json!({
        "clard0": if link.is_empty() { "absent" } else { "present" },
        "table2023_route": !route4.is_empty(),
        "rule9100": has_rule,
    })
    .to_string())
}

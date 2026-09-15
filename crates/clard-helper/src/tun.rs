//! TUN 落地与残留清理（doc/01 §6）：能力探测 / 冲突检测 / 幂等清理。
//!
//! 边界：网卡/路由/规则由 mihomo 核心进程自己创建（§6.1），helper 不参与创建。
//! TUN 开关自阶段 3 起并入 `SettingsSet`→`regenerate`（§8.4：clard-config 注入 tun 块 +
//! PUT 热更 + 回读校验），本模块仅提供开启前置检查（能力/冲突/残留）与 cleanup-tun。
//! 失败必须 fail-open（恢复直连），绝不 fail-closed。

use std::path::{Path, PathBuf};

use tokio::process::Command;

/// 托管固定标识（§6.2，与 clard-core config_gen 的 TunOptions 保持一致）。
pub const TUN_DEVICE: &str = "clard0";
pub const TUN_TABLE: i64 = 2023;
/// 清理区间（§6.4：删除 [9100, 9110) 的 rule）。
pub const RULE_RANGE: std::ops::Range<i64> = 9100..9110;


/// 外部命令工具路径（测试注入 fake 脚本）。
#[derive(Debug, Clone)]
pub struct Tools {
    pub ip: PathBuf,
    pub nft: PathBuf,
    pub resolvectl: PathBuf,
}

impl Tools {
    /// 系统默认：按 PATH 解析（iproute2 / nftables / systemd-resolved）。
    pub fn system() -> Self {
        Self {
            ip: "ip".into(),
            nft: "nft".into(),
            resolvectl: "resolvectl".into(),
        }
    }
}

/// 单条命令执行结果。
#[derive(Debug)]
struct CmdOut {
    status: bool,
    stdout: String,
    stderr: String,
}

impl CmdOut {
    fn ok(&self) -> bool {
        self.status
    }
}

async fn run(tool: &Path, args: &[&str]) -> CmdOut {
    match Command::new(tool).args(args).output().await {
        Ok(out) => CmdOut {
            status: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Err(e) => CmdOut {
            status: false,
            stdout: String::new(),
            stderr: format!("{e}"),
        },
    }
}

// ---- §6.3 托管 tun 块 ----

/// 构造托管 `tun` 块（enable=true 时完整注入，§6.3；enable=false 仅 enable:false）。
/// 与 clard-core `managed::inject` 的注入值保持一致（同一份契约）。
/// 能力探测：任一不满足返回可操作建议（TUI 直接展示）。
pub async fn capability_check(tools: &Tools) -> Result<(), String> {
    // /dev/net/tun 可访问
    if !Path::new("/dev/net/tun").exists() {
        return Err("缺少 /dev/net/tun（内核未加载 tun 模块或容器未挂载），无法开启 TUN".into());
    }
    // CAP_NET_ADMIN：helper 恒为 root 运行（doc/01 §3）；euid!=0 时明确提示
    if !nix::unistd::geteuid().is_root() {
        return Err("helper 未以 root 运行，无 CAP_NET_ADMIN，无法创建 TUN".into());
    }
    // iproute2 可用
    let probe = run(&tools.ip, &["-V"]).await;
    if !probe.ok() {
        return Err(format!("iproute2 不可用（`ip` 无法执行: {}）", probe.stderr.trim()));
    }
    Ok(())
}

// ---- §6.2 冲突检测 ----

/// 检测其他活跃 TUN 设备（如 clash-verge 的 `Meta`）。两个 TUN 无法共存——
/// auto-route 抢默认路由、dns-hijack 抢 53。检测到即报错，不静默开。
pub async fn check_other_tun(tools: &Tools) -> Result<(), String> {
    let out = run(&tools.ip, &["-o", "link", "show", "type", "tun"]).await;
    let devices = tun_devices(&out.stdout);
    let foreign: Vec<&str> = devices.iter().filter(|d| d.as_str() != TUN_DEVICE).map(String::as_str).collect();
    if !foreign.is_empty() {
        return Err(format!(
            "检测到其他工具正在占用 TUN（{}），请先停用，否则 auto-route/dns-hijack 会互相冲突",
            foreign.join(", ")
        ));
    }
    Ok(())
}

/// 解析 `ip -o link show type tun` 输出中的设备名：`3: clard0: <...> ...`。
fn tun_devices(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start();
            let after_idx = rest.find(": ")?;
            let name_end = rest[after_idx + 2..].find(':')? + after_idx + 2;
            Some(rest[after_idx + 2..name_end].to_string())
        })
        .collect()
}

/// 解析 `ip rule` 输出中落在 [lo, hi) 的 pref：`9100: from all lookup 2023`。
pub fn rule_prefs_in_range(output: &str, lo: i64, hi: i64) -> Vec<i64> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let pref = line.split(':').next()?.trim();
            pref.parse::<i64>().ok().filter(|p| *p >= lo && *p < hi)
        })
        .collect()
}

// ---- §6.5 健康判据 / 回读校验辅助 ----

/// 网卡存在且 UP。
pub async fn link_up(tools: &Tools, name: &str) -> bool {
    let out = run(&tools.ip, &["-o", "link", "show", name]).await;
    out.ok() && out.stdout.contains("UP")
}

/// 规则区间（[9100, 9110)）是否存在。
pub async fn rule_range_present(tools: &Tools) -> bool {
    let out = run(&tools.ip, &["rule"]).await;
    !rule_prefs_in_range(&out.stdout, RULE_RANGE.start, RULE_RANGE.end).is_empty()
}

/// table 2023 是否有（默认）路由。
pub async fn table_has_route(tools: &Tools) -> bool {
    let out4 = run(&tools.ip, &["-4", "route", "show", "table", &TUN_TABLE.to_string()]).await;
    let out6 = run(&tools.ip, &["-6", "route", "show", "table", &TUN_TABLE.to_string()]).await;
    out4.ok() && !out4.stdout.trim().is_empty() || out6.ok() && !out6.stdout.trim().is_empty()
}

/// §6.5 判据：核心在跑不等于 TUN 在工作。四条件（GET /version 由调用方探测）中
/// 网卡/规则/路由任一缺失即视为不健康。
pub async fn tun_active(tools: &Tools) -> bool {
    link_up(tools, TUN_DEVICE).await && rule_range_present(tools).await && table_has_route(tools).await
}

// ---- §6.4 cleanup-tun（幂等） ----

/// 幂等清理 TUN 残留（§6.4）：ip rule（最关键的残留）→ route table → 网卡 →
/// nft 表 → resolvectl。每步失败只记 warn 继续。返回 (是否彻底干净, 残余描述)。
pub async fn cleanup_tun(tools: &Tools) -> (bool, Vec<String>) {
    // 1. ip rule del pref 9100..9110（inet 与 inet6 各一遍）
    for pref in RULE_RANGE.clone() {
        let p = pref.to_string();
        for family in ["-4", "-6"] {
            let out = run(&tools.ip, &[family, "rule", "del", "pref", &p]).await;
            if !out.ok() {
                tracing::warn!("cleanup: ip {family} rule del pref {p}: {}", out.stderr.trim());
            }
        }
    }
    // 2. ip route flush table 2023
    for family in ["-4", "-6"] {
        let out = run(&tools.ip, &[family, "route", "flush", "table", &TUN_TABLE.to_string()]).await;
        if !out.ok() {
            tracing::warn!("cleanup: ip {family} route flush table {}: {}", TUN_TABLE, out.stderr.trim());
        }
    }
    // 3. ip link del clard0（存在才删；非持久 TUN 本应随进程消失，兜底）
    let link = run(&tools.ip, &["-o", "link", "show", TUN_DEVICE]).await;
    if link.ok() {
        let out = run(&tools.ip, &["link", "del", TUN_DEVICE]).await;
        if !out.ok() {
            tracing::warn!("cleanup: ip link del {}: {}", TUN_DEVICE, out.stderr.trim());
        }
    }
    // 4. nft delete table inet mihomo（仅当启用过 auto-redirect 才可能残留）
    let out = run(&tools.nft, &["delete", "table", "inet", "mihomo"]).await;
    if !out.ok() && !out.stderr.contains("No such file or directory") && !out.stderr.contains("does not exist") {
        tracing::warn!("cleanup: nft delete table inet mihomo: {}", out.stderr.trim());
    }
    // 5. resolvectl revert clard0（best-effort，link 不存在时忽略）
    let out = run(&tools.resolvectl, &["revert", TUN_DEVICE]).await;
    if !out.ok() {
        tracing::warn!("cleanup: resolvectl revert {}: {}", TUN_DEVICE, out.stderr.trim());
    }

    // 6. 校验
    let residuals = residuals(&tools).await;
    (residuals.is_empty(), residuals)
}

/// 残留扫描（§6.4 第 6 步的校验查询）。
async fn residuals(tools: &Tools) -> Vec<String> {
    let mut list = Vec::new();
    let link = run(&tools.ip, &["-o", "link", "show", TUN_DEVICE]).await;
    if link.ok() {
        list.push(format!("link {TUN_DEVICE}"));
    }
    let rule = run(&tools.ip, &["rule"]).await;
    let prefs = rule_prefs_in_range(&rule.stdout, RULE_RANGE.start, RULE_RANGE.end);
    if !prefs.is_empty() {
        list.push(format!("rule pref {:?}", prefs));
    }
    for family in ["-4", "-6"] {
        let route = run(&tools.ip, &[family, "route", "show", "table", &TUN_TABLE.to_string()]).await;
        if route.ok() && !route.stdout.trim().is_empty() {
            list.push(format!("route table {TUN_TABLE} ({family})"));
        }
    }
    let nft = run(&tools.nft, &["list", "table", "inet", "mihomo"]).await;
    if nft.ok() {
        list.push("nft table inet mihomo".into());
    }
    list
}

/// TUN 开启前置检查（§5.6/§8.4）：能力探测 + 其他 TUN 占用检测 + 自身残留清理。
/// `core_running` 时若检测到自身标识占用视为冲突（报错）；核心未运行则先清理残留再继续。
pub(crate) async fn precheck_tun_enable(core_running: bool, tools: &Tools) -> Result<(), String> {
    capability_check(tools).await?;
    check_other_tun(tools).await?;
    // 自身残留（上次崩溃遗留）：核心未运行时清理后再开；核心运行中则视为他人占用
    let link = run(&tools.ip, &["-o", "link", "show", TUN_DEVICE]).await;
    let rule = run(&tools.ip, &["rule"]).await;
    let rule_busy = !rule_prefs_in_range(&rule.stdout, RULE_RANGE.start, RULE_RANGE.end).is_empty();
    if link.ok() || rule_busy || table_has_route(tools).await {
        if core_running {
            return Err(format!(
                "TUN 标识被占用（{TUN_DEVICE}/table {TUN_TABLE}/rule {RULE_RANGE:?}），且核心正在运行；\
                 可能是其他工具占用或残留，请先执行「紧急恢复直连」或停止核心后重试"
            ));
        }
        tracing::warn!("precheck: 检测到自身 TUN 残留，先清理再开启");
        cleanup_tun(tools).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parse_tun_devices() {
        let out = "1: lo: <LOOPBACK,...> \\    link/loopback ...\n\
                   3: clard0: <POINTOPOINT,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UNKNOWN \\    link/none\n\
                   4: Meta: <POINTOPOINT,UP> mtu 1500 ... \\    link/none\n";
        let devs = tun_devices(out);
        assert_eq!(devs, vec!["lo", "clard0", "Meta"]);
    }

    #[test]
    fn parse_rule_prefs_in_range() {
        let out = "9000:\tfrom all lookup 2022\n9100:\tfrom all lookup 2023\n9105:\tfrom all lookup 2023\n9110:\tfrom all lookup 2024\n";
        let prefs = rule_prefs_in_range(out, 9100, 9110);
        assert_eq!(prefs, vec![9100, 9105]);
    }

    #[test]
    fn parse_rule_ignores_unrelated_prefs() {
        let out = "32767: from all lookup 100\n";
        assert!(rule_prefs_in_range(out, 9100, 9110).is_empty());
    }
}

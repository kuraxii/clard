//! TUN 落地与残留清理（doc/01 §6）：能力探测 / 冲突检测 / 开关（托管注入 + PATCH 热更 +
//! 回读校验）/ 幂等清理。
//!
//! 边界：网卡/路由/规则由 mihomo 核心进程自己创建（§6.1），helper 不参与创建，只负责
//! 开关（`PATCH /configs` 字段级热更，doc/04 §3）与残留清理（shell out `ip`/`nft`/
//! `resolvectl`，§6.4——不自写 netlink）。失败必须 fail-open（恢复直连），绝不 fail-closed。

use std::path::{Path, PathBuf};

use clard_proto::Settings;
use serde_yaml_ng::{Mapping, Value};
use tokio::process::Command;

/// 托管固定标识（§6.2，与 clard-core config_gen 的 TunOptions 保持一致）。
pub const TUN_DEVICE: &str = "clard0";
pub const TUN_TABLE: i64 = 2023;
pub const TUN_RULE: i64 = 9100;
/// 清理区间（§6.4：删除 [9100, 9110) 的 rule）。
pub const RULE_RANGE: std::ops::Range<i64> = 9100..9110;

/// 默认 dns-hijack（§6.3）。
pub const DEFAULT_DNS_HIJACK: &[&str] = &["any:53", "tcp://any:53"];
/// 默认私网排除段（§6.7，用户可追加/替换）。
pub const DEFAULT_ROUTE_EXCLUDE: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
    "127.0.0.0/8",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
];

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
pub fn build_tun_block(settings: &Settings) -> Mapping {
    let mut t = Mapping::new();
    t.insert(Value::String("enable".into()), Value::Bool(true));
    kv(&mut t, "device", TUN_DEVICE);
    let stack = if settings.tun_stack.is_empty() {
        "system"
    } else {
        settings.tun_stack.as_str()
    };
    kv(&mut t, "stack", stack);
    kv(&mut t, "auto-route", true);
    kv(&mut t, "auto-detect-interface", true);
    let hijack: Vec<&str> = if settings.dns_hijack.is_empty() {
        DEFAULT_DNS_HIJACK.to_vec()
    } else {
        settings.dns_hijack.iter().map(String::as_str).collect()
    };
    t.insert(Value::String("dns-hijack".into()), Value::Sequence(strs(&hijack)));
    t.insert(Value::String("iproute2-table-index".into()), Value::Number(TUN_TABLE.into()));
    t.insert(Value::String("iproute2-rule-index".into()), Value::Number(TUN_RULE.into()));
    t.insert(Value::String("strict-route".into()), Value::Bool(settings.strict_route));
    t.insert(Value::String("auto-redirect".into()), Value::Bool(settings.auto_redirect));
    let exclude: Vec<&str> = if settings.route_exclude_address.is_empty() {
        DEFAULT_ROUTE_EXCLUDE.to_vec()
    } else {
        settings.route_exclude_address.iter().map(String::as_str).collect()
    };
    t.insert(
        Value::String("route-exclude-address".into()),
        Value::Sequence(strs(&exclude)),
    );
    if !settings.exclude_uid.is_empty() {
        t.insert(
            Value::String("exclude-uid".into()),
            Value::Sequence(settings.exclude_uid.iter().map(|v| Value::Number((*v).into())).collect()),
        );
    }
    if !settings.exclude_interface.is_empty() {
        t.insert(
            Value::String("exclude-interface".into()),
            Value::Sequence(settings.exclude_interface.iter().map(|s| Value::String(s.clone())).collect()),
        );
    }
    if !settings.exclude_dst_port.is_empty() {
        t.insert(
            Value::String("exclude-dst-port".into()),
            Value::Sequence(settings.exclude_dst_port.iter().map(|v| Value::Number((*v).into())).collect()),
        );
    }
    t
}

/// 关闭时的最小块（避免误改用户其他 tun 字段）。
pub fn tun_off_block() -> Mapping {
    let mut t = Mapping::new();
    t.insert(Value::String("enable".into()), Value::Bool(false));
    t
}

/// 上游 DNS（对齐 clash-verge，抗封锁且本环境可达）：
/// - `system`：读系统 resolv.conf（mihomo 内建，doc/04）；
/// - DoH（走 443）与国内裸 DNS 兜底，替代裸 8.8.8.8/1.1.1.1（被墙时悬挂）。
const DNS_DEFAULT_NAMESERVER: &[&str] = &["system", "223.5.5.5", "119.29.29.29"];
const DNS_NAMESERVER: &[&str] = &["system", "https://dns.alidns.com/dns-query", "223.5.5.5"];

/// TUN 开启时配套的 DNS 块（§6.3 + 上游 nameserver）。
/// `mode` 取 `fake-ip` / `redir-host`（settings.tun_dns_mode，helper 侧已归一化）。
/// dns-hijack 劫持本地 53 后，mihomo 必须有可用的上游 DNS，否则域名解析失败=全断网。
pub fn build_dns_block(mode: &str) -> Mapping {
    let mut d = Mapping::new();
    kv(&mut d, "enable", true);
    kv(&mut d, "enhanced-mode", mode);
    if mode == "fake-ip" {
        kv(&mut d, "fake-ip-range", "198.18.0.1/16");
    }
    d.insert(
        Value::String("default-nameserver".into()),
        Value::Sequence(strs(DNS_DEFAULT_NAMESERVER)),
    );
    d.insert(
        Value::String("nameserver".into()),
        Value::Sequence(strs(DNS_NAMESERVER)),
    );
    d
}

fn strs(list: &[&str]) -> Vec<Value> {
    list.iter().map(|s| Value::String((*s).to_string())).collect()
}

fn kv(m: &mut Mapping, k: &str, v: impl Into<Value>) {
    m.insert(Value::String(k.into()), v.into());
}

// ---- §6.6 能力探测 ----

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

// ---- §5.6 SetTun ----

/// SetTun 结果。
#[derive(Debug)]
pub struct TunApply {
    /// 核心运行中且已 PATCH 热更（false = 核心未运行，仅落盘，启动时生效）
    pub hot_reloaded: bool,
    /// 已通过读回校验（仅 hot_reloaded=true 时有意义）
    pub verified: bool,
}

/// 开/关 TUN（§5.6）：编辑运行态 config.yaml 的 tun 块（原子落盘）→ 核心运行中则
/// `PATCH /configs` 字段级热更 → 回读校验（`GET /configs` + `ip link`/`ip rule`）→
/// 未生效回退（恢复原 yaml + PUT 全量重载）并报错。
///
/// 前置：`runtime_yaml` 已存在（先应用过配置）；`core_running` 表示核心进程状态。
/// 失败时磁盘与内存配置都回退到调用前的状态。
pub async fn set_tun(
    enable: bool,
    settings: &Settings,
    runtime_yaml: &Path,
    core_sock: &Path,
    core_running: bool,
    tools: &Tools,
) -> Result<TunApply, String> {
    if enable {
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
            tracing::warn!("SetTun: 检测到自身 TUN 残留，先清理再开启");
            cleanup_tun(tools).await;
        }
    }

    let original = std::fs::read_to_string(runtime_yaml)
        .map_err(|e| format!("读取运行态配置失败（请先应用配置）: {e}"))?;
    let mut doc: Value = serde_yaml_ng::from_str(&original)
        .map_err(|e| format!("解析运行态配置失败: {e}"))?;
    let block = if enable { build_tun_block(settings) } else { tun_off_block() };
    let dns_block = enable.then(|| build_dns_block(&settings.tun_dns_mode));
    let mapping = doc
        .as_mapping_mut()
        .ok_or_else(|| "运行态配置根必须是 mapping".to_string())?;
    mapping.insert(Value::String("tun".into()), Value::Mapping(block.clone()));
    // TUN 开启必须配完整 fake-ip DNS（劫持 53 后无上游 nameserver 会断网，见下）
    if let Some(d) = &dns_block {
        mapping.insert(Value::String("dns".into()), Value::Mapping(d.clone()));
    }
    let new_yaml = serde_yaml_ng::to_string(&doc).map_err(|e| e.to_string())?;

    write_yaml_atomic(runtime_yaml, &new_yaml).map_err(|e| e.to_string())?;

    if !core_running {
        return Ok(TunApply { hot_reloaded: false, verified: false });
    }

    // 热更：PUT /configs 内联完整配置（new_yaml 已含 tun 块 + TUN 开启时注入的 dns 块，
    // 否则 dns-hijack 劫持 53 后无上游 nameserver，fake-ip 无法解析真实域名=开 TUN 断网）。
    // 注意：mihomo 的 PATCH /configs 语义是「用 payload/path 整体重载」，并非字段级合并
    // （payload 为空回落磁盘默认配置、缺失字段取默认值），字段级 PATCH 会被忽略/破坏——
    // 故 TUN 开关一律走 PUT 内联完整 yaml（doc/04 §3 勘误）。
    if let Err(e) = crate::core::reload_config(core_sock, &new_yaml).await {
        rollback(runtime_yaml, core_sock, &original).await;
        return Err(format!("TUN 热更失败: {e}"));
    }

    // 回读校验（轮询等待网卡/规则生效，mihomo 异步创建）
    let verified = verify(enable, core_sock, tools).await;
    if !verified {
        rollback(runtime_yaml, core_sock, &original).await;
        return Err(format!(
            "TUN 回读校验未通过（{} 未生效），已回退并恢复原配置；请查看核心日志",
            if enable { "开启" } else { "关闭" }
        ));
    }
    Ok(TunApply { hot_reloaded: true, verified: true })
}

/// 回读校验：`GET /configs` 的 tun.enable + `ip link`/`ip rule`（R7.2）。
async fn verify(enable: bool, core_sock: &Path, tools: &Tools) -> bool {
    for _ in 0..20 {
        let mut ok = true;
        // GET /configs 回读
        match crate::core::get_configs(core_sock).await {
            Ok(cfg) => {
                let cur = cfg
                    .get("tun")
                    .and_then(|t| t.get("enable"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if cur != enable {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        if enable {
            if !link_up(tools, TUN_DEVICE).await {
                ok = false;
            }
            if !rule_range_present(tools).await {
                ok = false;
            }
        } else {
            // 关闭：校验网卡消失 + 规则区间消失
            if run(&tools.ip, &["-o", "link", "show", TUN_DEVICE]).await.ok() {
                ok = false;
            }
            if rule_range_present(tools).await {
                ok = false;
            }
        }
        if ok {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    false
}

/// 回退：恢复原 yaml 落盘 + 核心运行中则 PUT 全量重载（保持内存与磁盘一致）。
async fn rollback(runtime_yaml: &Path, core_sock: &Path, original: &str) {
    let _ = write_yaml_atomic(runtime_yaml, original);
    let _ = crate::core::reload_config(core_sock, original).await;
}

fn write_yaml_atomic(path: &Path, yaml: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("yaml.tmp");
    std::fs::write(&tmp, yaml)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    fn as_mapping(v: &Value) -> &Mapping {
        v.as_mapping().expect("mapping")
    }

    fn get<'a>(m: &'a Mapping, key: &str) -> Option<&'a Value> {
        m.get(&Value::String(key.into()))
    }

    #[test]
    fn build_tun_block_injects_managed_fields() {
        let block = build_tun_block(&settings());
        assert_eq!(get(&block, "enable").unwrap().as_bool(), Some(true));
        assert_eq!(get(&block, "device").unwrap().as_str(), Some("clard0"));
        assert_eq!(get(&block, "stack").unwrap().as_str(), Some("gvisor"));
        assert_eq!(get(&block, "auto-route").unwrap().as_bool(), Some(true));
        assert_eq!(get(&block, "iproute2-table-index").unwrap().as_i64(), Some(2023));
        assert_eq!(get(&block, "iproute2-rule-index").unwrap().as_i64(), Some(9100));
        assert_eq!(get(&block, "strict-route").unwrap().as_bool(), Some(false));
        assert_eq!(get(&block, "auto-redirect").unwrap().as_bool(), Some(false));
        // 空设置 → 默认 dns-hijack 与默认私网段
        let hijack = get(&block, "dns-hijack").unwrap().as_sequence().unwrap();
        assert_eq!(hijack.len(), 2);
        let exclude = get(&block, "route-exclude-address").unwrap().as_sequence().unwrap();
        assert_eq!(exclude.len(), DEFAULT_ROUTE_EXCLUDE.len());
        assert!(exclude.iter().any(|v| v.as_str() == Some("10.0.0.0/8")));
        // 空列表不注入 exclude-* 键
        assert!(get(&block, "exclude-uid").is_none());
        assert!(get(&block, "exclude-interface").is_none());
        assert!(get(&block, "exclude-dst-port").is_none());
    }

    #[test]
    fn build_tun_block_uses_user_settings() {
        let mut s = settings();
        s.tun_stack = "gvisor".into();
        s.dns_hijack = vec!["any:5353".into()];
        s.route_exclude_address = vec!["10.0.0.0/8".into()];
        s.exclude_uid = vec![1000, 1001];
        s.exclude_interface = vec!["eth1".into()];
        s.exclude_dst_port = vec![5353];
        s.strict_route = true;
        s.auto_redirect = true;
        let block = build_tun_block(&s);
        assert_eq!(get(&block, "stack").unwrap().as_str(), Some("gvisor"));
        let hijack = get(&block, "dns-hijack").unwrap().as_sequence().unwrap();
        assert_eq!(hijack.len(), 1);
        let exclude = get(&block, "route-exclude-address").unwrap().as_sequence().unwrap();
        assert_eq!(exclude.len(), 1, "用户自定义替换默认私网段");
        let uid = get(&block, "exclude-uid").unwrap().as_sequence().unwrap();
        assert_eq!(uid.len(), 2);
        assert_eq!(get(&block, "exclude-interface").unwrap().as_sequence().unwrap().len(), 1);
        assert_eq!(get(&block, "exclude-dst-port").unwrap().as_sequence().unwrap().len(), 1);
        assert_eq!(get(&block, "strict-route").unwrap().as_bool(), Some(true));
        assert_eq!(get(&block, "auto-redirect").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn build_dns_block_fakeip_injects_mode_and_nameservers() {
        let d = build_dns_block("fake-ip");
        assert_eq!(get(&d, "enable").unwrap().as_bool(), Some(true));
        assert_eq!(get(&d, "enhanced-mode").unwrap().as_str(), Some("fake-ip"));
        assert_eq!(get(&d, "fake-ip-range").unwrap().as_str(), Some("198.18.0.1/16"));
        let ns = get(&d, "nameserver").unwrap().as_sequence().unwrap();
        assert_eq!(ns[0].as_str(), Some("system"), "system 上游（读系统 resolv.conf）");
        let dn = get(&d, "default-nameserver").unwrap().as_sequence().unwrap();
        assert_eq!(dn[0].as_str(), Some("system"));
    }

    #[test]
    fn build_dns_block_redir_host_no_fakeip_range() {
        let d = build_dns_block("redir-host");
        assert_eq!(get(&d, "enhanced-mode").unwrap().as_str(), Some("redir-host"));
        assert_eq!(get(&d, "fake-ip-range"), None, "redir-host 不注入 fake-ip-range");
        assert_eq!(get(&d, "nameserver").unwrap().as_sequence().unwrap().len(), 3);
    }

    #[test]
    fn tun_off_block_only_enable_false() {
        let block = tun_off_block();
        assert_eq!(block.len(), 1);
        assert_eq!(get(&block, "enable").unwrap().as_bool(), Some(false));
    }

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

    #[tokio::test]
    async fn set_tun_core_stopped_persists_yaml_only() {
        let dir = tempdir().unwrap();
        let yaml_path = dir.path().join("config.yaml");
        std::fs::write(&yaml_path, "mode: rule\nmixed-port: 7890\ntun:\n  enable: false\n").unwrap();
        let tools = Tools::system();
        let out = set_tun(true, &settings(), &yaml_path, &dir.path().join("no.sock"), false, &tools)
            .await
            .unwrap();
        assert!(!out.hot_reloaded, "核心未运行 → 不热更");
        assert!(!out.verified);

        let doc: Value = serde_yaml_ng::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
        let tun = as_mapping(doc.get("tun").unwrap());
        assert_eq!(get(tun, "enable").unwrap().as_bool(), Some(true));
        assert_eq!(get(tun, "device").unwrap().as_str(), Some("clard0"), "托管块完整注入");
        // 开启时必须注入 dns（劫持 53 后无上游 nameserver 会断网）；默认 fake-ip 模式
        let dns = as_mapping(doc.get("dns").unwrap());
        assert_eq!(get(dns, "enable").unwrap().as_bool(), Some(true));
        assert_eq!(get(dns, "enhanced-mode").unwrap().as_str(), Some("fake-ip"));
        let ns = get(dns, "nameserver").unwrap().as_sequence().unwrap();
        assert_eq!(ns[0].as_str(), Some("system"), "system 上游（读系统 resolv.conf）");
        // 其他顶层字段不受影响
        assert_eq!(as_mapping(&doc).get(&Value::String("mode".into())).unwrap().as_str(), Some("rule"));

        // 关闭：只写 enable:false
        set_tun(false, &settings(), &yaml_path, &dir.path().join("no.sock"), false, &tools)
            .await
            .unwrap();
        let doc: Value = serde_yaml_ng::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
        let tun = as_mapping(doc.get("tun").unwrap());
        assert_eq!(tun.len(), 1);
        assert_eq!(get(tun, "enable").unwrap().as_bool(), Some(false));
    }

    #[tokio::test]
    async fn set_tun_missing_runtime_yaml_errors() {
        let dir = tempdir().unwrap();
        let tools = Tools::system();
        let e = set_tun(true, &settings(), &dir.path().join("nope.yaml"), &dir.path().join("no.sock"), false, &tools)
            .await
            .unwrap_err();
        assert!(e.contains("读取运行态配置失败"), "{e}");
    }

    #[tokio::test]
    async fn set_tun_enable_true_cleans_own_residual_when_core_stopped() {
        // fake ip：报告 clard0 存在、9100 rule 存在 → 应触发 cleanup 再写 yaml
        let dir = tempdir().unwrap();
        let fake = write_fake_ip(&dir, r#"#!/bin/sh
if [ "$1" = "-V" ]; then echo "iproute2-ss230000"; exit 0; fi
if [ "$1" = "-o" ] && [ "$2" = "link" ] && [ "$3" = "show" ] && [ "$4" = "clard0" ]; then
  echo "3: clard0: <POINTOPOINT,UP> mtu 1500 ... link/none"; exit 0
fi
if [ "$1" = "rule" ] && [ -z "$2" ]; then echo "9100: from all lookup 2023"; exit 0; fi
if [ "$1" = "rule" ] && [ "$2" = "del" ]; then exit 0; fi
if [ "$1" = "link" ] && [ "$2" = "del" ]; then exit 0; fi
if [ "$1" = "-4" ] && [ "$2" = "route" ] && [ "$3" = "flush" ]; then exit 0; fi
if [ "$1" = "-6" ] && [ "$2" = "route" ] && [ "$3" = "flush" ]; then exit 0; fi
if [ "$1" = "-4" ] && [ "$2" = "route" ] && [ "$3" = "show" ]; then exit 0; fi
if [ "$1" = "-6" ] && [ "$2" = "route" ] && [ "$3" = "show" ]; then exit 0; fi
exit 1"#);
        let yaml_path = dir.path().join("config.yaml");
        std::fs::write(&yaml_path, "tun:\n  enable: false\n").unwrap();
        let tools = Tools {
            ip: fake,
            nft: dir.path().join("nft").to_path_buf(),
            resolvectl: dir.path().join("resolvectl").to_path_buf(),
        };
        set_tun(true, &settings(), &yaml_path, &dir.path().join("no.sock"), false, &tools)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn set_tun_enable_true_conflict_when_core_running_and_identifiers_busy() {
        let dir = tempdir().unwrap();
        let fake = write_fake_ip(&dir, r#"#!/bin/sh
if [ "$1" = "-V" ]; then echo "iproute2-ss230000"; exit 0; fi
if [ "$1" = "-o" ] && [ "$2" = "link" ] && [ "$3" = "show" ] && [ "$4" = "clard0" ]; then
  echo "3: clard0: <POINTOPOINT,UP> mtu 1500 ... link/none"; exit 0
fi
exit 1"#);
        let yaml_path = dir.path().join("config.yaml");
        std::fs::write(&yaml_path, "tun:\n  enable: false\n").unwrap();
        let tools = Tools {
            ip: fake,
            nft: dir.path().join("nft").to_path_buf(),
            resolvectl: dir.path().join("resolvectl").to_path_buf(),
        };
        let e = set_tun(true, &settings(), &yaml_path, &dir.path().join("no.sock"), true, &tools)
            .await
            .unwrap_err();
        assert!(e.contains("标识被占用"), "{e}");
    }

    #[tokio::test]
    async fn set_tun_rolls_back_on_patch_failure() {
        // fake ip 全失败 + core sock 指向不存在的 socket → PATCH 失败 → yaml 恢复原样
        let dir = tempdir().unwrap();
        let fake = write_fake_ip(&dir, "#!/bin/sh\nif [ \"$1\" = \"-V\" ]; then echo \"iproute2-ss230000\"; exit 0; fi\nexit 1\n");
        let yaml_path = dir.path().join("config.yaml");
        let original = "mode: rule\ntun:\n  enable: false\n";
        std::fs::write(&yaml_path, original).unwrap();
        let tools = Tools {
            ip: fake,
            nft: dir.path().join("nft").to_path_buf(),
            resolvectl: dir.path().join("resolvectl").to_path_buf(),
        };
        let e = set_tun(true, &settings(), &yaml_path, &dir.path().join("no.sock"), true, &tools)
            .await
            .unwrap_err();
        assert!(e.contains("TUN 热更失败"), "{e}");
        assert_eq!(
            std::fs::read_to_string(&yaml_path).unwrap(),
            original,
            "失败回退：yaml 恢复原样"
        );
    }

    /// 写一个可执行 fake 脚本，返回其路径。
    fn write_fake_ip(dir: &tempfile::TempDir, script: &str) -> PathBuf {
        let path = dir.path().join("ip");
        std::fs::write(&path, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

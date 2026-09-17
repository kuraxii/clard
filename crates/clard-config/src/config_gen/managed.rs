//! 托管字段（doc/01 §6.3/§6.7）：由应用注入、覆盖用户值，防止用户改坏固定标识。
//!
//! 注入项：
//! - 顶层：`mixed-port` / `bind-address` / `allow-lan` / `log-level` / `mode` /
//!   `external-controller-unix`（§8 注入清单）；
//! - TUN 开：`tun` 整块（device/stack/auto-route/dns-hijack/iproute2-table-index/
//!   iproute2-rule-index/strict-route/auto-redirect/route-exclude-address）+ `dns` 强制
//!   fake-ip（§6.3，避免 DNS 泄漏）+ DNS 页签字段覆盖（R7.2.1，空值回退 clard 默认）；
//! - TUN 关：仅注入 `tun.enable: false`（防止用户配置私自开启 TUN）；DNS 页签字段
//!   在用户配置过时合并注入（enable 取用户值，不强制 fake-ip）。

use std::path::PathBuf;

use serde_yaml_ng::{Mapping, Value};

/// 默认私网排除段（§6.7，用户可追加）
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

/// 配置生成选项（托管字段值，§6.3）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigGenOptions {
    /// `rule` / `global` / `direct`
    pub mode: String,
    pub log_level: String,
    pub mixed_port: u16,
    /// 仅绑回环，保证 mixed-port 不暴露到局域网
    pub bind_address: String,
    pub allow_lan: bool,
    /// helper 指定的 core 控制面路径（不开 TCP controller）
    pub external_controller_unix: PathBuf,
    /// `None` = TUN 关闭（注入 `tun.enable: false`）
    pub tun: Option<TunOptions>,
    /// DNS 页签托管字段（R7.2.1，TUN 开时与强制字段合并）
    pub dns: DnsOptions,
}

impl Default for ConfigGenOptions {
    fn default() -> Self {
        Self {
            mode: "rule".into(),
            log_level: "info".into(),
            mixed_port: 7890,
            bind_address: "127.0.0.1".into(),
            allow_lan: false,
            external_controller_unix: PathBuf::from("/run/clard/core.sock"),
            tun: Some(TunOptions::default()),
            dns: DnsOptions::default(),
        }
    }
}

/// clard 默认 DNS 上游（TUN 下 dns-hijack 劫持 53 后必须有上游，否则域名解析失败=全断网）。
pub const DEFAULT_NAMESERVER: &[&str] = &["tls://223.5.5.5", "tls://1.12.12.12"];
/// clard 默认 default-nameserver（解析域名型上游用的纯 IP）。
pub const DEFAULT_DNS_NAMESERVER: &[&str] = &["223.5.5.5", "119.29.29.29"];

/// DNS 页签托管字段（doc/05 R7.2.1）：TUN 开时与强制字段合并注入 `dns` 块；
/// TUN 关时仅在用户配置过 DNS 字段时合并注入（enable 取用户值，不强制 fake-ip）。
/// `nameserver_policy` 条目格式 `domain=dns1,dns2`；`hosts` 条目格式 `domain=ip`（注入顶层 hosts）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsOptions {
    pub enable: bool,
    /// fake-ip-filter-mode：blacklist / whitelist / rule（默认 blacklist）
    pub fake_ip_filter_mode: String,
    /// fake-ip-filter 域名列表（`*.` 通配；仅 fake-ip 模式生效）
    pub fake_ip_filter: Vec<String>,
    /// 查询时先查顶层 hosts（默认 true）
    pub use_hosts: bool,
    /// 额外读取系统 /etc/hosts（默认 true）
    pub use_system_hosts: bool,
    /// nameserver-policy 条目（`domain=dns1,dns2`）
    pub nameserver_policy: Vec<String>,
    /// hosts 静态映射条目（`domain=ip`）
    pub hosts: Vec<String>,
    /// 全局上游；空 = clard 默认 `DEFAULT_NAMESERVER`
    pub nameserver: Vec<String>,
    /// 纯 IP 上游；空 = clard 默认 `DEFAULT_DNS_NAMESERVER`
    pub default_nameserver: Vec<String>,
}

impl Default for DnsOptions {
    fn default() -> Self {
        Self {
            enable: false,
            fake_ip_filter_mode: "blacklist".into(),
            fake_ip_filter: Vec::new(),
            use_hosts: true,
            use_system_hosts: true,
            nameserver_policy: Vec::new(),
            hosts: Vec::new(),
            nameserver: Vec::new(),
            default_nameserver: Vec::new(),
        }
    }
}

/// TUN 托管字段（§6.2/§6.3）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunOptions {
    pub device: String,
    pub stack: String,
    pub table_index: i64,
    pub rule_index: i64,
    pub auto_route: bool,
    pub auto_detect_interface: bool,
    pub dns_hijack: Vec<String>,
    /// TUN 下 DNS 模式：fake-ip / redir-host（对齐 helper settings.tun_dns_mode）
    pub dns_mode: String,
    pub strict_route: bool,
    pub auto_redirect: bool,
    pub route_exclude_address: Vec<String>,
}

impl Default for TunOptions {
    fn default() -> Self {
        Self {
            device: "clard0".into(),
            stack: "gvisor".into(),
            table_index: 2023,
            rule_index: 9100,
            auto_route: true,
            auto_detect_interface: true,
            dns_hijack: vec!["any:53".into(), "tcp://any:53".into()],
            dns_mode: "fake-ip".into(),
            strict_route: false,
            auto_redirect: false,
            route_exclude_address: DEFAULT_ROUTE_EXCLUDE.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// 注入托管字段（覆盖用户值）。
pub fn inject(doc: &mut Mapping, options: &ConfigGenOptions) {
    kv(doc, "mixed-port", i64::from(options.mixed_port));
    kv(doc, "bind-address", options.bind_address.as_str());
    kv(doc, "allow-lan", options.allow_lan);
    kv(doc, "log-level", options.log_level.as_str());
    kv(doc, "mode", options.mode.as_str());
    kv(
        doc,
        "external-controller-unix",
        options.external_controller_unix.to_string_lossy().as_ref(),
    );
    // geo 数据（§6.3/§8）：随 RPM 分发到核心 -d 目录 /var/clard/lib/runtime（mihomo 只在
    // -d 目录找 MMDB，geodata-path 不生效），TUI 可更新（UpdateGeoData）；
    // geo-auto-update 关闭——避免 mihomo 启动时自连 GitHub 下载 geodata 卡死（github 被墙场景）
    kv(doc, "geo-auto-update", false);

    match &options.tun {
        Some(tun) => {
            // TUN 下强制 DNS 块（保留用户 dns 的其他字段）+ 上游 nameserver：
            // dns-hijack 劫持 53 后必须有上游 DNS，否则域名解析失败=全断网。
            // 模式取 tun.dns_mode（fake-ip / redir-host，对齐 helper build_dns_block）。
            let mut dns = existing_dns(doc);
            dns.insert(Value::String("enable".into()), Value::Bool(true));
            let mode = tun.dns_mode.as_str();
            dns.insert(Value::String("enhanced-mode".into()), Value::String(mode.into()));
            if mode == "fake-ip" {
                dns.insert(
                    Value::String("fake-ip-range".into()),
                    Value::String("198.18.0.1/16".into()),
                );
            }
            // DNS 页签字段覆盖（R7.2.1）；nameserver/default-nameserver 空值回退 clard 默认
            apply_dns_overlay(&mut dns, &options.dns, true);
            doc.insert(Value::String("dns".into()), Value::Mapping(dns));

            let mut t = Mapping::new();
            kv(&mut t, "enable", true);
            kv(&mut t, "device", tun.device.as_str());
            kv(&mut t, "stack", tun.stack.as_str());
            kv(&mut t, "auto-route", tun.auto_route);
            kv(&mut t, "auto-detect-interface", tun.auto_detect_interface);
            kv(&mut t, "dns-hijack", Value::Sequence(strings(&tun.dns_hijack)));
            kv(&mut t, "iproute2-table-index", tun.table_index);
            kv(&mut t, "iproute2-rule-index", tun.rule_index);
            kv(&mut t, "strict-route", tun.strict_route);
            kv(&mut t, "auto-redirect", tun.auto_redirect);
            kv(
                &mut t,
                "route-exclude-address",
                Value::Sequence(strings(&tun.route_exclude_address)),
            );
            doc.insert(Value::String("tun".into()), Value::Mapping(t));
        }
        None => {
            // TUN 关：用户配置过 DNS 页签字段时才合并注入（enable 取用户值，不强制 fake-ip）
            if dns_active(&options.dns) {
                let mut dns = existing_dns(doc);
                dns.insert(Value::String("enable".into()), Value::Bool(options.dns.enable));
                apply_dns_overlay(&mut dns, &options.dns, false);
                doc.insert(Value::String("dns".into()), Value::Mapping(dns));
            }
            // 覆盖用户可能自带的 tun 块，仅保留 enable:false
            let mut t = Mapping::new();
            kv(&mut t, "enable", false);
            doc.insert(Value::String("tun".into()), Value::Mapping(t));
        }
    }
    // 顶层 hosts（R7.2.1）：与 profile 自带 hosts 合并，clard 条目覆盖同名
    inject_hosts(doc, &options.dns.hosts);
}

/// 读取 doc 中已有的 dns 块（保留 profile 自带的用户字段）。
fn existing_dns(doc: &Mapping) -> Mapping {
    doc.get(Value::String("dns".into()))
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default()
}

/// DNS 页签字段是否有非默认值（TUN 关时据此决定是否注入 dns 块）。
fn dns_active(d: &DnsOptions) -> bool {
    d.enable
        || d.fake_ip_filter_mode != "blacklist"
        || !d.fake_ip_filter.is_empty()
        || !d.use_hosts
        || !d.use_system_hosts
        || !d.nameserver_policy.is_empty()
        || !d.hosts.is_empty()
        || !d.nameserver.is_empty()
        || !d.default_nameserver.is_empty()
}

/// 把 DNS 页签字段合并进 dns 块。`force_defaults`：TUN 开时 nameserver/
/// default-nameserver 空值回退 clard 默认（劫持 53 后必须有上游）。
fn apply_dns_overlay(dns: &mut Mapping, d: &DnsOptions, force_defaults: bool) {
    if !d.default_nameserver.is_empty() {
        dns.insert(
            Value::String("default-nameserver".into()),
            Value::Sequence(strings(&d.default_nameserver)),
        );
    } else if force_defaults {
        dns.insert(
            Value::String("default-nameserver".into()),
            Value::Sequence(strings(&strings_of(DEFAULT_DNS_NAMESERVER))),
        );
    }
    if !d.nameserver.is_empty() {
        dns.insert(
            Value::String("nameserver".into()),
            Value::Sequence(strings(&d.nameserver)),
        );
    } else if force_defaults {
        dns.insert(
            Value::String("nameserver".into()),
            Value::Sequence(strings(&strings_of(DEFAULT_NAMESERVER))),
        );
    }
    if d.fake_ip_filter_mode != "blacklist" {
        dns.insert(
            Value::String("fake-ip-filter-mode".into()),
            Value::String(d.fake_ip_filter_mode.clone()),
        );
    }
    if !d.fake_ip_filter.is_empty() {
        dns.insert(
            Value::String("fake-ip-filter".into()),
            Value::Sequence(strings(&d.fake_ip_filter)),
        );
    }
    if !d.use_hosts {
        dns.insert(Value::String("use-hosts".into()), Value::Bool(false));
    }
    if !d.use_system_hosts {
        dns.insert(Value::String("use-system-hosts".into()), Value::Bool(false));
    }
    if !d.nameserver_policy.is_empty() {
        dns.insert(
            Value::String("nameserver-policy".into()),
            Value::Mapping(policy_mapping(&d.nameserver_policy)),
        );
    }
}

/// 顶层 hosts 注入：与 profile 自带 hosts 合并，clard 条目覆盖同名（`domain=ip`）。
fn inject_hosts(doc: &mut Mapping, entries: &[String]) {
    if entries.is_empty() {
        return;
    }
    let mut hosts = doc
        .get(Value::String("hosts".into()))
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default();
    for e in entries {
        if let Some((k, v)) = e.split_once('=') {
            let k = k.trim();
            let v = v.trim();
            if !k.is_empty() && !v.is_empty() {
                hosts.insert(Value::String(k.into()), Value::String(v.into()));
            }
        }
    }
    doc.insert(Value::String("hosts".into()), Value::Mapping(hosts));
}

/// `domain=dns1,dns2` 条目 → nameserver-policy 映射（值仅一项时为字符串，多项为序列）。
fn policy_mapping(entries: &[String]) -> Mapping {
    let mut m = Mapping::new();
    for e in entries {
        let Some((k, v)) = e.split_once('=') else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        let vals: Vec<Value> = v
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| Value::String(s.to_string()))
            .collect();
        if vals.is_empty() {
            continue;
        }
        let mut vals = vals;
        let value = if vals.len() == 1 {
            match vals.pop() {
                Some(v) => v,
                // len==1 时 pop 必有值
                None => continue,
            }
        } else {
            Value::Sequence(vals)
        };
        m.insert(Value::String(key.into()), value);
    }
    m
}

fn strings(list: &[String]) -> Vec<Value> {
    list.iter().map(|s| Value::String(s.clone())).collect()
}

fn strings_of(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn kv(m: &mut Mapping, k: &str, v: impl Into<Value>) {
    m.insert(Value::String(k.into()), v.into());
}

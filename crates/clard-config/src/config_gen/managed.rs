//! 托管字段（doc/01 §6.3/§6.7）：由应用注入、覆盖用户值，防止用户改坏固定标识。
//!
//! 注入项：
//! - 顶层：`mixed-port` / `bind-address` / `allow-lan` / `log-level` / `mode` /
//!   `external-controller-unix`（§8 注入清单）；
//! - TUN 开：`tun` 整块（device/stack/auto-route/dns-hijack/iproute2-table-index/
//!   iproute2-rule-index/strict-route/auto-redirect/route-exclude-address）+ `dns` 强制
//!   fake-ip（§6.3，避免 DNS 泄漏）；
//! - TUN 关：仅注入 `tun.enable: false`，防止用户配置私自开启 TUN。

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
    /// 该本地用户不被接管（§6.7）
    pub exclude_uid: Vec<u32>,
    /// 该网卡不参与（§6.7）
    pub exclude_interface: Vec<String>,
    /// 该目的端口不参与（§6.7）
    pub exclude_dst_port: Vec<u16>,
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
            route_exclude_address: DEFAULT_ROUTE_EXCLUDE
                .iter()
                .map(|s| s.to_string())
                .collect(),
            exclude_uid: Vec::new(),
            exclude_interface: Vec::new(),
            exclude_dst_port: Vec::new(),
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
            let mut dns = doc
                .get(Value::String("dns".into()))
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default();
            dns.insert(Value::String("enable".into()), Value::Bool(true));
            let mode = tun.dns_mode.as_str();
            dns.insert(Value::String("enhanced-mode".into()), Value::String(mode.into()));
            if mode == "fake-ip" {
                dns.insert(
                    Value::String("fake-ip-range".into()),
                    Value::String("198.18.0.1/16".into()),
                );
            }
            dns.insert(
                Value::String("default-nameserver".into()),
                Value::Sequence(
                    ["223.5.5.5", "119.29.29.29"]
                        .iter()
                        .map(|s| Value::String((*s).to_string()))
                        .collect(),
                ),
            );
            dns.insert(
                Value::String("nameserver".into()),
                Value::Sequence(
                    ["tls://223.5.5.5", "tls://1.12.12.12"]
                        .iter()
                        .map(|s| Value::String((*s).to_string()))
                        .collect(),
                ),
            );
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
            kv(&mut t, "route-exclude-address", Value::Sequence(strings(&tun.route_exclude_address)));
            if !tun.exclude_uid.is_empty() {
                kv(&mut t, "exclude-uid", Value::Sequence(tun.exclude_uid.iter().map(|v| Value::Number((*v).into())).collect()));
            }
            if !tun.exclude_interface.is_empty() {
                kv(&mut t, "exclude-interface", Value::Sequence(strings(&tun.exclude_interface)));
            }
            if !tun.exclude_dst_port.is_empty() {
                kv(&mut t, "exclude-dst-port", Value::Sequence(tun.exclude_dst_port.iter().map(|v| Value::Number((*v).into())).collect()));
            }
            doc.insert(Value::String("tun".into()), Value::Mapping(t));
        }
        None => {
            // 覆盖用户可能自带的 tun 块，仅保留 enable:false
            let mut t = Mapping::new();
            kv(&mut t, "enable", false);
            doc.insert(Value::String("tun".into()), Value::Mapping(t));
        }
    }
}

fn strings(list: &[String]) -> Vec<Value> {
    list.iter().map(|s| Value::String(s.clone())).collect()
}

fn kv(m: &mut Mapping, k: &str, v: impl Into<Value>) {
    m.insert(Value::String(k.into()), v.into());
}

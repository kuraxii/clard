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
    pub strict_route: bool,
    pub auto_redirect: bool,
    pub route_exclude_address: Vec<String>,
}

impl Default for TunOptions {
    fn default() -> Self {
        Self {
            device: "clard0".into(),
            stack: "system".into(),
            table_index: 2023,
            rule_index: 9100,
            auto_route: true,
            auto_detect_interface: true,
            dns_hijack: vec!["any:53".into(), "tcp://any:53".into()],
            strict_route: false,
            auto_redirect: false,
            route_exclude_address: DEFAULT_ROUTE_EXCLUDE
                .iter()
                .map(|s| s.to_string())
                .collect(),
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

    match &options.tun {
        Some(tun) => {
            // TUN 下强制 DNS fake-ip（保留用户 dns 的其他字段）
            let mut dns = doc
                .get(&Value::String("dns".into()))
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default();
            dns.insert(Value::String("enable".into()), Value::Bool(true));
            dns.insert(Value::String("enhanced-mode".into()), Value::String("fake-ip".into()));
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

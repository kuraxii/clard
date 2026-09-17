//! config_gen：订阅内容 → 运行时 mihomo 配置（TUI 侧，doc/01 §8）
//!
//! 链路：归一化（base64 → 文本，订阅商常见格式）→ 解析（yaml 或节点列表转换）
//!      → 深合并（全局 base + profile）→ 注入托管字段（覆盖用户值）→ 序列化 runtime yaml。
//!
//! 产物随后经 IPC `ApplyConfig` 投递 helper（§5.5，`PUT /configs` 内联重载，见 doc/04 §3）。

pub mod convert;
pub mod managed;
pub mod merge;
pub mod normalize;

use serde_yaml_ng::{Mapping, Value};
use thiserror::Error;

pub use managed::{ConfigGenOptions, TunOptions};
pub use merge::deep_merge;

/// 配置生成失败
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigGenError {
    #[error("订阅内容为空")]
    Empty,
    #[error("订阅内容既不是 YAML 配置也不是可识别的节点列表")]
    NotAConfig,
    #[error("不支持的节点协议: {0}（本期仅支持 vless）")]
    UnsupportedScheme(String),
    #[error("节点行解析失败: {0}")]
    InvalidNode(String),
    #[error("YAML 解析失败: {0}")]
    Yaml(String),
}

/// 生成运行时 mihomo 配置文本。
///
/// - `profile`：订阅原始内容（可能为 base64）；
/// - `base`：全局基础配置（可空；将来来自 `clard.toml` 的全局 merge，§8）；
/// - `options`：托管字段（§6.3，覆盖用户值）。
pub fn generate(
    profile: &str,
    base: Option<&str>,
    options: &ConfigGenOptions,
) -> Result<String, ConfigGenError> {
    let text = normalize::normalize(profile)?;
    let mut doc = parse_subscription(&text)?;
    if let Some(base) = base {
        let base_val = parse_yaml(base, "全局基础配置")?;
        doc = deep_merge(&base_val, &doc);
    }
    let mapping = match &mut doc {
        Value::Mapping(m) => m,
        other => return Err(ConfigGenError::Yaml(format!("配置根必须是 mapping，实际是 {other:?}"))),
    };
    managed::inject(mapping, options);
    ensure_default_routing(mapping);
    serde_yaml_ng::to_string(&doc).map_err(|e| ConfigGenError::Yaml(e.to_string()))
}

/// 订阅只含节点（无 `rules`，纯节点列表）时补默认分组与规则：
/// 否则 `mode: rule` + 空规则 → 全部直连，外网不可达。
/// 默认规则对齐 clash-verge 经典默认：私网/国内直连，其余走 `GLOBAL` 组（doc/01 §8.3 A）。
fn ensure_default_routing(m: &mut Mapping) {
    // 订阅自带规则则不干预（其自有路由/分组保留）
    let has_rules = m
        .get(Value::String("rules".into()))
        .and_then(Value::as_sequence)
        .is_some_and(|s| !s.is_empty());
    if has_rules {
        return;
    }
    // 收集节点名（供 GLOBAL 组引用）
    let names: Vec<String> = m
        .get(Value::String("proxies".into()))
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(|p| p.get(Value::String("name".into())).and_then(Value::as_str))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if names.is_empty() {
        return; // 无节点可路由，留给 mihomo 兜底
    }
    // 分组：未定义 proxy-groups 时注入 GLOBAL（select：全部节点 + DIRECT，TUI 可切换）
    let has_groups = m
        .get(Value::String("proxy-groups".into()))
        .and_then(Value::as_sequence)
        .is_some_and(|s| !s.is_empty());
    if !has_groups {
        let mut members: Vec<Value> = names.iter().cloned().map(Value::String).collect();
        members.push(Value::String("DIRECT".into()));
        let mut group = Mapping::new();
        group.insert(Value::String("name".into()), Value::String("GLOBAL".into()));
        group.insert(Value::String("type".into()), Value::String("select".into()));
        group.insert(Value::String("proxies".into()), Value::Sequence(members));
        m.insert(
            Value::String("proxy-groups".into()),
            Value::Sequence(vec![Value::Mapping(group)]),
        );
    }
    // 默认规则（clash-verge 经典默认）：私网/国内直连，其余走 GLOBAL
    m.insert(
        Value::String("rules".into()),
        Value::Sequence(vec![
            Value::String("GEOIP,private,DIRECT,no-resolve".into()),
            Value::String("GEOIP,CN,DIRECT".into()),
            Value::String("MATCH,GLOBAL".into()),
        ]),
    );
}

/// 订阅原始内容 → **可存储的 yaml**（提交 `ProfileImport` 前用，§7.1）：
/// base64 解码；节点列表转 `proxies:`；已是 yaml 则原样。不做 merge/托管注入。
pub fn subscription_to_yaml(raw: &str) -> Result<String, ConfigGenError> {
    let text = normalize::normalize(raw)?;
    let doc = parse_subscription(&text)?;
    serde_yaml_ng::to_string(&doc).map_err(|e| ConfigGenError::Yaml(e.to_string()))
}

fn parse_subscription(text: &str) -> Result<Value, ConfigGenError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(ConfigGenError::Empty);
    }
    // 先按 YAML 解析；mapping 直接用，否则看是否节点列表
    if let Ok(v) = serde_yaml_ng::from_str::<Value>(t) {
        if v.is_mapping() {
            return Ok(v);
        }
    }
    if convert::looks_like_node_list(t) {
        return convert::node_list_to_yaml(t);
    }
    Err(ConfigGenError::NotAConfig)
}

fn parse_yaml(text: &str, what: &str) -> Result<Value, ConfigGenError> {
    serde_yaml_ng::from_str::<Value>(text)
        .map_err(|e| ConfigGenError::Yaml(format!("{what}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ConfigGenOptions {
        ConfigGenOptions::default()
    }

    fn as_mapping(yaml: &str) -> serde_yaml_ng::Mapping {
        let v: Value = serde_yaml_ng::from_str(yaml).expect("valid yaml");
        v.as_mapping().expect("mapping").clone()
    }

    #[test]
    fn generate_injects_managed_fields() {
        let profile = "proxies:\n  - name: n1\n    type: socks5\n    server: 1.2.3.4\n    port: 1080\n";
        let out = generate(profile, None, &opts()).unwrap();
        let m = as_mapping(&out);
        assert_eq!(get(&m, "mode").unwrap().as_str(), Some("rule"));
        assert_eq!(get(&m, "mixed-port").unwrap().as_i64(), Some(7890));
        assert_eq!(get(&m, "bind-address").unwrap().as_str(), Some("127.0.0.1"));
        assert_eq!(get(&m, "allow-lan").unwrap().as_bool(), Some(false));
        assert_eq!(
            get(&m, "external-controller-unix").unwrap().as_str(),
            Some("/run/clard/core.sock")
        );
        // geo 数据随包分发到核心 -d 目录（mihomo 只在 -d 目录找 MMDB）；关闭 mihomo 自更新
        // （防 github 被墙时启动卡死，改由 TUI g 更新）
        assert_eq!(get(&m, "geo-auto-update").unwrap().as_bool(), Some(false));
        // 用户配置的 proxies 原样保留
        assert!(get(&m, "proxies").unwrap().is_sequence());
    }

    #[test]
    fn generate_injects_mode_from_options() {
        // mode 为托管注入：settings.mode 三种取值直达 yaml（global/direct 生效，见 doc/05 §7 R7.1）
        let profile = "proxies:\n  - name: n1\n    type: socks5\n    server: 1.2.3.4\n    port: 1080\n";
        for m in ["rule", "global", "direct"] {
            let mut o = opts();
            o.mode = m.into();
            let out = generate(profile, None, &o).unwrap();
            let m_out = as_mapping(&out);
            assert_eq!(get(&m_out, "mode").unwrap().as_str(), Some(m), "mode={m} 注入");
        }
    }

    #[test]
    fn generate_managed_beats_profile() {
        let profile = "mixed-port: 8888\nmode: direct\nproxies: []\n";
        let out = generate(profile, None, &opts()).unwrap();
        let m = as_mapping(&out);
        assert_eq!(get(&m, "mixed-port").unwrap().as_i64(), Some(7890), "托管字段覆盖用户值");
        assert_eq!(get(&m, "mode").unwrap().as_str(), Some("rule"));
    }

    #[test]
    fn generate_tun_enabled_injects_full_block_and_fakeip_dns() {
        let out = generate("proxies: []\n", None, &opts()).unwrap();
        let m = as_mapping(&out);
        let tun = get(&m, "tun").unwrap().as_mapping().unwrap();
        assert_eq!(get(tun, "enable").unwrap().as_bool(), Some(true));
        assert_eq!(get(tun, "device").unwrap().as_str(), Some("clard0"));
        assert_eq!(get(tun, "iproute2-table-index").unwrap().as_i64(), Some(2023));
        assert_eq!(get(tun, "iproute2-rule-index").unwrap().as_i64(), Some(9100));
        assert_eq!(get(tun, "strict-route").unwrap().as_bool(), Some(false));
        assert_eq!(get(tun, "auto-redirect").unwrap().as_bool(), Some(false));
        let exclude = get(tun, "route-exclude-address").unwrap().as_sequence().unwrap();
        assert!(exclude.iter().any(|v| v.as_str() == Some("10.0.0.0/8")));
        assert!(exclude.iter().any(|v| v.as_str() == Some("::1/128")));
        assert!(get(tun, "exclude-uid").is_none(), "exclude-* 已移除，不注入");
        let dns = get(&m, "dns").unwrap().as_mapping().unwrap();
        assert_eq!(get(dns, "enable").unwrap().as_bool(), Some(true));
        assert_eq!(get(dns, "enhanced-mode").unwrap().as_str(), Some("fake-ip"));
        assert_eq!(get(dns, "fake-ip-range").unwrap().as_str(), Some("198.18.0.1/16"));
        let ns = get(dns, "nameserver").unwrap().as_sequence().unwrap();
        assert_eq!(ns[0].as_str(), Some("tls://223.5.5.5"), "TUN 下必须有上游 nameserver（劫持 53 后解析依赖）");
        let dn = get(dns, "default-nameserver").unwrap().as_sequence().unwrap();
        assert_eq!(dn[0].as_str(), Some("223.5.5.5"));
    }

    #[test]
    fn generate_tun_redir_host_dns_no_fakeip_range() {
        let mut options = opts();
        let mut tun = options.tun.clone().unwrap();
        tun.dns_mode = "redir-host".into();
        options.tun = Some(tun);
        let out = generate("proxies: []\n", None, &options).unwrap();
        let m = as_mapping(&out);
        let dns = get(&m, "dns").unwrap().as_mapping().unwrap();
        assert_eq!(get(dns, "enhanced-mode").unwrap().as_str(), Some("redir-host"));
        assert_eq!(get(dns, "fake-ip-range"), None, "redir-host 不注入 fake-ip-range");
    }

    #[test]
    fn generate_tun_disabled_forces_enable_false() {
        let mut options = opts();
        options.tun = None;
        let out = generate("proxies: []\n", None, &options).unwrap();
        let m = as_mapping(&out);
        let tun = get(&m, "tun").unwrap().as_mapping().unwrap();
        assert_eq!(get(tun, "enable").unwrap().as_bool(), Some(false));
        assert!(get(&m, "dns").is_none(), "TUN 关闭时不强制 fake-ip dns");
    }

    #[test]
    fn generate_merges_base_then_profile() {
        let base = "mode: global\nipv6: false\nlog-level: debug\n";
        let profile = "ipv6: true\nproxies: []\n";
        let out = generate(profile, Some(base), &opts()).unwrap();
        let m = as_mapping(&out);
        assert_eq!(get(&m, "mode").unwrap().as_str(), Some("rule"), "托管 mode 优先级最高");
        assert_eq!(get(&m, "ipv6").unwrap().as_bool(), Some(true), "profile 覆盖 base");
        assert_eq!(get(&m, "log-level").unwrap().as_str(), Some("info"), "托管 log-level 覆盖 base");
    }

    #[test]
    fn generate_base64_node_list_end_to_end() {
        // 真实订阅形态：base64 的 vless:// 节点列表
        let node = "vless://9ed7bc52-438e-48ae-a9ee-ed5c6f3df97e@hk.example:8443?encryption=none&flow=xtls-rprx-vision&type=tcp&security=reality&sni=www.cloudflare.com&fp=chrome&pbk=KEY&sid=0123456789abcdef#%E9%A6%99%E6%B8%AF-01";
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(node);
        let out = generate(&b64, None, &opts()).unwrap();
        let m = as_mapping(&out);
        let proxies = get(&m, "proxies").unwrap().as_sequence().unwrap();
        assert_eq!(proxies.len(), 1);
        let p = proxies[0].as_mapping().unwrap();
        assert_eq!(get(p, "type").unwrap().as_str(), Some("vless"));
        assert_eq!(get(p, "server").unwrap().as_str(), Some("hk.example"));
        assert_eq!(get(p, "port").unwrap().as_i64(), Some(8443));
        assert_eq!(get(p, "uuid").unwrap().as_str(), Some("9ed7bc52-438e-48ae-a9ee-ed5c6f3df97e"));
        assert_eq!(get(p, "flow").unwrap().as_str(), Some("xtls-rprx-vision"));
        assert_eq!(get(p, "servername").unwrap().as_str(), Some("www.cloudflare.com"));
        assert_eq!(get(p, "name").unwrap().as_str(), Some("香港-01"), "fragment 需百分号解码");
        let ro = get(p, "reality-opts").unwrap().as_mapping().unwrap();
        assert_eq!(get(ro, "public-key").unwrap().as_str(), Some("KEY"));
        assert_eq!(get(ro, "short-id").unwrap().as_str(), Some("0123456789abcdef"));
        // 托管字段照常注入
        assert_eq!(get(&m, "mixed-port").unwrap().as_i64(), Some(7890));
    }

    #[test]
    fn generate_empty_profile_errors() {
        assert_eq!(generate("", None, &opts()), Err(ConfigGenError::Empty));
    }

    #[test]
    fn generate_garbage_errors() {
        assert_eq!(
            generate("这不是配置也不是节点", None, &opts()),
            Err(ConfigGenError::NotAConfig)
        );
    }

    #[test]
    fn generate_unsupported_scheme_errors() {
        let out = generate("tuic://abc@1.2.3.4:443", None, &opts());
        assert!(matches!(out, Err(ConfigGenError::UnsupportedScheme(_))));
    }

    #[test]
    fn node_list_subscription_gets_default_routing() {
        let profile = "vless://uuid@1.2.3.4:443?encryption=none&type=tcp#香港-01\n\
                       vless://uuid@5.6.7.8:443?encryption=none&type=tcp#日本-02\n";
        let out = generate(profile, None, &opts()).unwrap();
        let m = as_mapping(&out);
        // 默认分组 GLOBAL：全部节点 + DIRECT（TUI 可切换）
        let groups = get(&m, "proxy-groups").unwrap().as_sequence().unwrap();
        let g0 = groups[0].as_mapping().unwrap();
        assert_eq!(get(g0, "name").unwrap().as_str(), Some("GLOBAL"));
        assert_eq!(get(g0, "type").unwrap().as_str(), Some("select"));
        let members: Vec<&str> = get(g0, "proxies")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(members.contains(&"香港-01"));
        assert!(members.contains(&"日本-02"));
        assert!(members.contains(&"DIRECT"));
        // 默认规则（clash-verge 经典默认）：私网/国内直连 + 兜底 GLOBAL
        let rs: Vec<&str> = get(&m, "rules")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            rs,
            vec!["GEOIP,private,DIRECT,no-resolve", "GEOIP,CN,DIRECT", "MATCH,GLOBAL"]
        );
        assert_eq!(get(&m, "mode").unwrap().as_str(), Some("rule"));
    }

    #[test]
    fn yaml_subscription_with_rules_is_not_touched() {
        let profile = "proxies:\n  - name: n1\n    type: vless\n    server: 1.2.3.4\n    port: 443\n\
                       proxy-groups:\n  - name: 我的组\n    type: select\n    proxies: [n1, DIRECT]\n\
                       rules:\n  - DOMAIN-SUFFIX,example.com,我的组\n  - MATCH,我的组\n";
        let out = generate(profile, None, &opts()).unwrap();
        let m = as_mapping(&out);
        let rs: Vec<&str> = get(&m, "rules")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            rs,
            vec!["DOMAIN-SUFFIX,example.com,我的组", "MATCH,我的组"],
            "订阅自带规则不干预"
        );
        let groups = get(&m, "proxy-groups").unwrap().as_sequence().unwrap();
        assert_eq!(
            get(groups[0].as_mapping().unwrap(), "name").unwrap().as_str(),
            Some("我的组"),
            "订阅自带分组不干预"
        );
        assert_eq!(get(&m, "mode").unwrap().as_str(), Some("rule"));
    }

    #[test]
    fn yaml_proxies_only_gets_default_routing() {
        let profile = "proxies:\n  - name: n1\n    type: vless\n    server: 1.2.3.4\n    port: 443\n";
        let out = generate(profile, None, &opts()).unwrap();
        let m = as_mapping(&out);
        assert!(get(&m, "rules").is_some(), "proxies-only YAML 也补默认规则");
        assert!(get(&m, "proxy-groups").is_some(), "proxies-only YAML 也补默认分组");
        let rs: Vec<&str> = get(&m, "rules")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(rs.last(), Some(&"MATCH,GLOBAL"));
    }

    fn get<'a>(m: &'a serde_yaml_ng::Mapping, key: &str) -> Option<&'a Value> {
        m.get(Value::String(key.into()))
    }
}

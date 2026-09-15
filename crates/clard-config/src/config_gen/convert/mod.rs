//! 节点列表 → mihomo `proxies:` yaml。
//!
//! 订阅商可能返回「纯节点列表」（一行一个 `vless://` 等 URI）。转换逻辑移植自
//! clash-verge-rev `src/utils/uri-parser/`（各协议字段映射与 base64/query 语义保持一致）。
//!
//! 已实现协议：vless / vmess（V2rayN JSON）/ ss / trojan / http(s) / socks5 / hysteria2。
//! 未实现（明确报错，可后续按同结构扩展）：ssr / anytls / hysteria / tuic / wireguard /
//! vmess 的 Shadowrocket 与 Quantumult 变体。

mod helpers;
mod http_socks;
mod hysteria2;
mod ss;
mod trojan;
mod vless;
mod vmess;

use serde_yaml_ng::{Mapping, Value};

use crate::config_gen::ConfigGenError;

pub(crate) use helpers::decode_base64_or_original;

/// 已识别的节点协议前缀（用于判断「这是节点列表」）
const KNOWN_SCHEMES: &[&str] = &[
    "vless://", "vmess://", "ss://", "trojan://", "hysteria2://", "hy2://", "tuic://",
    "wireguard://", "wg://", "ssr://", "anytls://", "hysteria://", "hy://", "socks5://",
    "socks://", "http://", "https://",
];

/// 内容是否像节点列表：首个非空行以已知协议前缀开头。
pub fn looks_like_node_list(text: &str) -> bool {
    text.lines()
        .find_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            Some(l)
        })
        .is_some_and(|first| KNOWN_SCHEMES.iter().any(|s| first.starts_with(s)))
}

/// 节点列表 → `{ proxies: [...] }`。
pub fn node_list_to_yaml(text: &str) -> Result<Value, ConfigGenError> {
    let mut proxies = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let proxy = convert_node(line).map_err(|e| match e {
            ConfigGenError::UnsupportedScheme(s) => {
                ConfigGenError::UnsupportedScheme(format!("第 {} 行: {s}", i + 1))
            }
            other => other,
        })?;
        proxies.push(proxy);
    }
    if proxies.is_empty() {
        return Err(ConfigGenError::NotAConfig);
    }
    let mut m = Mapping::new();
    m.insert(Value::String("proxies".into()), Value::Sequence(proxies));
    Ok(Value::Mapping(m))
}

fn convert_node(line: &str) -> Result<Value, ConfigGenError> {
    let scheme = line.split("://").next().unwrap_or("").to_lowercase();
    match scheme.as_str() {
        "vless" => vless::convert(line),
        "vmess" => vmess::convert(line),
        "ss" => ss::convert(line),
        "trojan" => trojan::convert(line),
        "http" | "https" => http_socks::convert_http(line),
        "socks5" | "socks" => http_socks::convert_socks(line),
        "hysteria2" | "hy2" => hysteria2::convert(line),
        other => Err(ConfigGenError::UnsupportedScheme(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(m: &'a Mapping, key: &str) -> Option<&'a Value> {
        m.get(Value::String(key.into()))
    }

    fn parse_one(line: &str) -> Mapping {
        convert_node(line).unwrap().as_mapping().unwrap().clone()
    }

    #[test]
    fn dispatch_unknown_scheme_errors_with_line_number() {
        let err = node_list_to_yaml("tuic://a@b:1#n\nvless://x@y:1?t#m").unwrap_err();
        assert!(matches!(
            err,
            ConfigGenError::UnsupportedScheme(s) if s.starts_with("第 1 行: tuic")
        ));
    }

    #[test]
    fn ss_uri_with_base64_userinfo() {
        use base64::Engine;
        let userinfo = base64::engine::general_purpose::STANDARD.encode("aes-128-gcm:pass123");
        let line = format!("ss://{userinfo}@1.2.3.4:8388#日本-01");
        let m = parse_one(&line);
        assert_eq!(get(&m, "type").unwrap().as_str(), Some("ss"));
        assert_eq!(get(&m, "server").unwrap().as_str(), Some("1.2.3.4"));
        assert_eq!(get(&m, "port").unwrap().as_i64(), Some(8388));
        assert_eq!(get(&m, "cipher").unwrap().as_str(), Some("aes-128-gcm"));
        assert_eq!(get(&m, "password").unwrap().as_str(), Some("pass123"));
        assert_eq!(get(&m, "name").unwrap().as_str(), Some("日本-01"));
    }

    #[test]
    fn ss_uri_plain_userinfo() {
        let line = "ss://chacha20-ietf-poly1305:pw@host:443#s";
        let m = parse_one(line);
        assert_eq!(get(&m, "cipher").unwrap().as_str(), Some("chacha20-ietf-poly1305"));
        assert_eq!(get(&m, "password").unwrap().as_str(), Some("pw"));
    }

    #[test]
    fn ss_uri_with_obfs_plugin() {
        let line = "ss://YWVzLTI1Ni1nY206cHc@host:8443?plugin=obfs-local%3Bobfs%3Dhttp%3Bobfs-host%3Dcdn.example#o";
        let m = parse_one(line);
        assert_eq!(get(&m, "plugin").unwrap().as_str(), Some("obfs"));
        let po = get(&m, "plugin-opts").unwrap().as_mapping().unwrap();
        assert_eq!(get(po, "mode").unwrap().as_str(), Some("http"));
        assert_eq!(get(po, "host").unwrap().as_str(), Some("cdn.example"));
    }

    #[test]
    fn trojan_uri_with_ws() {
        let line = "trojan://pw@t.example:443?type=ws&host=cdn.example&path=%2Fws&sni=cdn.example&alpn=h2,http/1.1#T-1";
        let m = parse_one(line);
        assert_eq!(get(&m, "type").unwrap().as_str(), Some("trojan"));
        assert_eq!(get(&m, "password").unwrap().as_str(), Some("pw"));
        assert_eq!(get(&m, "network").unwrap().as_str(), Some("ws"));
        assert_eq!(get(&m, "sni").unwrap().as_str(), Some("cdn.example"));
        assert_eq!(get(&m, "alpn").unwrap().as_sequence().unwrap().len(), 2);
        let wo = get(&m, "ws-opts").unwrap().as_mapping().unwrap();
        assert_eq!(get(wo, "path").unwrap().as_str(), Some("/ws"));
        let hd = get(wo, "headers").unwrap().as_mapping().unwrap();
        assert_eq!(get(hd, "Host").unwrap().as_str(), Some("cdn.example"));
    }

    #[test]
    fn vmess_json_uri() {
        use base64::Engine;
        let json = r#"{"v":"2","ps":"HK-01","add":"hk.example","port":"443","id":"uuid-1","aid":"0","scy":"auto","net":"ws","type":"none","host":"cdn.example","path":"/ws","tls":"tls","sni":"cdn.example"}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        let m = parse_one(&format!("vmess://{b64}"));
        assert_eq!(get(&m, "type").unwrap().as_str(), Some("vmess"));
        assert_eq!(get(&m, "name").unwrap().as_str(), Some("HK-01"));
        assert_eq!(get(&m, "server").unwrap().as_str(), Some("hk.example"));
        assert_eq!(get(&m, "port").unwrap().as_i64(), Some(443));
        assert_eq!(get(&m, "uuid").unwrap().as_str(), Some("uuid-1"));
        assert_eq!(get(&m, "tls").unwrap().as_bool(), Some(true));
        assert_eq!(get(&m, "servername").unwrap().as_str(), Some("cdn.example"));
        let wo = get(&m, "ws-opts").unwrap().as_mapping().unwrap();
        assert_eq!(get(wo, "path").unwrap().as_str(), Some("/ws"));
    }

    #[test]
    fn http_and_socks() {
        let m = parse_one("http://u:pw@h.example:8080#H");
        assert_eq!(get(&m, "type").unwrap().as_str(), Some("http"));
        assert_eq!(get(&m, "username").unwrap().as_str(), Some("u"));
        assert_eq!(get(&m, "password").unwrap().as_str(), Some("pw"));

        let m = parse_one("socks5://h2.example:1080?udp=1#S");
        assert_eq!(get(&m, "type").unwrap().as_str(), Some("socks5"));
        assert_eq!(get(&m, "udp").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn hysteria2_uri() {
        let line = "hysteria2://pass@hy.example:8443?sni=hy.example&obfs=salamander&obfs-password=secret&insecure=1#HY";
        let m = parse_one(line);
        assert_eq!(get(&m, "type").unwrap().as_str(), Some("hysteria2"));
        assert_eq!(get(&m, "password").unwrap().as_str(), Some("pass"));
        assert_eq!(get(&m, "sni").unwrap().as_str(), Some("hy.example"));
        assert_eq!(get(&m, "obfs").unwrap().as_str(), Some("salamander"));
        assert_eq!(get(&m, "obfs-password").unwrap().as_str(), Some("secret"));
        assert_eq!(get(&m, "skip-cert-verify").unwrap().as_bool(), Some(true));
    }
}

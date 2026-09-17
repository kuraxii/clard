//! 节点 URI 解析工具（移植自 clash-verge-rev `src/utils/uri-parser/helpers.ts`）。
//!
//! 关键语义：
//! - base64 解码：URL-safe（`-`→`+`、`_`→`/`）+ 去空白 + 补 padding + UTF-8 严格解码，
//!   解码结果含控制字符则**放弃解码、原样返回**（避免误伤恰好可解码的普通文本）；
//! - query 解析：**`+` 不视为空格**（只解码 `%XX`），键 `_` 归一化为 `-`；
//! - URL 结构用 `auth@host:port?query#fragment` 手工解析（对齐 `parseUrlLike`）。

use std::collections::HashMap;

use base64::Engine;
use serde_yaml_ng::{Mapping, Value};

/// 往 mapping 里插一个键值。
pub fn kv(m: &mut Mapping, k: &str, v: impl Into<Value>) {
    m.insert(Value::String(k.into()), v.into());
}

/// 往 mapping 里插一个可空键值。
pub fn kv_opt(m: &mut Mapping, k: &str, v: Option<Value>) {
    if let Some(v) = v {
        m.insert(Value::String(k.into()), v);
    }
}

/// 解码失败时原样返回（对齐 `decodeBase64OrOriginal`）。
pub fn decode_base64_or_original(s: &str) -> String {
    let normalized: String = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let padded = match normalized.len() % 4 {
        0 => normalized,
        n => format!("{normalized}{}", "=".repeat(4 - n)),
    };
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(padded.as_bytes()) {
        if let Ok(decoded) = String::from_utf8(bytes) {
            let printable = decoded.chars().all(|c| {
                let code = u32::from(c);
                code == 9 || code == 10 || code == 13 || (32..=126).contains(&code) || code >= 128
            });
            if printable {
                return decoded;
            }
        }
    }
    s.to_string()
}

/// 非空且去空白后非空（对齐 `getIfNotBlank`）。
pub fn get_if_not_blank(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.trim().is_empty())
}

/// 百分号解码（仅 `%XX`，`+` 保持字面）；失败原样返回。
pub fn safe_decode_uri_component(value: &str) -> String {
    urlencoding::decode(value)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| value.to_string())
}

/// 百分号解码 + 去空白；空结果返回 None（对齐 `decodeAndTrim`）。
pub fn decode_and_trim(value: Option<&str>) -> Option<String> {
    let decoded = safe_decode_uri_component(value?);
    let trimmed = decoded.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 解析 `?a=b&c&d=e`；键去空白、`_`→`-`；无 `=` 的项记为「存在但无值」。
#[derive(Debug, Default, Clone)]
pub struct Query(HashMap<String, Option<String>>);

impl Query {
    pub fn parse(raw: Option<&str>) -> Self {
        let mut map = HashMap::new();
        if let Some(raw) = raw {
            for part in raw.split('&') {
                if part.is_empty() {
                    continue;
                }
                let (key_raw, value_raw) = match part.split_once('=') {
                    Some((k, v)) => (k, Some(v)),
                    None => (part, None),
                };
                let key = key_raw.trim().replace('_', "-");
                if key.is_empty() {
                    continue;
                }
                let value = value_raw.map(safe_decode_uri_component);
                map.insert(key, value);
            }
        }
        Self(map)
    }

    /// 键是否存在（包括「存在但无值」）
    pub fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// 键的值（「存在但无值」按 None 处理）
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.as_deref())
    }
}

/// `?flag` / `?flag=` / `?flag=true|1` → true。
pub fn parse_bool_or_presence(v: Option<&str>) -> bool {
    match v {
        None | Some("") => true,
        Some(s) => s.eq_ignore_ascii_case("true") || s == "1",
    }
}

/// `true|1`（忽略大小写）→ true，其余 → false；无值 → None。
pub fn parse_bool(v: Option<&str>) -> Option<bool> {
    v.map(|s| s.eq_ignore_ascii_case("true") || s == "1")
}

/// vless flow：`none`/非法字符 → None。
pub fn parse_vless_flow(v: Option<&str>) -> Option<String> {
    let flow = v?;
    if flow.is_empty() || flow.eq_ignore_ascii_case("none") {
        return None;
    }
    let valid = flow.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && flow.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then(|| flow.to_string())
}

/// 严格端口：1..=65535 全数字。
pub fn parse_required_port(s: &str) -> Option<u16> {
    if !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse::<u16>().ok().filter(|p| (1..=65535).contains(p))
}

pub fn parse_port_or_default(port: Option<&str>, dft: u16) -> u16 {
    port.and_then(parse_required_port).unwrap_or(dft)
}

/// 密文别名/白名单；未知 → `auto`。
pub fn get_cipher(v: Option<&str>) -> String {
    const KNOWN: &[&str] = &[
        "none",
        "auto",
        "dummy",
        "aes-128-gcm",
        "aes-192-gcm",
        "aes-256-gcm",
        "lea-128-gcm",
        "lea-192-gcm",
        "lea-256-gcm",
        "aes-128-gcm-siv",
        "aes-256-gcm-siv",
        "2022-blake3-aes-128-gcm",
        "2022-blake3-aes-256-gcm",
        "aes-128-cfb",
        "aes-192-cfb",
        "aes-256-cfb",
        "aes-128-ctr",
        "aes-192-ctr",
        "aes-256-ctr",
        "chacha20",
        "chacha20-ietf",
        "chacha20-ietf-poly1305",
        "2022-blake3-chacha20-poly1305",
        "rabbit128-poly1305",
        "xchacha20-ietf-poly1305",
        "xchacha20",
        "aegis-128l",
        "aegis-256",
        "aez-384",
        "deoxys-ii-256-128",
        "rc4-md5",
    ];
    let Some(v) = v else { return "none".into() };
    if v == "chacha20-poly1305" {
        return "chacha20-ietf-poly1305".into();
    }
    if KNOWN.contains(&v) {
        v.to_string()
    } else {
        "auto".into()
    }
}

/// `auth@host:port?query#fragment` 手工解析（对齐 `parseUrlLike`；端口可缺省）。
pub struct UrlParts {
    pub auth: Option<String>,
    pub host: String,
    pub port: Option<String>,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

pub fn parse_url_like(input: &str) -> Option<UrlParts> {
    let (before_frag, fragment) = match input.split_once('#') {
        Some((a, b)) => (a, Some(b.to_string())),
        None => (input, None),
    };
    let (before_query, query) = match before_frag.split_once('?') {
        Some((a, b)) => (a, Some(b.to_string())),
        None => (before_frag, None),
    };
    // 端口后允许一个可选斜杠（`host:port/`）
    let before_query = before_query.strip_suffix('/').unwrap_or(before_query);
    let (auth, rest) = match before_query.rsplit_once('@') {
        Some((a, r)) => (Some(a.to_string()), r),
        None => (None, before_query),
    };
    let (host, port) = match rest.rfind(':') {
        Some(idx) => {
            let (h, p) = (&rest[..idx], &rest[idx + 1..]);
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                (h, Some(p.to_string()))
            } else {
                (rest, None)
            }
        }
        None => (rest, None),
    };
    if host.is_empty() {
        return None;
    }
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host)
        .to_string();
    Some(UrlParts {
        auth,
        host,
        port,
        query,
        fragment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_urlsafe_and_padding() {
        assert_eq!(decode_base64_or_original("aGVsbG8="), "hello");
        assert_eq!(decode_base64_or_original("aGVsbG8"), "hello", "缺 padding 自动补");
        // URL-safe 字符
        let payload = "vless://uuid@host:443?x=y#n";
        let b64 = base64::engine::general_purpose::URL_SAFE.encode(payload);
        assert_eq!(decode_base64_or_original(&b64), payload);
    }

    #[test]
    fn base64_non_text_falls_back_to_original() {
        // 解码结果是二进制（含控制字符）→ 原样返回
        let bin = base64::engine::general_purpose::STANDARD.encode([0u8, 1, 2, 3, 255]);
        assert_eq!(decode_base64_or_original(&bin), bin);
    }

    #[test]
    fn base64_plain_text_falls_back_when_not_decodable() {
        // 非 base64 字母表 → 原样
        let s = "proxies: []\n";
        assert_eq!(decode_base64_or_original(s), s);
    }

    #[test]
    fn query_plus_kept_literal_and_key_normalized() {
        let q = Query::parse(Some("a_b=c%2Bd&flag=&empty"));
        assert_eq!(q.get("a-b"), Some("c+d"), "`+` 不得变成空格");
        assert!(q.has("flag"));
        assert_eq!(q.get("flag"), Some(""));
        assert!(q.has("empty"));
        assert_eq!(q.get("empty"), None);
    }

    #[test]
    fn url_like_parses_auth_host_port() {
        let p = parse_url_like("user:pass@example.com:443?x=1#name").unwrap();
        assert_eq!(p.auth.as_deref(), Some("user:pass"));
        assert_eq!(p.host, "example.com");
        assert_eq!(p.port.as_deref(), Some("443"));
        assert_eq!(p.query.as_deref(), Some("x=1"));
        assert_eq!(p.fragment.as_deref(), Some("name"));
    }

    #[test]
    fn url_like_ipv6_brackets() {
        let p = parse_url_like("uuid@[::1]:443?x#n").unwrap();
        assert_eq!(p.host, "::1");
        assert_eq!(p.port.as_deref(), Some("443"));
    }

    #[test]
    fn url_like_optional_port() {
        let p = parse_url_like("user@example.com#n").unwrap();
        assert_eq!(p.port, None);
        assert_eq!(p.host, "example.com");
    }

    #[test]
    fn bool_and_flow_helpers() {
        assert!(parse_bool_or_presence(None));
        assert!(parse_bool_or_presence(Some("true")));
        assert!(!parse_bool_or_presence(Some("false")));
        assert_eq!(parse_vless_flow(Some("none")), None);
        assert_eq!(
            parse_vless_flow(Some("xtls-rprx-vision")).as_deref(),
            Some("xtls-rprx-vision")
        );
        assert_eq!(parse_vless_flow(Some("bad flow!")), None);
        assert_eq!(get_cipher(Some("chacha20-poly1305")), "chacha20-ietf-poly1305");
        assert_eq!(get_cipher(Some("aes-128-gcm")), "aes-128-gcm");
        assert_eq!(get_cipher(Some("not-a-cipher")), "auto");
        assert_eq!(get_cipher(None), "none");
    }
}

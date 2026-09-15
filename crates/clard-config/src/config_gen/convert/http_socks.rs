//! http / socks5 URI → mihomo proxy 条目（移植自 clash-verge `uri-parser/http.ts`、`socks.ts`）。

use serde_yaml_ng::{Mapping, Value};

use super::helpers::{
    Query, decode_and_trim, kv, parse_bool_or_presence, parse_port_or_default, parse_url_like,
    safe_decode_uri_component,
};
use crate::config_gen::ConfigGenError;

fn base(
    line: &str,
    scheme: &str,
    default_port: u16,
    type_name: &str,
    label: &str,
) -> Result<(Mapping, String), ConfigGenError> {
    let after_scheme = line
        .strip_prefix(&format!("{scheme}://"))
        .ok_or_else(|| ConfigGenError::InvalidNode(format!("不是 {scheme} URI")))?;
    let parsed = parse_url_like(after_scheme)
        .ok_or_else(|| ConfigGenError::InvalidNode(format!("{scheme} URI 解析失败")))?;
    let port = parse_port_or_default(parsed.port.as_deref(), default_port);
    let auth = parsed.auth.as_deref().map(safe_decode_uri_component);
    let name = decode_and_trim(parsed.fragment.as_deref())
        .unwrap_or_else(|| format!("{label} {}:{port}", parsed.host));

    let mut m = Mapping::new();
    kv(&mut m, "type", type_name);
    kv(&mut m, "name", name);
    kv(&mut m, "server", parsed.host.as_str());
    kv(&mut m, "port", i64::from(port));
    if let Some(auth) = auth {
        if let Some((user, pass)) = auth.split_once(':') {
            kv(&mut m, "username", user);
            kv(&mut m, "password", pass);
        }
    }
    Ok((m, parsed.query.unwrap_or_default()))
}

fn apply_common(m: &mut Mapping, q: &Query) {
    if q.has("tls") {
        kv(m, "tls", parse_bool_or_presence(q.get("tls")));
    }
    if let Some(v) = q.get("fingerprint") {
        kv(m, "fingerprint", v);
    }
    if q.has("skip-cert-verify") {
        kv(m, "skip-cert-verify", parse_bool_or_presence(q.get("skip-cert-verify")));
    }
    if let Some(v) = q.get("ip-version") {
        kv(m, "ip-version", v);
    }
}

/// `http://user:pass@server:port?tls=...#name`（https 同）
pub fn convert_http(line: &str) -> Result<Value, ConfigGenError> {
    let (mut m, query_raw) = base(line, "http", 443, "http", "HTTP")?;
    let q = Query::parse(Some(&query_raw));
    apply_common(&mut m, &q);
    Ok(Value::Mapping(m))
}

/// `socks5://user:pass@server:port?udp=1#name`（socks 同）
pub fn convert_socks(line: &str) -> Result<Value, ConfigGenError> {
    let (mut m, query_raw) = base(line, "socks5", 443, "socks5", "SOCKS5")?;
    let q = Query::parse(Some(&query_raw));
    apply_common(&mut m, &q);
    if q.has("udp") {
        kv(&mut m, "udp", parse_bool_or_presence(q.get("udp")));
    }
    Ok(Value::Mapping(m))
}

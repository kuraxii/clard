//! hysteria2 URI → mihomo proxy 条目（移植自 clash-verge `uri-parser/hysteria2.ts`）。

use serde_yaml_ng::{Mapping, Value};

use super::helpers::{
    Query, decode_and_trim, kv, kv_opt, parse_bool_or_presence, parse_port_or_default,
    parse_url_like, safe_decode_uri_component,
};
use crate::config_gen::ConfigGenError;

/// `hysteria2://password@server:port?sni=...#name`
pub fn convert(line: &str) -> Result<Value, ConfigGenError> {
    let after_scheme = line
        .strip_prefix("hysteria2://")
        .or_else(|| line.strip_prefix("hy2://"))
        .ok_or_else(|| ConfigGenError::InvalidNode("不是 hysteria2 URI".into()))?;
    let parsed = parse_url_like(after_scheme)
        .ok_or_else(|| ConfigGenError::InvalidNode("hysteria2 URI 解析失败".into()))?;
    let password_raw = parsed
        .auth
        .as_deref()
        .ok_or_else(|| ConfigGenError::InvalidNode("缺少密码".into()))?;
    let password = safe_decode_uri_component(password_raw);
    let port = parse_port_or_default(parsed.port.as_deref(), 443);
    let name = decode_and_trim(parsed.fragment.as_deref())
        .unwrap_or_else(|| format!("Hysteria2 {}:{port}", parsed.host));

    let mut m = Mapping::new();
    kv(&mut m, "type", "hysteria2");
    kv(&mut m, "name", name);
    kv(&mut m, "server", parsed.host.as_str());
    kv(&mut m, "port", i64::from(port));
    kv(&mut m, "password", password.as_str());

    let q = Query::parse(parsed.query.as_deref());
    kv_opt(
        &mut m,
        "sni",
        q.get("sni").or_else(|| q.get("peer")).map(Value::from),
    );
    if let Some(obfs) = q.get("obfs") {
        if obfs != "none" {
            kv(&mut m, "obfs", obfs);
        }
    }
    kv_opt(&mut m, "ports", q.get("mport").map(Value::from));
    kv_opt(&mut m, "obfs-password", q.get("obfs-password").map(Value::from));
    if q.has("insecure") {
        kv(&mut m, "skip-cert-verify", parse_bool_or_presence(q.get("insecure")));
    }
    if q.has("fastopen") {
        kv(&mut m, "tfo", parse_bool_or_presence(q.get("fastopen")));
    }
    kv_opt(&mut m, "fingerprint", q.get("pinSHA256").map(Value::from));

    Ok(Value::Mapping(m))
}

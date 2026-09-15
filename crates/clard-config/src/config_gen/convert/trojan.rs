//! trojan URI → mihomo proxy 条目（移植自 clash-verge `uri-parser/trojan.ts`）。

use serde_yaml_ng::{Mapping, Value};

use super::helpers::{
    Query, decode_and_trim, kv, kv_opt, parse_bool_or_presence, parse_port_or_default,
    parse_url_like, safe_decode_uri_component,
};
use crate::config_gen::ConfigGenError;

/// `trojan://password@server:port?query#name`
pub fn convert(line: &str) -> Result<Value, ConfigGenError> {
    let after_scheme = line
        .strip_prefix("trojan://")
        .ok_or_else(|| ConfigGenError::InvalidNode("不是 trojan URI".into()))?;
    let parsed = parse_url_like(after_scheme)
        .ok_or_else(|| ConfigGenError::InvalidNode("trojan URI 解析失败".into()))?;
    let password_raw = parsed
        .auth
        .as_deref()
        .ok_or_else(|| ConfigGenError::InvalidNode("缺少密码".into()))?;
    let password = safe_decode_uri_component(password_raw);
    let port = parse_port_or_default(parsed.port.as_deref(), 443);
    let name = decode_and_trim(parsed.fragment.as_deref())
        .unwrap_or_else(|| format!("Trojan {}:{port}", parsed.host));

    let mut m = Mapping::new();
    kv(&mut m, "type", "trojan");
    kv(&mut m, "name", name);
    kv(&mut m, "server", parsed.host.as_str());
    kv(&mut m, "port", i64::from(port));
    kv(&mut m, "password", password.as_str());

    let q = Query::parse(parsed.query.as_deref());
    let network = q.get("type").filter(|n| matches!(*n, "ws" | "grpc" | "h2" | "tcp"));
    if let Some(n) = network {
        kv(&mut m, "network", n);
    }
    let host = q.get("host");
    let path = q.get("path");
    kv_opt(&mut m, "alpn", q.get("alpn").map(|v| Value::Sequence(v.split(',').map(Value::from).collect())));
    kv_opt(&mut m, "sni", q.get("sni").map(Value::from));
    if q.has("skip-cert-verify") {
        kv(&mut m, "skip-cert-verify", parse_bool_or_presence(q.get("skip-cert-verify")));
    }
    kv_opt(&mut m, "fingerprint", q.get("fingerprint").or_else(|| q.get("fp")).map(Value::from));

    // encryption=method;password（trojan + ss 混淆）
    if let Some(enc) = q.get("encryption") {
        let parts: Vec<&str> = enc.split(';').collect();
        if let [_, method, password] = parts.as_slice() {
            let mut so = Mapping::new();
            kv(&mut so, "enabled", true);
            kv(&mut so, "method", *method);
            kv(&mut so, "password", *password);
            kv(&mut m, "ss-opts", Value::Mapping(so));
        }
    }
    kv_opt(&mut m, "client-fingerprint", q.get("client-fingerprint").map(Value::from));

    match network {
        Some("ws") => {
            let mut wo = Mapping::new();
            kv_opt(&mut wo, "headers", host.map(|h| Value::Mapping({
                let mut hd = Mapping::new();
                kv(&mut hd, "Host", h);
                hd
            })));
            kv_opt(&mut wo, "path", path.map(Value::from));
            if !wo.is_empty() {
                kv(&mut m, "ws-opts", Value::Mapping(wo));
            }
        }
        Some("grpc") => {
            if let Some(sn) = path {
                let mut go = Mapping::new();
                kv(&mut go, "grpc-service-name", sn);
                kv(&mut m, "grpc-opts", Value::Mapping(go));
            }
        }
        _ => {}
    }

    Ok(Value::Mapping(m))
}

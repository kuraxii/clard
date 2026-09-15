//! vless URI → mihomo proxy 条目（移植自 clash-verge `uri-parser/vless.ts`）。

use serde_yaml_ng::{Mapping, Value};

use super::helpers::{
    Query, decode_and_trim, decode_base64_or_original, kv, kv_opt, parse_bool_or_presence,
    parse_required_port, parse_url_like, parse_vless_flow, safe_decode_uri_component,
};
use crate::config_gen::ConfigGenError;

/// `vless://uuid@server:port?query#name`
pub fn convert(line: &str) -> Result<Value, ConfigGenError> {
    let after_scheme = line
        .strip_prefix("vless://")
        .ok_or_else(|| ConfigGenError::InvalidNode("不是 vless URI".into()))?;
    if after_scheme.is_empty() {
        return Err(ConfigGenError::InvalidNode("vless URI 为空".into()));
    }

    // 兼容 Shadowrocket：uuid 部分是 base64
    let mut rest = after_scheme.to_string();
    let mut is_shadowrocket = false;
    let parsed = match parse_url_like(&rest) {
        Some(p) if p.port.is_some() => p,
        _ => {
            let (base64_part, other) = match rest.split_once('?') {
                Some((a, b)) => (a.to_string(), b.to_string()),
                None => return Err(ConfigGenError::InvalidNode("缺少端口".into())),
            };
            rest = format!("{}{}?", decode_base64_or_original(&base64_part), other);
            is_shadowrocket = true;
            parse_url_like(&rest)
                .ok_or_else(|| ConfigGenError::InvalidNode("vless URI 解析失败".into()))?
        }
    };

    let port = parsed
        .port
        .as_deref()
        .and_then(parse_required_port)
        .ok_or_else(|| ConfigGenError::InvalidNode("缺少/非法端口".into()))?;
    if parsed.host.is_empty() {
        return Err(ConfigGenError::InvalidNode("缺少 server".into()));
    }

    let mut uuid = parsed.auth.clone().unwrap_or_default();
    if is_shadowrocket {
        uuid = uuid.split(':').next_back().unwrap_or("").to_string();
    }
    uuid = safe_decode_uri_component(&uuid);
    if uuid.is_empty() {
        return Err(ConfigGenError::InvalidNode("缺少 uuid".into()));
    }

    let q = Query::parse(parsed.query.as_deref());
    let name = decode_and_trim(parsed.fragment.as_deref())
        .or_else(|| q.get("remarks").map(str::to_string))
        .or_else(|| q.get("remark").map(str::to_string))
        .unwrap_or_else(|| format!("VLESS {}:{port}", parsed.host));

    let mut m = Mapping::new();
    kv(&mut m, "type", "vless");
    kv(&mut m, "name", name);
    kv(&mut m, "server", parsed.host.as_str());
    kv(&mut m, "port", i64::from(port));
    kv(&mut m, "uuid", uuid.as_str());
    kv(&mut m, "udp", true);

    // encryption=none 省略
    if let Some(enc) = q.get("encryption") {
        if enc != "none" {
            kv(&mut m, "encryption", enc);
        }
    }

    let security = q.get("security").unwrap_or("none");
    let tls = security != "none";
    kv(&mut m, "tls", tls);
    kv_opt(
        &mut m,
        "servername",
        q.get("sni").or_else(|| q.get("peer")).map(Value::from),
    );
    kv_opt(&mut m, "flow", parse_vless_flow(q.get("flow")).map(Value::from));
    kv_opt(&mut m, "client-fingerprint", q.get("fp").map(Value::from));
    kv_opt(
        &mut m,
        "alpn",
        q.get("alpn").map(|v| Value::Sequence(v.split(',').map(Value::from).collect())),
    );
    if q.has("allowInsecure") {
        kv(&mut m, "skip-cert-verify", parse_bool_or_presence(q.get("allowInsecure")));
    }

    if security == "reality" {
        let mut ro = Mapping::new();
        kv_opt(&mut ro, "public-key", q.get("pbk").map(Value::from));
        kv_opt(&mut ro, "short-id", q.get("sid").map(Value::from));
        if !ro.is_empty() {
            kv(&mut m, "reality-opts", Value::Mapping(ro));
        }
    }

    // 网络层
    let httpupgrade = q.get("type") == Some("ws") || q.get("type") == Some("httpupgrade");
    let network = if q.get("headerType") == Some("http") {
        "http"
    } else {
        match q.get("type") {
            Some("websocket") => "ws",
            Some("ws") | Some("httpupgrade") => "ws",
            Some(t @ ("tcp" | "http" | "grpc" | "h2" | "xhttp")) => t,
            _ => "tcp",
        }
    };
    kv(&mut m, "network", network);

    if network != "tcp" && network != "none" {
        let host = q.get("host").or_else(|| q.get("obfsParam"));
        let path = q.get("path");
        match network {
            "ws" => {
                let mut wo = Mapping::new();
                kv_opt(&mut wo, "path", path.map(Value::from));
                if let Some(host) = host {
                    let mut hd = Mapping::new();
                    kv(&mut hd, "Host", host);
                    kv(&mut wo, "headers", Value::Mapping(hd));
                }
                if httpupgrade {
                    kv(&mut wo, "v2ray-http-upgrade", true);
                    kv(&mut wo, "v2ray-http-upgrade-fast-open", true);
                }
                if !wo.is_empty() {
                    kv(&mut m, "ws-opts", Value::Mapping(wo));
                }
            }
            "grpc" => {
                let service_name = q.get("serviceName").or(path);
                if let Some(sn) = service_name {
                    let mut go = Mapping::new();
                    kv(&mut go, "grpc-service-name", sn);
                    kv(&mut m, "grpc-opts", Value::Mapping(go));
                }
            }
            "h2" => {
                let mut ho = Mapping::new();
                kv_opt(&mut ho, "host", host.map(Value::from));
                kv_opt(&mut ho, "path", path.map(Value::from));
                if !ho.is_empty() {
                    kv(&mut m, "h2-opts", Value::Mapping(ho));
                }
            }
            "http" => {
                let mut ho = Mapping::new();
                kv_opt(&mut ho, "path", path.map(|p| Value::Sequence(vec![Value::from(p)])));
                if let Some(host) = host {
                    let mut hd = Mapping::new();
                    kv(&mut hd, "Host", vec![Value::from(host)]);
                    kv(&mut ho, "headers", Value::Mapping(hd));
                }
                if !ho.is_empty() {
                    kv(&mut m, "http-opts", Value::Mapping(ho));
                }
            }
            "xhttp" => {
                let mut xo = Mapping::new();
                kv_opt(&mut xo, "host", host.map(Value::from));
                kv_opt(&mut xo, "path", path.map(Value::from));
                kv_opt(&mut xo, "mode", q.get("mode").map(Value::from));
                if !xo.is_empty() {
                    kv(&mut m, "xhttp-opts", Value::Mapping(xo));
                }
            }
            _ => {}
        }
    }

    // tls 但无 servername → 从传输层 host 推导
    if tls && !m.contains_key(Value::String("servername".into())) {
        if let Some(host) = q.get("host") {
            kv(&mut m, "servername", host);
        }
    }

    Ok(Value::Mapping(m))
}

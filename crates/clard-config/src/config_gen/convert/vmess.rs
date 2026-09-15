//! vmess URI → mihomo proxy 条目（移植自 clash-verge `uri-parser/vmess.ts`）。
//!
//! 仅支持主流 V2rayN JSON 格式；Shadowrocket / Quantumult 变体明确报错（后续可扩展）。

use serde_json::Value as Json;
use serde_yaml_ng::{Mapping, Value};

use super::helpers::{
    decode_base64_or_original, get_cipher, kv, kv_opt, parse_bool, parse_required_port,
};
use crate::config_gen::ConfigGenError;

fn parse_vmess_params(decoded: &str) -> Result<Json, ConfigGenError> {
    serde_json::from_str::<Json>(decoded)
        .map_err(|_| ConfigGenError::InvalidNode("vmess 内容不是 V2rayN JSON 格式".into()))
}

/// `vmess://<base64>`（V2rayN：JSON；其他变体报错）
pub fn convert(line: &str) -> Result<Value, ConfigGenError> {
    let after_scheme = line
        .strip_prefix("vmess://")
        .ok_or_else(|| ConfigGenError::InvalidNode("不是 vmess URI".into()))?;
    let content = decode_base64_or_original(after_scheme);
    let params = parse_vmess_params(&content)?;
    let obj = params
        .as_object()
        .ok_or_else(|| ConfigGenError::InvalidNode("vmess JSON 不是对象".into()))?;

    let get_str = |k: &str| obj.get(k).and_then(Json::as_str);
    let get_any_str = |k: &str| {
        obj.get(k).map(|v| match v {
            Json::String(s) => s.clone(),
            other => other.to_string(),
        })
    };

    let server = get_str("add").ok_or_else(|| ConfigGenError::InvalidNode("缺少 server(add)".into()))?;
    let port_raw = get_any_str("port").ok_or_else(|| ConfigGenError::InvalidNode("缺少 port".into()))?;
    let port = parse_required_port(&port_raw).ok_or_else(|| ConfigGenError::InvalidNode("非法端口".into()))?;
    let uuid = get_str("id").ok_or_else(|| ConfigGenError::InvalidNode("缺少 id".into()))?;

    let name = get_str("ps")
        .or_else(|| get_str("remarks"))
        .or_else(|| get_str("remark"))
        .map(str::to_string)
        .unwrap_or_else(|| format!("VMess {server}:{port}"));

    let mut m = Mapping::new();
    kv(&mut m, "type", "vmess");
    kv(&mut m, "name", name);
    kv(&mut m, "server", server);
    kv(&mut m, "port", i64::from(port));
    kv(&mut m, "uuid", uuid);
    kv(&mut m, "cipher", get_cipher(Some(get_str("scy").unwrap_or("auto"))).as_str());
    kv(&mut m, "alterId", obj.get("aid").and_then(Json::as_u64).unwrap_or(0));

    // tls：tls 字段 ∈ {tls, true, 1, "1", "true"}
    let tls = match obj.get("tls") {
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_i64() == Some(1),
        Some(Json::String(s)) => s == "tls" || s == "1" || s.eq_ignore_ascii_case("true"),
        _ => false,
    };
    kv(&mut m, "tls", tls);
    if tls {
        kv_opt(&mut m, "servername", get_str("sni").map(Value::from));
    }
    if obj.contains_key("verify_cert") {
        let verify = parse_bool(get_any_str("verify_cert").as_deref()).unwrap_or(false);
        kv(&mut m, "skip-cert-verify", !verify);
    }

    // 网络层
    let net = get_str("net").or_else(|| get_str("obfs")).or_else(|| get_str("type"));
    let httpupgrade = net == Some("httpupgrade");
    let network = match net {
        Some("ws") | Some("websocket") | Some("httpupgrade") => Some("ws"),
        Some("http") => Some("http"),
        Some("grpc") => Some("grpc"),
        Some("h2") => Some("h2"),
        _ => None,
    };

    let host = get_str("host").or_else(|| get_str("obfsParam"));
    let path = get_str("path");
    if let Some(network) = network {
        match network {
            "ws" => {
                let mut wo = Mapping::new();
                kv_opt(&mut wo, "path", path.map(Value::from));
                kv_opt(
                    &mut wo,
                    "headers",
                    host.map(|h| Value::Mapping({
                        let mut hd = Mapping::new();
                        kv(&mut hd, "Host", h);
                        hd
                    })),
                );
                if httpupgrade {
                    kv(&mut wo, "v2ray-http-upgrade", true);
                    kv(&mut wo, "v2ray-http-upgrade-fast-open", true);
                }
                if !wo.is_empty() {
                    kv(&mut m, "ws-opts", Value::Mapping(wo));
                }
            }
            "grpc" => {
                if let Some(sn) = path {
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
                let paths = path.map(|p| Value::Sequence(vec![Value::from(p)])).unwrap_or_else(|| Value::Sequence(vec![Value::from("/")]));
                kv(&mut ho, "path", paths);
                if let Some(host) = host {
                    let mut hd = Mapping::new();
                    kv(&mut hd, "Host", vec![Value::from(host)]);
                    kv(&mut ho, "headers", Value::Mapping(hd));
                }
                kv(&mut m, "http-opts", Value::Mapping(ho));
            }
            _ => {}
        }
        if tls && !m.contains_key(Value::String("servername".into())) {
            if let Some(host) = host {
                kv(&mut m, "servername", host);
            }
        }
    }

    Ok(Value::Mapping(m))
}

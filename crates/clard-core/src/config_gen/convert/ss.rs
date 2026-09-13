//! ss URI → mihomo proxy 条目（移植自 clash-verge `uri-parser/ss.ts`）。

use serde_json::Value as Json;
use serde_yaml_ng::{Mapping, Value};

use super::helpers::{
    Query, decode_and_trim, decode_base64_or_original, get_cipher, get_if_not_blank, kv, kv_opt,
    parse_bool_or_presence, parse_required_port,
};
use crate::config_gen::ConfigGenError;

/// `ss://<base64(method:password)>@server:port?plugin=...#name` 或
/// `ss://method:password@server:port#name`（明文 userinfo）
pub fn convert(line: &str) -> Result<Value, ConfigGenError> {
    let after_scheme = line
        .strip_prefix("ss://")
        .ok_or_else(|| ConfigGenError::InvalidNode("不是 ss URI".into()))?;

    let (without_hash, hash) = match after_scheme.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (after_scheme, None),
    };
    let name_from_hash = decode_and_trim(hash);

    let (main_raw, query_raw) = match without_hash.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (without_hash, None),
    };
    let query = Query::parse(query_raw);

    let main = if main_raw.contains('@') {
        main_raw.to_string()
    } else {
        decode_base64_or_original(main_raw)
    };
    let at_idx = main
        .rfind('@')
        .ok_or_else(|| ConfigGenError::InvalidNode("缺少 '@'".into()))?;
    let user_info_raw = &main[..at_idx];
    let server_and_port_with_path = &main[at_idx + 1..];
    let server_and_port = server_and_port_with_path.split('/').next().unwrap_or_default();

    let port_idx = server_and_port
        .rfind(':')
        .ok_or_else(|| ConfigGenError::InvalidNode("缺少端口".into()))?;
    let server = &server_and_port[..port_idx];
    let server = server
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(server);
    let port_raw = &server_and_port[port_idx + 1..];
    let port = parse_required_port(port_raw).ok_or_else(|| ConfigGenError::InvalidNode("非法端口".into()))?;
    if server.is_empty() {
        return Err(ConfigGenError::InvalidNode("缺少 server".into()));
    }

    let user_info = decode_base64_or_original(user_info_raw);
    let (cipher_raw, password) = match user_info.split_once(':') {
        Some((c, p)) => (Some(c), Some(p)),
        None => (None, None),
    };
    if password.is_none() || password == Some("") {
        return Err(ConfigGenError::InvalidNode("缺少密码".into()));
    }

    let mut m = Mapping::new();
    kv(
        &mut m,
        "name",
        name_from_hash.unwrap_or_else(|| format!("SS {server}:{port}")),
    );
    kv(&mut m, "type", "ss");
    kv(&mut m, "server", server);
    kv(&mut m, "port", i64::from(port));
    kv(&mut m, "cipher", get_cipher(cipher_raw).as_str());
    kv(&mut m, "password", password.unwrap());

    // plugin=...（obfs / v2ray-plugin）
    if let Some(plugin_param) = query.get("plugin") {
        let parts: Vec<&str> = plugin_param.split(';').collect();
        let plugin_name = parts[0];
        let mut opts = std::collections::HashMap::new();
        for raw in &parts[1..] {
            if raw.is_empty() {
                continue;
            }
            if let Some((k, v)) = raw.split_once('=') {
                if !k.is_empty() {
                    opts.insert(k.to_string(), if v.is_empty() { "true".to_string() } else { v.to_string() });
                }
            }
        }
        match plugin_name {
            "obfs-local" | "simple-obfs" => {
                kv(&mut m, "plugin", "obfs");
                let mut po = Mapping::new();
                kv_opt(&mut po, "mode", get_if_not_blank(opts.get("obfs").map(String::as_str)).map(Value::from));
                kv_opt(&mut po, "host", get_if_not_blank(opts.get("obfs-host").map(String::as_str)).map(Value::from));
                if !po.is_empty() {
                    kv(&mut m, "plugin-opts", Value::Mapping(po));
                }
            }
            "v2ray-plugin" => {
                kv(&mut m, "plugin", "v2ray-plugin");
                let mut po = Mapping::new();
                kv(&mut po, "mode", "websocket");
                kv_opt(
                    &mut po,
                    "host",
                    get_if_not_blank(opts.get("obfs-host").map(String::as_str))
                        .or_else(|| get_if_not_blank(opts.get("host").map(String::as_str)))
                        .map(Value::from),
                );
                kv_opt(&mut po, "path", get_if_not_blank(opts.get("path").map(String::as_str)).map(Value::from));
                if opts.contains_key("tls") {
                    kv(&mut po, "tls", true);
                }
                if !po.is_empty() {
                    kv(&mut m, "plugin-opts", Value::Mapping(po));
                }
            }
            other => {
                return Err(ConfigGenError::InvalidNode(format!("不支持的 ss 插件: {other}")));
            }
        }
    }

    // v2ray-plugin=<base64 JSON>
    if let Some(v2ray_param) = query.get("v2ray-plugin") {
        let decoded = decode_base64_or_original(v2ray_param);
        if let Ok(json) = serde_json::from_str::<Json>(&decoded) {
            kv(&mut m, "plugin", "v2ray-plugin");
            kv(&mut m, "plugin-opts", json_to_mapping(&json));
        }
    }

    if query.has("uot") && parse_bool_or_presence(query.get("uot")) {
        kv(&mut m, "udp-over-tcp", true);
    }
    if query.has("tfo") && parse_bool_or_presence(query.get("tfo")) {
        kv(&mut m, "tfo", true);
    }

    Ok(Value::Mapping(m))
}

fn json_to_mapping(v: &Json) -> Value {
    match v {
        Json::Object(o) => {
            let mut m = Mapping::new();
            for (k, val) in o {
                m.insert(Value::String(k.clone()), json_to_mapping(val));
            }
            Value::Mapping(m)
        }
        Json::Array(a) => Value::Sequence(a.iter().map(json_to_mapping).collect()),
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Number(serde_yaml_ng::Number::from(n.as_i64().unwrap_or(0))),
        Json::String(s) => Value::String(s.clone()),
        Json::Null => Value::Null,
    }
}

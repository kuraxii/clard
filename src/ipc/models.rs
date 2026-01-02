use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[allow(missing_docs)] 
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendVersion {
    pub meta: bool,
    pub version: String,
}

#[allow(missing_docs)] 
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError{
    pub message: String
}


/// 代理组
#[allow(missing_docs)] 
#[derive(Debug, Serialize, Deserialize)]
pub struct Groups {
    pub proxies: Vec<Proxy>,
}
#[allow(missing_docs)] 
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proxy {
    // group type need
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub all: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expected_status: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fixed: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hidden: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub icon: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub now: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub test_url: Option<String>,

    // single proxy type need
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,

    // basic fields
    pub alive: bool,
    pub history: Vec<DelayHistory>,
    pub extra: HashMap<String, Extra>,
    pub name: String,
    pub udp: bool,
    pub uot: bool,
    #[serde(rename = "type")]
    pub proxy_type: ProxyType,
    pub xudp: bool,
    pub tfo: bool,
    pub mptcp: bool,
    pub smux: bool,
    pub interface: String,

    #[serde(rename(serialize = "dialerProxy", deserialize = "dialer-proxy"))]
    pub dialer_proxy: String,

    #[serde(rename(serialize = "routingMark", deserialize = "routing-mark"))]
    pub routing_mark: i8,
}

#[allow(missing_docs)] 
#[derive(Debug, Serialize, Deserialize)]
pub struct Extra {
    pub alive: bool,
    pub history: Vec<DelayHistory>,
}

#[allow(missing_docs)] 
#[derive(Debug, Serialize, Deserialize)]
pub struct DelayHistory {
    pub time: String,
    pub delay: u16,
}

#[allow(missing_docs)] 
#[derive(Debug, Serialize, Deserialize)]
pub enum ProxyType {
    Direct,
    Reject,
    RejectDrop,
    Compatible,
    Pass,
    Dns,
    Shadowsocks,
    ShadowsocksR,
    Snell,
    Socks5,
    Http,
    Vmess,
    Vless,
    Trojan,
    Hysteria,
    Hysteria2,
    WireGuard,
    Tuic,
    Ssh,
    Mieru,
    AnyTLS,
    Relay,
    Selector,
    Fallback,
    URLTest,
    LoadBalance,
}

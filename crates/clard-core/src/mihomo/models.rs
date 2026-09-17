use std::{collections::HashMap, fmt::Display};

use serde::{Deserialize, Serialize};

#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    pub message: String,
}

/// 代理组
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Groups {
    pub proxies: Vec<Proxy>,
}
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(rename = "provider-name", skip_serializing_if = "Option::is_none", default)]
    pub provider_name: Option<String>,

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
    pub routing_mark: i32,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extra {
    pub alive: bool,
    pub history: Vec<DelayHistory>,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelayHistory {
    pub time: String,
    pub delay: u16,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// 以下为 mihomo 可能出现的类型，补齐避免 `GET /proxies` 解码失败
    PassRule,
    Rematch,
    Sudoku,
    Masque,
    TrustTunnel,
    ShadowQuic,
    OpenVPN,
    Tailscale,
    ZeroTier,
    GostRelay,
}
/// connections
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connections {
    /// 总下载量
    pub download_total: u64,
    /// 总上传量
    pub upload_total: u64,
    /// 连接
    pub connections: Option<Vec<Connection>>,
    /// 内存占用
    pub memory: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub metadata: ConnectionMetaData,
    pub upload: u64,
    pub download: u64,
    pub start: String,
    pub chains: Vec<String>,
    pub rule: String,
    pub rule_payload: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionMetaData {
    pub network: Network,

    #[serde(rename = "type")]
    pub connection_type: ConnectionType,

    #[serde(rename = "sourceIP")]
    pub source_ip: String,

    #[serde(rename = "destinationIP")]
    pub destination_ip: String,

    #[serde(rename = "sourceGeoIP")]
    pub source_geo_ip: Option<Vec<String>>,

    #[serde(rename = "destinationGeoIP")]
    pub destination_geo_ip: Option<Vec<String>>,

    #[serde(rename = "sourceIPASN")]
    pub source_ip_asn: String,

    #[serde(rename = "destinationIPASN")]
    pub destination_ip_asn: String,

    pub source_port: String,
    pub destination_port: String,

    #[serde(rename = "inboundIP")]
    pub inbound_ip: String,

    pub inbound_port: String,
    pub inbound_name: String,
    pub inbound_user: String,
    pub host: String,
    pub dns_mode: DNSMode,
    pub uid: u32,
    pub process: String,
    pub process_path: String,
    pub special_proxy: String,
    pub special_rules: String,
    pub remote_destination: String,
    pub dscp: u8,
    pub sniff_host: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DNSMode {
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "fake-ip")]
    FakeIP,
    #[serde(rename = "redir-host")]
    Mapping,
    #[serde(rename = "hosts")]
    Hosts,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Network {
    #[serde(rename = "tcp")]
    TCP,
    #[serde(rename = "udp")]
    UDP,
    #[serde(rename = "all")]
    ALLNet,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionType {
    HTTP,
    HTTPS,
    #[serde(rename = "Socks4")]
    SOCKS4,
    #[serde(rename = "Socks5")]
    SOCKS5,
    #[serde(rename = "ShadowSocks")]
    SHADOWSOCKS,
    #[serde(rename = "Vmess")]
    VMESS,
    #[serde(rename = "Vless")]
    VLESS,
    #[serde(rename = "Redir")]
    REDIR,
    #[serde(rename = "TProxy")]
    TPROXY,
    #[serde(rename = "Trojan")]
    TROJAN,
    #[serde(rename = "Tunnel")]
    TUNNEL,
    #[serde(rename = "Tun")]
    TUN,
    #[serde(rename = "Tuic")]
    TUIC,
    #[serde(rename = "Hysteria2")]
    HYSTERIA2,
    #[serde(rename = "AnyTLS")]
    ANYTLS,
    #[serde(rename = "Inner")]
    INNER,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleProviders {
    pub providers: HashMap<String, RuleProvider>,
}

/// `GET /rules` 的响应：生效规则列表（doc/04 §5）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    pub rules: Vec<Rule>,
}

/// 单条生效规则（doc/04 §5）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub index: usize,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub payload: String,
    pub proxy: String,
    pub size: i64,
    /// 命中统计/禁用状态；仅 RuleWrapper 包装的规则存在。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub extra: Option<RuleExtra>,
}

/// 规则命中统计与禁用状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleExtra {
    pub disabled: bool,
    pub hit_count: u64,
    #[serde(default)]
    pub hit_at: Option<String>,
    pub miss_count: u64,
    #[serde(default)]
    pub miss_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleBehavior {
    Domain,
    #[serde(rename = "IPCIDR")]
    IpCidr,
    Classical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleFormat {
    #[serde(rename = "YamlRule")]
    Yaml,
    #[serde(rename = "TextRule")]
    Text,
    #[serde(rename = "MrsRule")]
    Mrs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleProvider {
    pub behavior: RuleBehavior,
    pub format: RuleFormat,
    pub name: String,
    pub rule_count: u32,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub updated_at: String,
    pub vehicle_type: VehicleType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderType {
    Proxy,
    Rule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VehicleType {
    File,
    HTTP,
    Compatible,
    Inline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "kebab-case"))]
pub struct BaseConfig {
    pub port: u16,
    pub socks_port: u16,
    pub redir_port: u16,
    pub tproxy_port: u16,
    pub mixed_port: u16,
    pub tun: TunConfig,
    pub tuic_server: TuicServer,
    pub ss_config: String,
    pub vmess_config: String,
    pub authentication: Option<Vec<String>>,
    pub skip_auth_prefixes: Option<Vec<String>>,
    pub lan_allowed_ips: Option<Vec<String>>,
    pub lan_disallowed_ips: Option<Vec<String>>,
    pub allow_lan: bool,
    pub bind_address: String,
    pub inbound_tfo: bool,
    pub inbound_mptcp: bool,
    pub mode: ClashMode,
    pub unified_delay: bool,
    pub log_level: LogLevel,
    pub ipv6: bool,
    pub interface_name: String,
    pub routing_mark: isize,
    pub geox_url: GeoXUrl,
    pub geo_auto_update: bool,
    pub geo_update_interval: isize,
    pub geodata_mode: bool,
    pub geodata_loader: String,
    pub geosite_matcher: String,
    pub tcp_concurrent: bool,
    pub find_process_mode: FindProcessMode,
    pub sniffing: bool,
    pub global_client_fingerprint: String,
    pub global_ua: String,
    pub etag_support: bool,
    pub keep_alive_interval: isize,
    pub keep_alive_idle: isize,
    pub disable_keep_alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "kebab-case"))]
pub struct TunConfig {
    pub enable: bool,
    pub device: String,
    pub stack: TunStack,
    pub dns_hijack: Vec<String>,
    pub auto_route: bool,
    pub auto_detect_interface: bool,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mtu: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gso: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gso_max_size: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inet4_address: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inet6_address: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub iproute2_table_index: Option<isize>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub iproute2_rule_index: Option<isize>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub auto_redirect: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub auto_redirect_input_mark: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub auto_redirect_output_mark: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub loopback_address: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub strict_route: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub route_address: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub route_address_set: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub route_exclude_address: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub route_exclude_address_set: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub include_interface: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_interface: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub include_uid: Option<Vec<u32>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub include_uid_range: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_uid: Option<Vec<u32>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_uid_range: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_src_port: Option<Vec<u16>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_src_port_range: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_dst_port: Option<Vec<u16>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_dst_port_range: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub include_android_user: Option<Vec<isize>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub include_package: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exclude_package: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub endpoint_independent_nat: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub udp_timeout: Option<i64>,

    pub file_descriptor: u32,

    // The following `inet*` fields will be deprecated
    // refer: https://wiki.metacubex.one/config/inbound/tun/#_1
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inet4_route_address: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inet6_route_address: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inet4_route_exclude_address: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub inet6_route_exclude_address: Option<Vec<String>>,

    // darwin special config
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recvmsgx: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sendmsgx: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoreUpdaterChannel {
    #[serde(rename = "release")]
    ReleaseChannel,
    #[serde(rename = "alpha")]
    AlphaChannel,
    #[serde(rename = "auto")]
    Auto,
}

impl Display for CoreUpdaterChannel {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreUpdaterChannel::ReleaseChannel => write!(f, "release"),
            CoreUpdaterChannel::AlphaChannel => write!(f, "alpha"),
            CoreUpdaterChannel::Auto => write!(f, "auto"),
        }
    }
}

/// mihomo 模式枚举（rule/global/direct）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClashMode {
    Rule,
    Global,
    Direct,
}
impl Display for ClashMode {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClashMode::Rule => write!(f, "rule"),
            ClashMode::Global => write!(f, "global"),
            ClashMode::Direct => write!(f, "direct"),
        }
    }
}

/// tun stack enum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TunStack {
    Mixed,
    #[serde(rename = "gVisor")]
    Gvisor,
    System,
}

impl Display for TunStack {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TunStack::Mixed => write!(f, "Mixed"),
            TunStack::Gvisor => write!(f, "gVisor"),
            TunStack::System => write!(f, "System"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "kebab-case"))]
pub struct TuicServer {
    pub enable: bool,
    pub listen: String,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub token: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub users: Option<HashMap<String, String>>,

    pub certificate: String,
    pub private_key: String,
    pub ech_key: String,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub congestion_controller: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_idle_time: Option<isize>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub authentication_timeout: Option<isize>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub alpn: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_udp_relay_packet_size: Option<isize>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_datagram_frame_size: Option<isize>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cwnd: Option<isize>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mux_option: Option<MuxOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct MuxOption {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub padding: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub brutal: Option<BrutalOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrutalOption {
    pub enabled: bool,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub up: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub down: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindProcessMode {
    Strict,
    Always,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    DEBUG,
    INFO,
    WARNING,
    ERROR,
    SILENT,
}

impl Display for LogLevel {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::DEBUG => write!(f, "debug"),
            LogLevel::INFO => write!(f, "info"),
            LogLevel::WARNING => write!(f, "warning"),
            LogLevel::ERROR => write!(f, "error"),
            LogLevel::SILENT => write!(f, "silent"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "kebab-case"))]
pub struct GeoXUrl {
    pub geo_ip: String,
    pub mmdb: String,
    pub asn: String,
    pub geo_site: String,
}

/// commands 使用

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Traffic {
    pub up: u64,
    pub down: u64,
}

#[allow(missing_docs)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendVersion {
    pub meta: bool,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_groups() {
        let json = r#"{"proxies": []}"#;
        let groups: Result<Groups, _> = serde_json::from_str(json);
        match groups {
            Ok(_) => println!("Groups parsed successfully"),
            Err(e) => panic!("Failed to parse groups: {:?}", e),
        }
    }
}

//! Minimal IPC protocol types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub proxies: Vec<ProxyGroup>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProxyGroup {
    pub name: String,
    pub now: String,
    pub all: Vec<String>,
    pub r#type: String,
    pub alive: bool,
    pub udp: bool,
    pub tfo: bool,
    pub uot: bool,
    pub smux: bool,
    pub mptcp: bool,
    pub xudp: bool,

    pub history: Vec<serde_json::Value>,
    pub extra: serde_json::Value,
    pub hidden: bool,
    pub icon: String,
    pub interface: String,
    #[serde(rename = "dialer-proxy")]
    pub dialer_proxy: String,
    #[serde(rename = "routing-mark")]
    pub routing_mark: u32,
    #[serde(rename = "testUrl")]
    pub test_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_group() {
        let json: &str = r#"{"proxies":[{"alive":true,"all":["DIRECT","🔰 选择节点"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🇨🇳 国内网站","now":"DIRECT","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["🔰 选择节点","🇨🇳 台湾A01 | IEPL | x2","DIRECT"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"📺 动画疯","now":"🔰 选择节点","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["🇭🇰 香港A01","🇯🇵 日本A01","🇭🇰 香港A02 | IEPL","🇯🇵 日本A02 | IEPL","🇭🇰 香港A03 | IEPL","🇯🇵 日本A03 | IEPL","🇭🇰 香港A04 | IEPL","🇯🇵 日本A04 | IEPL","🇭🇰 香港A05 | IEPL","🇯🇵 日本A05 | 下载专用 | x0.01","🇭🇰 香港A06 | x0.8","🇯🇵 日本A06 | 下载专用 | x0.01","🇭🇰 香港A07 | x0.8","🇯🇵 日本A07 | x0.8","🇭🇰 香港A08 | x0.8","🇯🇵 日本A08 | x0.8","🇭🇰 香港A09 | IEPL","🇯🇵 日本A09 | IEPL","🇭🇰 香港A10 | IEPL","🇯🇵 日本A10 | IEPL","🇭🇰 香港A11 | IEPL","🇯🇵 日本A11 | IEPL","🇸🇬 新加坡A01","🇸🇬 新加坡A02","🇸🇬 新加坡A03 | IEPL | x2","🇨🇳 台湾A01 | IEPL | x2","🇺🇲 美国A01","🇺🇲 美国A02","🇬🇧 英国A01","🇦🇷 阿根廷A01","🇷🇺 俄罗斯A01","🇹🇷 土耳其A01","🇰🇷 韩国A01","🇮🇳 印度A01","🇩🇪 德国A01","🇨🇦 加拿大A01","🇦🇺 澳大利亚A01","🇫🇷 法国A01","🇺🇦 乌克兰A01","🇯🇵 免费-日本1-Ver.7","🇯🇵 免费-日本2-Ver.8","🇯🇵 免费-日本3-Ver.7","🇯🇵 免费-日本4-Ver.8","🇯🇵 免费-日本5-Ver.9","🇯🇵 免费-日本6-Ver.8","🇯🇵 免费-日本7-Ver.2","DIRECT"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🔰 选择节点","now":"🇭🇰 香港A03 | IEPL","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["🔰 选择节点","DIRECT","🇯🇵 日本A01","🇯🇵 日本A02 | IEPL","🇯🇵 日本A03 | IEPL","🇯🇵 日本A04 | IEPL","🇯🇵 日本A05 | 下载专用 | x0.01","🇯🇵 日本A06 | 下载专用 | x0.01","🇯🇵 日本A07 | x0.8","🇯🇵 日本A08 | x0.8","🇯🇵 日本A09 | IEPL","🇯🇵 日本A10 | IEPL","🇯🇵 日本A11 | IEPL","🇯🇵 免费-日本1-Ver.7","🇯🇵 免费-日本2-Ver.8","🇯🇵 免费-日本3-Ver.7","🇯🇵 免费-日本4-Ver.8","🇯🇵 免费-日本5-Ver.9","🇯🇵 免费-日本6-Ver.8","🇯🇵 免费-日本7-Ver.2","🇭🇰 香港A01","🇭🇰 香港A02 | IEPL","🇭🇰 香港A03 | IEPL","🇭🇰 香港A04 | IEPL","🇭🇰 香港A05 | IEPL","🇭🇰 香港A06 | x0.8","🇭🇰 香港A07 | x0.8","🇭🇰 香港A08 | x0.8","🇭🇰 香港A09 | IEPL","🇭🇰 香港A10 | IEPL","🇭🇰 香港A11 | IEPL"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"☁️ OneDrive","now":"🔰 选择节点","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["DIRECT","REJECT","🇭🇰 香港A01","🇯🇵 日本A01","🇭🇰 香港A02 | IEPL","🇯🇵 日本A02 | IEPL","🇭🇰 香港A03 | IEPL","🇯🇵 日本A03 | IEPL","🇭🇰 香港A04 | IEPL","🇯🇵 日本A04 | IEPL","🇭🇰 香港A05 | IEPL","🇯🇵 日本A05 | 下载专用 | x0.01","🇭🇰 香港A06 | x0.8","🇯🇵 日本A06 | 下载专用 | x0.01","🇭🇰 香港A07 | x0.8","🇯🇵 日本A07 | x0.8","🇭🇰 香港A08 | x0.8","🇯🇵 日本A08 | x0.8","🇭🇰 香港A09 | IEPL","🇯🇵 日本A09 | IEPL","🇭🇰 香港A10 | IEPL","🇯🇵 日本A10 | IEPL","🇭🇰 香港A11 | IEPL","🇯🇵 日本A11 | IEPL","🇸🇬 新加坡A01","🇸🇬 新加坡A02","🇸🇬 新加坡A03 | IEPL | x2","🇨🇳 台湾A01 | IEPL | x2","🇺🇲 美国A01","🇺🇲 美国A02","🇬🇧 英国A01","🇦🇷 阿根廷A01","🇷🇺 俄罗斯A01","🇹🇷 土耳其A01","🇰🇷 韩国A01","🇮🇳 印度A01","🇩🇪 德国A01","🇨🇦 加拿大A01","🇦🇺 澳大利亚A01","🇫🇷 法国A01","🇺🇦 乌克兰A01","🇯🇵 免费-日本1-Ver.7","🇯🇵 免费-日本2-Ver.8","🇯🇵 免费-日本3-Ver.7","🇯🇵 免费-日本4-Ver.8","🇯🇵 免费-日本5-Ver.9","🇯🇵 免费-日本6-Ver.8","🇯🇵 免费-日本7-Ver.2","🔰 选择节点","🌏 爱奇艺\u0026哔哩哔哩","📺 动画疯","🎮 Steam 登录/下载","🎮 Steam 商店/社区","🌩️ Cloudflare","☁️ OneDrive","🎓学术网站","🇨🇳 国内网站","🛑 拦截广告","🐟 漏网之鱼"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"GLOBAL","now":"🇯🇵 日本A03 | IEPL","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["🔰 选择节点","DIRECT"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🌩️ Cloudflare","now":"🔰 选择节点","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["DIRECT","🔰 选择节点"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🎓学术网站","now":"DIRECT","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["DIRECT","🇭🇰 香港A01","🇭🇰 香港A02 | IEPL","🇭🇰 香港A03 | IEPL","🇭🇰 香港A04 | IEPL","🇭🇰 香港A05 | IEPL","🇭🇰 香港A06 | x0.8","🇭🇰 香港A07 | x0.8","🇭🇰 香港A08 | x0.8","🇭🇰 香港A09 | IEPL","🇭🇰 香港A10 | IEPL","🇭🇰 香港A11 | IEPL","🇨🇳 台湾A01 | IEPL | x2"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🌏 爱奇艺\u0026哔哩哔哩","now":"DIRECT","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["DIRECT","🔰 选择节点","🇦🇷 阿根廷A01","🇷🇺 俄罗斯A01","🇹🇷 土耳其A01","🇮🇳 印度A01"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🎮 Steam 登录/下载","now":"DIRECT","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["🔰 选择节点","🇦🇷 阿根廷A01","🇷🇺 俄罗斯A01","🇹🇷 土耳其A01","🇮🇳 印度A01","DIRECT"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🎮 Steam 商店/社区","now":"🔰 选择节点","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["🔰 选择节点","DIRECT"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🐟 漏网之鱼","now":"🔰 选择节点","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false},{"alive":true,"all":["REJECT","DIRECT","🔰 选择节点"],"dialer-proxy":"","extra":{},"hidden":false,"history":[],"icon":"","interface":"","mptcp":false,"name":"🛑 拦截广告","now":"REJECT","routing-mark":0,"smux":false,"testUrl":"","tfo":false,"type":"Selector","udp":true,"uot":false,"xudp":false}]}"#;
        let _: Config = serde_json::from_str(json).unwrap();
    }
}

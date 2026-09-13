//! 订阅内容归一化：检测 base64 并解码为文本（对齐 clash-verge `decodeBase64OrOriginal`）。
//!
//! 很多订阅商返回 base64 编码内容（可能是完整 yaml，也可能是节点列表）。
//! 解码采用「URL-safe + 去空白 + 补 padding + UTF-8 严格解码 + 控制字符检查」，
//! 解码结果含控制字符则视为非 base64、原样返回。

use super::ConfigGenError;

/// 归一化：base64 → 文本；非 base64 原样返回。
pub fn normalize(raw: &str) -> Result<String, ConfigGenError> {
    if raw.trim().is_empty() {
        return Err(ConfigGenError::Empty);
    }
    Ok(super::convert::decode_base64_or_original(raw))
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;

    #[test]
    fn plain_yaml_passthrough() {
        let yaml = "proxies: []\nmixed-port: 7890\n";
        assert_eq!(normalize(yaml).unwrap(), yaml);
    }

    #[test]
    fn base64_of_yaml_decodes() {
        let yaml = "proxies:\n  - name: a\n    type: socks5\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(yaml);
        assert_eq!(normalize(&b64).unwrap(), yaml);
    }

    #[test]
    fn base64_of_node_list_decodes() {
        let nodes = "vless://a@b:1?x#n\nvless://c@d:2?x#m\n";
        let b64 = base64::engine::general_purpose::STANDARD.encode(nodes);
        assert_eq!(normalize(&b64).unwrap(), nodes);
    }

    #[test]
    fn random_text_passthrough() {
        let text = "hello world, 这不是 base64";
        assert_eq!(normalize(text).unwrap(), text);
    }

    #[test]
    fn empty_errors() {
        assert_eq!(normalize("  \n "), Err(ConfigGenError::Empty));
    }
}

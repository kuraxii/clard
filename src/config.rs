use serde::{Deserialize, Serialize};

/// ClardRs Configuration
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClardRsConfig {
    /// An example configuration section
    pub proxy: Proxy,
}

/// Default configuration settings.
///
/// Note: if your needs are as simple as below, you can
/// use `#[derive(Default)]` on ClardRsConfig instead.
impl Default for ClardRsConfig {
    fn default() -> Self {
        Self {
            proxy: Proxy::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Port(u16);

/// 代理相关配置： 代理端口...
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proxy {
    mixed: Port,
}

impl Default for Proxy {
    fn default() -> Self {
        Self { mixed: Port(7891) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml() {
        let config = ClardRsConfig::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        println!("{:?}", toml);
    }
}

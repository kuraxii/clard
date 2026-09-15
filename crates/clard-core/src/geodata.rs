//! geo 数据下载（geoip.metadb / geosite.dat，doc/01 §6.3，TUI 侧）：MetaCubeX
//! meta-rules-dat。jsdelivr CDN 优先（国内可直连），GitHub release 兜底；下载后
//! TUI 自算 sha256 供 helper 复核（与 InstallCore 同构的 inbox 流，helper 原子替换
//! `/var/clard/geodata/<file>` 并重启核心）。

use std::time::Duration;

use thiserror::Error;

/// meta-rules-dat release 分支（jsdelivr CDN，国内可直连）
pub const GEO_CDN_BASE: &str = "https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release";
/// GitHub release 兜底
pub const GEO_GITHUB_BASE: &str = "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download";

/// geo 数据下载上限（geoip.metadb ~8MB / geosite.dat ~4MB）。
pub const GEO_DOWNLOAD_LIMIT: u64 = 64 * 1024 * 1024;

const TIMEOUT: Duration = Duration::from_secs(120);

/// geo 数据下载失败（对用户可读）。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GeoError {
    #[error("请求失败: {0}")]
    Request(String),
    #[error("HTTP 状态异常: {0}")]
    HttpStatus(u16),
    #[error("响应体为空")]
    Empty,
    #[error("下载源均不可用（jsdelivr/GitHub）")]
    AllSourcesFailed,
}

/// 依次尝试各源下载 geo 数据文件（`geoip.metadb` / `geosite.dat`）。
pub async fn download_geo(client: &reqwest::Client, name: &str) -> Result<Vec<u8>, GeoError> {
    let mut last = GeoError::AllSourcesFailed;
    for base in [GEO_CDN_BASE, GEO_GITHUB_BASE] {
        match fetch_bytes(client, &format!("{base}/{name}")).await {
            Ok(data) if !data.is_empty() => return Ok(data),
            Ok(_) => return Err(GeoError::Empty),
            Err(e) => last = e,
        }
    }
    Err(last)
}

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, GeoError> {
    let resp = client
        .get(url)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| GeoError::Request(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(GeoError::HttpStatus(resp.status().as_u16()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| GeoError::Request(e.to_string()))?
        .to_vec();
    if bytes.len() as u64 > GEO_DOWNLOAD_LIMIT {
        return Err(GeoError::Request(format!("响应体超过上限 {GEO_DOWNLOAD_LIMIT} 字节")));
    }
    Ok(bytes)
}

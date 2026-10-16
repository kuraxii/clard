//! geo 数据下载（geoip.metadb / geosite.dat，doc/01 §6.3，TUI 侧）：MetaCubeX
//! meta-rules-dat 官方 GitHub release（`releases/download/latest`，格式对齐仓库
//! README 下载表）。下载后 TUI 自算 sha256 供 helper 复核（与 InstallCore 同构的
//! inbox 流，helper 原子替换 `/var/clard/lib/runtime/<file>` 并重启核心）。

use std::time::Duration;

use thiserror::Error;

/// GitHub 官方最新 release（README 下载表格式）
pub const GEO_GITHUB_BASE: &str = "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest";

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
    #[error("GitHub 下载失败")]
    AllSourcesFailed,
}

/// 从 GitHub 官方 release 下载 geo 数据文件（`geoip.metadb` / `geosite.dat`）。
pub async fn download_geo(client: &reqwest::Client, name: &str) -> Result<Vec<u8>, GeoError> {
    fetch_bytes(client, &format!("{GEO_GITHUB_BASE}/{name}")).await
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

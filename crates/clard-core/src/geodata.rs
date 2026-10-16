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
    /// 传输层/网络错误（连接失败、超时、TLS 中断等）——经代理下载时最常出现
    #[error("请求失败: {0}")]
    Request(String),
    #[error("HTTP 状态异常: {0}")]
    HttpStatus(u16),
    #[error("响应体为空")]
    Empty,
}

/// 从 GitHub 官方 release 下载 geo 数据文件（`geoip.metadb` / `geosite.dat`）。
pub async fn download_geo(client: &reqwest::Client, name: &str) -> Result<Vec<u8>, GeoError> {
    download_geo_from(client, GEO_GITHUB_BASE, name).await
}

/// 从指定 base 下载（测试可注入本地 mock；生产固定用 `GEO_GITHUB_BASE`）。
async fn download_geo_from(client: &reqwest::Client, base: &str, name: &str) -> Result<Vec<u8>, GeoError> {
    fetch_bytes(client, &format!("{base}/{name}"), GEO_DOWNLOAD_LIMIT).await
}

/// 单次 GET：非 2xx → `HttpStatus`；空响应 → `Empty`；超过 `limit` → `Request`。
/// `limit` 由调用方传入，测试用极小值验证超限分支（无需分配 64MiB）。
async fn fetch_bytes(client: &reqwest::Client, url: &str, limit: u64) -> Result<Vec<u8>, GeoError> {
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
    if bytes.is_empty() {
        return Err(GeoError::Empty);
    }
    if bytes.len() as u64 > limit {
        return Err(GeoError::Request(format!("响应体超过上限 {limit} 字节")));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// 起一个本地 HTTP mock：响应指定 status + body，接受一次连接即关闭。
    /// 返回 (base_url, 服务句柄)。
    async fn spawn_http_mock(status: u16, body: Vec<u8>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&body).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://127.0.0.1:{}", addr.port()), handle)
    }

    #[tokio::test]
    async fn download_success_returns_body_bytes() {
        let (base, srv) = spawn_http_mock(200, b"mmdb-bytes".to_vec()).await;
        let client = reqwest::Client::new();
        let data = download_geo_from(&client, &base, "geoip.metadb").await.unwrap();
        assert_eq!(data, b"mmdb-bytes");
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn http_error_status_maps_to_http_status() {
        for (status, expect) in [(403u16, 403u16), (429, 429), (500, 500), (503, 503)] {
            let (base, srv) = spawn_http_mock(status, Vec::new()).await;
            let client = reqwest::Client::new();
            let err = download_geo_from(&client, &base, "geosite.dat").await.unwrap_err();
            assert_eq!(err, GeoError::HttpStatus(expect), "状态码 {status}");
            srv.await.unwrap();
        }
    }

    #[tokio::test]
    async fn empty_body_is_empty_error() {
        // 200 + Content-Length 0：不得返回空字节让上层装空文件
        let (base, srv) = spawn_http_mock(200, Vec::new()).await;
        let client = reqwest::Client::new();
        let err = download_geo_from(&client, &base, "geoip.metadb").await.unwrap_err();
        assert_eq!(err, GeoError::Empty);
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn oversize_body_rejected() {
        let (base, srv) = spawn_http_mock(200, b"toolarge".to_vec()).await;
        let client = reqwest::Client::new();
        // 极小 limit 触发超限分支（不分配 64MiB）
        let err = fetch_bytes(&client, &format!("{base}/geoip.metadb"), 4)
            .await
            .unwrap_err();
        assert!(matches!(err, GeoError::Request(_)));
        assert!(err.to_string().contains("响应体超过上限"));
        srv.await.unwrap();
    }

    #[tokio::test]
    async fn transport_failure_is_request_error() {
        // 未监听端口 → 连接失败（经代理下载时的典型抖动）→ Request
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // 释放端口，模拟不可达
        let client = reqwest::Client::new();
        let err = download_geo_from(&client, &format!("http://127.0.0.1:{}", addr.port()), "geoip.metadb")
            .await
            .unwrap_err();
        assert!(matches!(err, GeoError::Request(_)), "传输错误应为 Request: {err:?}");
    }
}

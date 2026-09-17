//! 订阅内容下载：HTTP(S) GET，带超时、大小上限、文本校验。
//!
//! 边界：`fetch` 只负责「拿到合法文本」；「是否是合法 mihomo 配置」由
//! `clard-config`（后续里程碑）校验。`HttpFetcher` 之外的实现（测试 mock）
//! 通过 [`SubscriptionFetcher`] trait 注入，store 逻辑不感知传输层。

use std::time::Duration;

use thiserror::Error;

/// 下载失败的边界错误（对用户可读，可直接提示）
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DownloadError {
    #[error("请求失败: {0}")]
    Request(String),
    #[error("HTTP 状态异常: {0}")]
    HttpStatus(u16),
    #[error("响应体为空")]
    Empty,
    #[error("响应体超过上限 {0} 字节")]
    TooLarge(u64),
    #[error("响应体不是合法 UTF-8 文本")]
    InvalidUtf8,
}

/// 订阅获取抽象：让存储/导入逻辑与 HTTP 传输解耦（便于单元测试注入 mock）。
pub trait SubscriptionFetcher: Send + Sync {
    /// 获取订阅内容；`url` 非法或传输失败时返回 [`DownloadError`]。
    fn fetch(&self, url: &str) -> impl Future<Output = Result<String, DownloadError>> + Send;
}

/// 订阅流量/到期信息（响应头 `subscription-userinfo`，doc/05 §2 R2.7）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionInfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: Option<i64>,
}

/// 默认 HTTP(S) 抓取器。
pub struct HttpFetcher {
    client: reqwest::Client,
    timeout: Duration,
    /// 单次订阅响应体上限（防拖垮内存/磁盘）。
    max_bytes: u64,
}

impl HttpFetcher {
    /// 默认：30s 超时、8 MiB 上限。
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            timeout: Duration::from_secs(30),
            max_bytes: 8 * 1024 * 1024,
        }
    }
}

impl SubscriptionFetcher for HttpFetcher {
    async fn fetch(&self, url: &str) -> Result<String, DownloadError> {
        self.fetch_with_info(url).await.map(|(body, _, _)| body)
    }
}

impl HttpFetcher {
    /// 下载并同时解析 `subscription-userinfo`（R2.7）与 `Content-Disposition` 文件名（R2.1
    /// 缺省名，对齐 clash-verge-rev：`filename*` → `filename`）。
    pub async fn fetch_with_info(
        &self,
        url: &str,
    ) -> Result<(String, SubscriptionInfo, Option<String>), DownloadError> {
        let resp = self
            .client
            .get(url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| DownloadError::Request(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DownloadError::HttpStatus(resp.status().as_u16()));
        }
        let info = resp
            .headers()
            .get("subscription-userinfo")
            .and_then(|v| v.to_str().ok())
            .map(parse_subscription_userinfo)
            .unwrap_or_default();
        let filename = resp
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_disposition);
        let bytes = resp.bytes().await.map_err(|e| DownloadError::Request(e.to_string()))?;
        if bytes.is_empty() {
            return Err(DownloadError::Empty);
        }
        if bytes.len() as u64 > self.max_bytes {
            return Err(DownloadError::TooLarge(self.max_bytes));
        }
        let body = String::from_utf8(bytes.to_vec()).map_err(|_| DownloadError::InvalidUtf8)?;
        Ok((body, info, filename))
    }
}

/// 解析 `Content-Disposition` 文件名（对齐 clash-verge-rev `PrfItem::from_url`）：
/// 优先 `filename*`（RFC 5987：`UTF-8''<percent-encoded>`，取 `''` 后并 percent-decode），
/// 其次 `filename`（字面量，去包裹引号）。
fn parse_content_disposition(header: &str) -> Option<String> {
    if let Some(v) = header.split(';').map(str::trim).find_map(|s| {
        let (k, v) = s.split_once('=')?;
        (k.trim() == "filename*").then(|| v.trim().to_string())
    }) {
        let encoded = v.rsplit("''").next().unwrap_or(&v);
        if let Ok(decoded) = urlencoding::decode(encoded) {
            return Some(decoded.into_owned());
        }
    }
    header.split(';').map(str::trim).find_map(|s| {
        let (k, v) = s.split_once('=')?;
        if k.trim() != "filename" {
            return None;
        }
        Some(v.trim().trim_matches('"').to_string())
    })
}

/// 缺省配置名（对齐 clash-verge-rev）：`Content-Disposition` 文件名 →
/// URL 最后一段路径（percent-decode）→ "订阅"。
pub fn default_name(filename: Option<&str>, url: &str) -> String {
    filename
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| url_last_segment(url).unwrap_or_else(|| "订阅".to_string()))
}

/// URL 最后一段路径（去 query、按 `/` 切取最后一段、percent-decode），无则 `None`。
/// 仅接受带 scheme 的 URL（对齐 clash-verge-rev `get_last_part_and_decode`，额外要求 `://`
/// 以防非 URL 乱串被当成名字）。
fn url_last_segment(url: &str) -> Option<String> {
    if !url.contains("://") {
        return None;
    }
    let path = url.split('?').next()?;
    let last = path.rsplit('/').next()?;
    if last.is_empty() {
        return None;
    }
    Some(
        urlencoding::decode(last)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| last.to_string()),
    )
}

/// 解析 `subscription-userinfo`：`upload=…; download=…; total=…; expire=…`。
fn parse_subscription_userinfo(header: &str) -> SubscriptionInfo {
    let mut info = SubscriptionInfo::default();
    for pair in header.split(';') {
        let Some((k, v)) = pair.trim().split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "upload" => info.upload = v.parse().unwrap_or(0),
            "download" => info.download = v.parse().unwrap_or(0),
            "total" => info.total = v.parse().unwrap_or(0),
            "expire" => info.expire = v.parse().ok().filter(|n| *n > 0),
            _ => {}
        }
    }
    info
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// 起一个本地 HTTP 服务，单次响应后关闭；返回地址。
    async fn serve_once(status_line: &str, body: &[u8]) -> SocketAddr {
        serve_once_headers(status_line, "", body).await
    }

    /// 带额外响应头（每个以 `\r\n` 结尾）的单次 HTTP 服务。
    async fn serve_once_headers(status_line: &str, extra_headers: &str, body: &[u8]) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status_line.to_string();
        let extra = extra_headers.to_string();
        let body = body.to_vec();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let mut read = 0;
            loop {
                let n = sock.read(&mut buf[read..]).await.unwrap();
                if n == 0 {
                    break;
                }
                read += n;
                if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).await.unwrap();
            sock.write_all(&body).await.unwrap();
            let _ = sock.shutdown().await;
        });
        addr
    }

    fn fetcher() -> HttpFetcher {
        HttpFetcher::new(reqwest::Client::new())
    }

    #[tokio::test]
    async fn fetch_ok_returns_text() {
        let addr = serve_once("200 OK", b"proxies: []\n").await;
        let out = fetcher().fetch(&format!("http://{addr}/sub")).await.unwrap();
        assert_eq!(out, "proxies: []\n");
    }

    #[tokio::test]
    async fn fetch_with_info_parses_filename_star() {
        let addr = serve_once_headers(
            "200 OK",
            "Content-Disposition: attachment; filename*=UTF-8''My%20Sub.yaml\r\n",
            b"proxies: []\n",
        )
        .await;
        let (body, info, filename) = fetcher().fetch_with_info(&format!("http://{addr}/sub")).await.unwrap();
        assert_eq!(body, "proxies: []\n");
        assert_eq!(info, SubscriptionInfo::default());
        assert_eq!(filename.as_deref(), Some("My Sub.yaml"));
    }

    #[tokio::test]
    async fn fetch_with_info_parses_plain_filename() {
        let addr = serve_once_headers(
            "200 OK",
            "Content-Disposition: attachment; filename=\"sub.yaml\"\r\n",
            b"proxies: []\n",
        )
        .await;
        let (_, _, filename) = fetcher().fetch_with_info(&format!("http://{addr}/sub")).await.unwrap();
        assert_eq!(filename.as_deref(), Some("sub.yaml"));
    }

    #[tokio::test]
    async fn fetch_with_info_no_disposition_is_none() {
        let addr = serve_once("200 OK", b"proxies: []\n").await;
        let (_, _, filename) = fetcher().fetch_with_info(&format!("http://{addr}/sub")).await.unwrap();
        assert_eq!(filename, None);
    }

    #[test]
    fn parse_content_disposition_prefers_filename_star() {
        assert_eq!(
            parse_content_disposition("attachment; filename=\"a.yaml\"; filename*=UTF-8''My%20Sub.yaml"),
            Some("My Sub.yaml".to_string()),
        );
        assert_eq!(
            parse_content_disposition("attachment; filename*=UTF-8''%E6%B5%8B%E8%AF%95%20%E8%8A%82%E7%82%B9.yaml"),
            Some("测试 节点.yaml".to_string()),
        );
    }

    #[test]
    fn parse_content_disposition_missing_is_none() {
        assert_eq!(parse_content_disposition("attachment"), None);
        assert_eq!(parse_content_disposition(""), None);
    }

    #[test]
    fn default_name_uses_filename_first() {
        assert_eq!(
            default_name(Some("机场A.yaml"), "https://example.com/sub"),
            "机场A.yaml"
        );
        // 空/空白 filename 视为缺失，回退 URL 最后一段
        assert_eq!(default_name(Some("   "), "https://example.com/sub-a"), "sub-a");
    }

    #[test]
    fn default_name_falls_back_to_url_last_segment() {
        assert_eq!(default_name(None, "https://example.com/sub-a?x=1"), "sub-a");
        // percent-decode 最后一段（对齐 clash-verge-rev get_last_part_and_decode）
        assert_eq!(default_name(None, "https://example.com/My%20Sub"), "My Sub");
        // 无路径段 → 兜底
        assert_eq!(default_name(None, "not a url"), "订阅");
        assert_eq!(default_name(None, "https://example.com/"), "订阅");
    }

    #[tokio::test]
    async fn fetch_non_2xx_is_status_error() {
        let addr = serve_once("404 Not Found", b"nope").await;
        let err = fetcher().fetch(&format!("http://{addr}/")).await.unwrap_err();
        assert_eq!(err, DownloadError::HttpStatus(404));
    }

    #[tokio::test]
    async fn fetch_empty_body_is_empty_error() {
        let addr = serve_once("200 OK", b"").await;
        let err = fetcher().fetch(&format!("http://{addr}/")).await.unwrap_err();
        assert_eq!(err, DownloadError::Empty);
    }

    #[tokio::test]
    async fn fetch_oversize_body_is_too_large() {
        let body = vec![b'x'; 9 * 1024 * 1024];
        let addr = serve_once("200 OK", &body).await;
        let err = fetcher().fetch(&format!("http://{addr}/")).await.unwrap_err();
        assert_eq!(err, DownloadError::TooLarge(8 * 1024 * 1024));
    }

    #[tokio::test]
    async fn fetch_non_utf8_is_invalid_utf8() {
        let addr = serve_once("200 OK", &[0xff, 0xfe, 0x00]).await;
        let err = fetcher().fetch(&format!("http://{addr}/")).await.unwrap_err();
        assert_eq!(err, DownloadError::InvalidUtf8);
    }

    #[tokio::test]
    async fn fetch_malformed_url_errors_without_network() {
        let err = fetcher().fetch("not a url").await.unwrap_err();
        assert!(matches!(err, DownloadError::Request(_)), "got {err:?}");
    }

    #[test]
    fn parse_subscription_userinfo_extracts_fields() {
        let info = parse_subscription_userinfo("upload=100; download=200; total=300; expire=1700000000");
        assert_eq!(info.upload, 100);
        assert_eq!(info.download, 200);
        assert_eq!(info.total, 300);
        assert_eq!(info.expire, Some(1_700_000_000));
    }

    #[test]
    fn parse_subscription_userinfo_missing_and_zero_expire() {
        let info = parse_subscription_userinfo("upload=1; download=2; total=3; expire=0");
        assert_eq!(info.expire, None);
        assert_eq!(parse_subscription_userinfo("garbage"), SubscriptionInfo::default());
    }
}

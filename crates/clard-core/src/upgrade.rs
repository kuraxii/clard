//! mihomo 核心升级下载（doc/05 §7 R7.3，TUI 侧）：二进制下载（.gz 自动解压）、
//! 期望 sha256 获取（mihomo release 惯例 `<url>.sha256sum`）、本地校验。
//!
//! 边界：本模块只负责「拿到合法、哈希匹配的安装包字节」；安装（helper 复核 + 原子替换
//! `/var/clard/bin/mihomo`）经 IPC `InstallCore` 由 helper 完成（doc/01 §5.1）。

use std::time::Duration;

use thiserror::Error;

/// 核心安装包下载上限（mihomo release 通常 < 100MB，gzip 后更小）。
pub const CORE_DOWNLOAD_LIMIT: u64 = 256 * 1024 * 1024;
/// `.sha256sum` 文件上限（纯文本）。
const SHA256SUM_LIMIT: u64 = 64 * 1024;
/// 下载超时。
const TIMEOUT: Duration = Duration::from_secs(60);

/// 升级下载失败（对用户可读）。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UpgradeError {
    #[error("请求失败: {0}")]
    Request(String),
    #[error("HTTP 状态异常: {0}")]
    HttpStatus(u16),
    #[error("响应体为空")]
    Empty,
    #[error("响应体超过上限 {0} 字节")]
    TooLarge(u64),
    #[error("期望哈希获取失败（{url} 不可用）: {msg}")]
    ShaSum { url: String, msg: String },
    #[error("期望哈希格式无法解析（{line:?}，应为 \"<hex>  <file>\"）")]
    ShaSumFormat { line: String },
    #[error("校验和失败：期望 {expected}，实际 {actual}")]
    ShaMismatch { expected: String, actual: String },
    #[error("gzip 解压失败: {0}")]
    Gzip(String),
}

/// sha256 十六进制。
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// 下载核心二进制：`url` 指向 mihomo 可执行文件；`.gz` 结尾自动解压。
pub async fn download_core(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, UpgradeError> {
    let raw = fetch_bytes(client, url, CORE_DOWNLOAD_LIMIT).await?;
    let data = if url.ends_with(".gz") {
        gzip_decode(&raw).map_err(UpgradeError::Gzip)?
    } else {
        raw
    };
    if data.is_empty() {
        return Err(UpgradeError::Empty);
    }
    Ok(data)
}

/// 获取期望 sha256（`<url>.sha256sum`，第一行 `<hex>  <filename>`）。
pub async fn fetch_expected_sha256(
    client: &reqwest::Client,
    url: &str,
) -> Result<String, UpgradeError> {
    let sum_url = format!("{url}.sha256sum");
    let bytes = fetch_bytes(client, &sum_url, SHA256SUM_LIMIT).await.map_err(|e| {
        UpgradeError::ShaSum {
            url: sum_url.clone(),
            msg: match e {
                UpgradeError::HttpStatus(s) => format!("HTTP {s}"),
                other => other.to_string(),
            },
        }
    })?;
    let text = String::from_utf8(bytes).map_err(|_| UpgradeError::ShaSum {
        url: sum_url,
        msg: "非 UTF-8".into(),
    })?;
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
    parse_sha256sum_line(line).ok_or_else(|| UpgradeError::ShaSumFormat {
        line: line.to_string(),
    })
}

/// 解析 `<hex>  <filename>` 行的 hex 部分（纯函数，可单测）。
pub fn parse_sha256sum_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.len() < 64 {
        return None;
    }
    let hex = &line[..64];
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

/// 下载 + 期望哈希 + 本地校验，返回 (安装包字节, 期望哈希)。哈希不符即拒绝。
pub async fn download_and_verify(
    client: &reqwest::Client,
    url: &str,
) -> Result<(Vec<u8>, String), UpgradeError> {
    let expected = fetch_expected_sha256(client, url).await?;
    let bytes = download_core(client, url).await?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(UpgradeError::ShaMismatch { expected, actual });
    }
    Ok((bytes, expected))
}

async fn fetch_bytes(
    client: &reqwest::Client,
    url: &str,
    limit: u64,
) -> Result<Vec<u8>, UpgradeError> {
    let resp = client
        .get(url)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| UpgradeError::Request(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(UpgradeError::HttpStatus(resp.status().as_u16()));
    }
    // Content-Length 预判超限（避免下载超大文件）
    if let Some(len) = resp.content_length()
        && len > limit
    {
        return Err(UpgradeError::TooLarge(limit));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| UpgradeError::Request(e.to_string()))?
        .to_vec();
    if bytes.len() as u64 > limit {
        return Err(UpgradeError::TooLarge(limit));
    }
    Ok(bytes)
}

fn gzip_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// 本地 HTTP 服务：按路径分发（`/core` 主体、`/core.sha256sum` 哈希）；accept 循环处理多次请求。
    async fn serve_router(
        bin: Vec<u8>,
        sha_file: Option<String>,
        gzip: bool,
    ) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..4 {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 8192];
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
                let req = String::from_utf8_lossy(&buf[..read]);
                let path = req
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();

                let (status, body) = if path.ends_with(".sha256sum") {
                    match &sha_file {
                        Some(t) => ("200 OK", t.as_bytes().to_vec()),
                        None => ("404 Not Found", b"nope".to_vec()),
                    }
                } else if gzip {
                    use std::io::Write;
                    let mut enc =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                    enc.write_all(&bin).unwrap();
                    ("200 OK", enc.finish().unwrap())
                } else {
                    ("200 OK", bin.clone())
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&body).await;
                let _ = sock.shutdown().await;
            }
        });
        addr
    }

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    #[test]
    fn parse_sha256sum_line_extracts_hex() {
        let line = format!("{}  mihomo-linux-amd64-v1.19.2.gz", "a".repeat(64));
        assert_eq!(parse_sha256sum_line(&line), Some("a".repeat(64)));
        assert_eq!(parse_sha256sum_line("not a hash"), None);
        assert_eq!(parse_sha256sum_line(&"z".repeat(64)), None, "非法 hex");
        assert_eq!(parse_sha256sum_line(&"a".repeat(63)), None, "长度不足");
        // 大小写归一化
        let upper = format!("{}  x", "A".repeat(64));
        assert_eq!(parse_sha256sum_line(&upper), Some("a".repeat(64)));
    }

    #[test]
    fn sha256_hex_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn download_core_plain_binary() {
        let bin = vec![0x7f, b'E', b'L', b'F', 1, 2, 3];
        let addr = serve_router(bin.clone(), None, false).await;
        let out = download_core(&client(), &format!("http://{addr}/core")).await.unwrap();
        assert_eq!(out, bin);
    }

    #[tokio::test]
    async fn download_core_gunzips_gz() {
        use std::io::Write;
        let bin = b"mihomo-binary".to_vec();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&bin).unwrap();
        let gz = enc.finish().unwrap();
        let addr = serve_router(gz, None, false).await;
        // 服务端给的是 gz 字节；URL 带 .gz → 解压
        let out = download_core(&client(), &format!("http://{addr}/mihomo.gz")).await.unwrap();
        assert_eq!(out, bin);
    }

    #[tokio::test]
    async fn download_and_verify_ok_with_gzip() {
        let bin = b"mihomo-v9.9.9".to_vec();
        let sha = sha256_hex(&bin);
        let sha_file = format!("{sha}  mihomo.gz\n");
        let addr = serve_router(bin.clone(), Some(sha_file), true).await;
        let url = format!("http://{addr}/mihomo-linux-amd64-v9.9.9.gz");
        let (bytes, expected) = download_and_verify(&client(), &url).await.unwrap();
        assert_eq!(bytes, bin);
        assert_eq!(expected, sha);
    }

    #[tokio::test]
    async fn download_and_verify_sha_mismatch_rejected() {
        let bin = b"mihomo".to_vec();
        let sha_file = format!("{}  x.gz\n", "b".repeat(64));
        let addr = serve_router(bin, Some(sha_file), false).await;
        let err = download_and_verify(&client(), &format!("http://{addr}/core"))
            .await
            .unwrap_err();
        assert!(matches!(err, UpgradeError::ShaMismatch { .. }), "{err}");
    }

    #[tokio::test]
    async fn download_and_verify_missing_sha_file_rejected() {
        let bin = b"mihomo".to_vec();
        let addr = serve_router(bin, None, false).await;
        let err = download_and_verify(&client(), &format!("http://{addr}/core.gz"))
            .await
            .unwrap_err();
        assert!(matches!(err, UpgradeError::ShaSum { .. }), "{err}");
    }

    #[tokio::test]
    async fn download_core_content_length_over_limit_rejected() {
        let addr = serve_router(vec![0u8; 100], None, false).await;
        // 10 字节上限：Content-Length 预判拒绝
        let err = fetch_bytes(&client(), &format!("http://{addr}/big"), 10).await.unwrap_err();
        assert_eq!(err, UpgradeError::TooLarge(10));
    }
}

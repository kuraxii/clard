//! mihomo 核心升级下载（doc/05 §7 R7.3，TUI 侧）：GitHub 最新 release 自动获取、
//! 二进制下载（.gz 自动解压）、本地校验、inbox 转交前的哈希计算。
//!
//! 边界：本模块只负责「拿到合法、哈希匹配的安装包字节」；安装（helper 复核 + 原子替换
//! `/var/clard/bin/mihomo`）经 IPC `InstallCore` 由 helper 完成（doc/01 §5.1）。
//!
//! 哈希策略：MetaCubeX 官方 release **不提供**独立 sha256sum 文件，故 TUI 下载后自算
//! sha256 作为期望哈希交 helper 复核——保证「下载内容 → inbox → helper 原子替换」链路
//! 完整（HTTPS 传输完整性 + helper 复核），供应链信任边界与 RPM 包内 mihomo 同级。

use std::time::Duration;

use thiserror::Error;

/// GitHub 最新 release API（测试可注入 mock 地址）。
pub const GITHUB_API_URL: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest";

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
    #[error("GitHub release API 获取失败: {0}")]
    Release(String),
    #[error("不支持的架构（{0}），mihomo 仅提供 amd64/arm64")]
    UnsupportedArch(String),
    #[error("release {tag} 未找到 {arch} 资产（{asset:?}）")]
    AssetNotFound { tag: String, arch: String, asset: String },
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

/// 最新 release 信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// 版本 tag（如 `v1.19.30`）
    pub tag: String,
    /// 资产下载 URL（`mihomo-linux-{arch}-{tag}.gz` 默认变体）
    pub asset_url: String,
}

/// 本机架构 → mihomo 资产架构名（amd64/arm64）。
pub fn asset_arch() -> Result<&'static str, UpgradeError> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        other => Err(UpgradeError::UnsupportedArch(other.to_string())),
    }
}

/// 从 GitHub release API 解析最新版本与对应资产 URL。
/// `api_url` 生产为 [`GITHUB_API_URL`]，测试注入 mock 地址。
pub async fn fetch_latest_release(client: &reqwest::Client, api_url: &str) -> Result<ReleaseInfo, UpgradeError> {
    let arch = asset_arch()?;
    let resp = client
        .get(api_url)
        .timeout(TIMEOUT)
        .header(reqwest::header::USER_AGENT, "clard")
        .send()
        .await
        .map_err(|e| UpgradeError::Release(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(UpgradeError::Release(format!("HTTP {}", resp.status().as_u16())));
    }
    let body = resp.bytes().await.map_err(|e| UpgradeError::Release(e.to_string()))?;
    let v: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| UpgradeError::Release(format!("JSON 解析失败: {e}")))?;
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or_else(|| UpgradeError::Release("缺少 tag_name".into()))?
        .to_string();
    let want = format!("mihomo-linux-{arch}-{tag}.gz");
    let asset_url = v
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|assets| {
            assets
                .iter()
                .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(want.as_str()))
                .and_then(|a| a.get("browser_download_url").and_then(|u| u.as_str()))
        })
        .ok_or_else(|| UpgradeError::AssetNotFound {
            tag: tag.clone(),
            arch: arch.to_string(),
            asset: want,
        })?
        .to_string();
    Ok(ReleaseInfo { tag, asset_url })
}

/// 下载最新 release 核心：获取 release 信息 → 下载（gzip 解压）→ 自算 sha256。
/// 返回 (安装包字节, release 信息, sha256)。
pub async fn download_latest(
    client: &reqwest::Client,
    api_url: &str,
) -> Result<(Vec<u8>, ReleaseInfo, String), UpgradeError> {
    let info = fetch_latest_release(client, api_url).await?;
    let bytes = download_core(client, &info.asset_url).await?;
    let sha = sha256_hex(&bytes);
    Ok((bytes, info, sha))
}

/// 获取期望 sha256（`<url>.sha256sum`，第一行 `<hex>  <filename>`）。
pub async fn fetch_expected_sha256(client: &reqwest::Client, url: &str) -> Result<String, UpgradeError> {
    let sum_url = format!("{url}.sha256sum");
    let bytes = fetch_bytes(client, &sum_url, SHA256SUM_LIMIT)
        .await
        .map_err(|e| UpgradeError::ShaSum {
            url: sum_url.clone(),
            msg: match e {
                UpgradeError::HttpStatus(s) => format!("HTTP {s}"),
                other => other.to_string(),
            },
        })?;
    let text = String::from_utf8(bytes).map_err(|_| UpgradeError::ShaSum {
        url: sum_url,
        msg: "非 UTF-8".into(),
    })?;
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
    parse_sha256sum_line(line).ok_or_else(|| UpgradeError::ShaSumFormat { line: line.to_string() })
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
pub async fn download_and_verify(client: &reqwest::Client, url: &str) -> Result<(Vec<u8>, String), UpgradeError> {
    let expected = fetch_expected_sha256(client, url).await?;
    let bytes = download_core(client, url).await?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(UpgradeError::ShaMismatch { expected, actual });
    }
    Ok((bytes, expected))
}

async fn fetch_bytes(client: &reqwest::Client, url: &str, limit: u64) -> Result<Vec<u8>, UpgradeError> {
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
    async fn serve_router(bin: Vec<u8>, sha_file: Option<String>, gzip: bool) -> SocketAddr {
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
                    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
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
        let out = download_core(&client(), &format!("http://{addr}/mihomo.gz"))
            .await
            .unwrap();
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
        let err = fetch_bytes(&client(), &format!("http://{addr}/big"), 10)
            .await
            .unwrap_err();
        assert_eq!(err, UpgradeError::TooLarge(10));
    }

    /// mock GitHub release API：`/latest` 返回 release JSON（资产 URL 指向自身 `/asset.gz`），
    /// `/asset.gz` 返回 gzip 主体。
    async fn serve_release_api(arch: &str, tag: &str, payload: &[u8], with_asset: bool) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let arch = arch.to_string();
        let tag = tag.to_string();
        let payload = payload.to_vec();
        let with_asset = with_asset;
        tokio::spawn(async move {
            use std::io::Write;
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&payload).unwrap();
            let gz = enc.finish().unwrap();
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
                let (status, body): (&str, Vec<u8>) = if path == "/latest" {
                    let asset_name = format!("mihomo-linux-{arch}-{tag}.gz");
                    let json = if with_asset {
                        format!(
                            r#"{{"tag_name":"{tag}","assets":[{{"name":"{asset_name}","browser_download_url":"{base}/asset.gz"}}]}}"#
                        )
                    } else {
                        format!(r#"{{"tag_name":"{tag}","assets":[]}}"#)
                    };
                    ("200 OK", json.into_bytes())
                } else if path == "/asset.gz" {
                    ("200 OK", gz.clone())
                } else {
                    ("404 Not Found", b"nope".to_vec())
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

    #[tokio::test]
    async fn fetch_latest_release_parses_mock_api() {
        let addr = serve_release_api("amd64", "v1.19.30", b"bin", true).await;
        let info = fetch_latest_release(&client(), &format!("http://{addr}/latest"))
            .await
            .unwrap();
        assert_eq!(info.tag, "v1.19.30");
        assert_eq!(info.asset_url, format!("http://{addr}/asset.gz"));
    }

    #[tokio::test]
    async fn fetch_latest_release_missing_asset_errors() {
        let addr = serve_release_api("amd64", "v1.19.30", b"bin", false).await;
        let err = fetch_latest_release(&client(), &format!("http://{addr}/latest"))
            .await
            .unwrap_err();
        assert!(matches!(err, UpgradeError::AssetNotFound { .. }), "{err}");
    }

    #[tokio::test]
    async fn fetch_latest_release_bad_json_errors() {
        // 复用 serve_router：返回裸字节（非 JSON）→ Release 错误
        let addr = serve_router(vec![b'x'; 8], None, false).await;
        let err = fetch_latest_release(&client(), &format!("http://{addr}/latest"))
            .await
            .unwrap_err();
        assert!(matches!(err, UpgradeError::Release(_)), "{err}");
    }

    #[tokio::test]
    async fn download_latest_full_flow_with_gzip() {
        let bin = b"mihomo-auto-upgrade".to_vec();
        let addr = serve_release_api("amd64", "v2.0.0", &bin, true).await;
        let (bytes, info, sha) = download_latest(&client(), &format!("http://{addr}/latest"))
            .await
            .unwrap();
        assert_eq!(bytes, bin);
        assert_eq!(info.tag, "v2.0.0");
        assert_eq!(sha, sha256_hex(&bin));
    }

    #[test]
    fn asset_arch_maps_host() {
        // 本机编译架构必须被支持（x86_64→amd64 / aarch64→arm64）
        assert!(asset_arch().is_ok());
    }

    #[tokio::test]
    #[ignore = "需要外网（GitHub API）；手动验证真实 release 兼容性"]
    async fn fetch_latest_release_live() {
        let info = fetch_latest_release(&client(), GITHUB_API_URL).await.unwrap();
        assert!(info.tag.starts_with('v'));
        assert!(info.asset_url.contains(&info.tag));
    }
}

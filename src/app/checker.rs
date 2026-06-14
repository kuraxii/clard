use std::time::Duration;

use reqwest::{Client, Proxy};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct IPInfo {
    pub ip: String,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnlockStatus {
    Testing,
    Unlocked(String), // e.g. "US"
    OriginalsOnly,
    Blocked(String), // Reason
    Error(String),
}

#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub direct_ip: Option<IPInfo>,
    pub proxy_ip: Option<IPInfo>,
    pub youtube_status: UnlockStatus,
    pub netflix_status: UnlockStatus,
    pub spotify_status: UnlockStatus,
    pub bilibili_status: UnlockStatus,
}

impl Default for AnalysisResult {
    fn default() -> Self {
        Self {
            direct_ip: None,
            proxy_ip: None,
            youtube_status: UnlockStatus::Testing,
            netflix_status: UnlockStatus::Testing,
            spotify_status: UnlockStatus::Testing,
            bilibili_status: UnlockStatus::Testing,
        }
    }
}

pub fn create_direct_client() -> reqwest::Result<Client> {
    Client::builder().no_proxy().timeout(Duration::from_secs(5)).build()
}

pub fn create_proxy_client(proxy_url: &str) -> reqwest::Result<Client> {
    let proxy = Proxy::all(proxy_url)?;
    Client::builder().proxy(proxy).timeout(Duration::from_secs(10)).build()
}

#[derive(Deserialize)]
struct IpSbResponse {
    ip: String,
    country: String,
    // other fields ignored
}

pub async fn check_ip(client: &Client) -> reqwest::Result<IPInfo> {
    // Using ip.sb API for easy JSON parsing, or similar
    let resp: IpSbResponse = client
        .get("https://api.ip.sb/geoip")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?
        .json()
        .await?;

    Ok(IPInfo {
        ip: resp.ip,
        region: resp.country,
    })
}

pub async fn check_ip_direct() -> Result<IPInfo, String> {
    let client = create_direct_client().map_err(|e| e.to_string())?;
    check_ip(&client).await.map_err(|e| e.to_string())
}

pub async fn check_ip_proxy(proxy_url: &str) -> Result<IPInfo, String> {
    let client = create_proxy_client(proxy_url).map_err(|e| e.to_string())?;
    check_ip(&client).await.map_err(|e| e.to_string())
}

// Stubs for the streaming tests
pub async fn check_netflix(client: &Client) -> UnlockStatus {
    let url_original = "https://www.netflix.com/title/80018499";
    let url_non_original = "https://www.netflix.com/title/70143836";

    let resp_orig = client
        .get(url_original)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await;
    match resp_orig {
        Ok(resp) => {
            if resp.status().as_u16() == 403 {
                return UnlockStatus::Blocked("403 Forbidden".into());
            }
            // Check non-original
            let resp_non_orig = client
                .get(url_non_original)
                .header("User-Agent", "Mozilla/5.0")
                .send()
                .await;
            if let Ok(r) = resp_non_orig {
                if r.status().as_u16() == 404 {
                    UnlockStatus::OriginalsOnly
                } else if r.status().as_u16() == 200 {
                    UnlockStatus::Unlocked("Yes".into())
                } else {
                    UnlockStatus::Error(format!("Status {}", r.status()))
                }
            } else {
                UnlockStatus::OriginalsOnly
            }
        }
        Err(e) => UnlockStatus::Error(e.to_string()),
    }
}

pub async fn check_youtube(client: &Client) -> UnlockStatus {
    match client
        .get("https://www.youtube.com/premium")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
    {
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("Premium is not available in your country") {
                UnlockStatus::Blocked("Premium Not Available".into())
            } else if let Some(idx) = body.find("\"GL\":\"") {
                let region = &body[idx + 6..idx + 8];
                UnlockStatus::Unlocked(region.into())
            } else {
                UnlockStatus::Blocked("Unknown Region".into())
            }
        }
        Err(e) => UnlockStatus::Error(e.to_string()),
    }
}

pub async fn check_spotify(client: &Client) -> UnlockStatus {
    match client
        .get("https://open.spotify.com/")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
    {
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("Spotify is currently not available in your country") {
                UnlockStatus::Blocked("Not Available".into())
            } else {
                UnlockStatus::Unlocked("Yes".into())
            }
        }
        Err(e) => UnlockStatus::Error(e.to_string()),
    }
}

pub async fn check_bilibili(client: &Client) -> UnlockStatus {
    match client
        .get("https://api.bilibili.com/pgc/player/web/playurl?cid=1")
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
    {
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("\"code\":0") {
                UnlockStatus::Unlocked("Mainland".into())
            } else {
                UnlockStatus::Blocked("Restricted".into())
            }
        }
        Err(e) => UnlockStatus::Error(e.to_string()),
    }
}

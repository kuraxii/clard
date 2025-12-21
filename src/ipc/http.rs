use reqwest::{Client, Method, Request, Url};
use serde::{Deserialize, Serialize};

use crate::ipc::error::{IpcError, Result};

/// unix domain socket
pub mod blocking {
    use super::*;
    /// 从 unix domain socket直接获取请求
    pub fn get_uds<T>(usd_path: &str, url: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let client = reqwest::blocking::Client::builder().unix_socket(usd_path).build()?;
        let response = client.get(url).send()?.error_for_status()?;
        Ok(response.json::<T>()?)
    }
}

/// 构建 request
pub fn build_request(method: Method, path: &str) -> Result<Request> {
    let authority = get_http_url(path);
    let url = Url::parse(&authority).map_err(|_|IpcError::InvalidUrl)?;
    Ok(Request::new(method, url))
}

/// http url, localhost only
/// todo Validate using URI
#[inline]
pub fn get_http_url(suffix: &str) -> String {
    let clean_suffix = suffix.trim_start_matches('/');
    format!("http://localhost/{}", clean_suffix)
}

use reqwest::{Client, Method, Request, Url};
use serde::{Deserialize, Serialize};

use crate::ipc::error::{IpcError, Result};



pub async fn get_uds<T>(usd_path: &str, url: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let client = reqwest::Client::builder().unix_socket(usd_path).build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.json::<T>().await?)
}

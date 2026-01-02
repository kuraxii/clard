use std::{net::SocketAddr, path::PathBuf};

use dashmap::DashMap;
use http::Method;
use rand::random;
use reqwest::{Client, RequestBuilder, Url};
use serde::de::DeserializeOwned;
use tokio::sync::mpsc::UnboundedSender;

use super::{
    error::{IpcError, Result},
    models::{BackendVersion, ResponseError, Groups},
    websocket::WsControl,
};
use crate::ipc::websocket::{self, WebSocketMessage};

/// websocket id 通过id索引
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebSocketId(u32);

/// 后端类型
/// 支持 uds TCP
#[derive(Debug, Clone)]
pub enum Protocol {
    /// unix domain socket
    UDS(PathBuf),
    /// tcp/ip socket
    TCP(SocketAddr),
}




/// `BackendBuilder` 用于配置并构建 [`Backend`] 实例。
#[derive(Default)]
pub struct BackendBuilder {
    unix_path: Option<String>,
    tcp_addr: Option<String>,
}

impl BackendBuilder {
    /// 设置 Unix Domain Socket 路径（如 `/tmp/mihomo.sock`）。
    pub fn set_unix_socket(mut self, socket_path: &str) -> Self {
        self.unix_path = Some(socket_path.to_string());
        self
    }

    /// 设置 TCP 连接地址（如 `127.0.0.1:8080`）。
    pub fn set_tcp_addr(mut self, addr: &str) -> Self {
        self.tcp_addr = Some(addr.to_string());
        self
    }

    /// 构建 Backend 实例。若 UDS 和 TCP 同时存在，优先使用 UDS。
    pub fn build(self) -> Result<Backend> {
        let protocol = match (self.unix_path, self.tcp_addr) {
            (Some(path), _) => Protocol::UDS(PathBuf::from(path)),
            (None, Some(addr_str)) => {
                let socket_addr = addr_str
                    .parse::<SocketAddr>()
                    .map_err(|e| IpcError::FailedBackend(format!("无效的 TCP 地址: {}", e)))?;
                Protocol::TCP(socket_addr)
            }
            (None, None) => {
                return Err(IpcError::FailedBackend("未设置任何后端类型".to_owned()));
            }
        };

        Ok(Backend {
            protocol: protocol.clone(),
            client: match protocol {
                Protocol::UDS(unix_path) => Client::builder().unix_socket(unix_path).build()?,
                Protocol::TCP(_) => Client::new(),
            },
            ws_connects: DashMap::new(),
        })
    }
}

/// mihomo 后端管理
#[derive(Debug)]
pub struct Backend {
    protocol: Protocol,
    client: Client,
    ws_connects: DashMap<WebSocketId, UnboundedSender<WsControl>>,
}

impl Backend {
    /// 建造者 backend 的唯一初始化方法
    pub fn builder() -> BackendBuilder {
        BackendBuilder::default()
    }

    /// 切换后端类型
    pub fn switch_backend() -> Result<()> {
        Ok(())
    }

    /// 获取 http url
    fn get_http_url(&self, suffix: &str) -> String {
        let suffix = suffix.trim_start_matches('/');
        match self.protocol {
            Protocol::TCP(addr) => format!("http://{}:{}/{}", addr.ip(), addr.port(), suffix),
            Protocol::UDS(_) => format!("http://localhost/{suffix}"),
        }
    }

    /// http 请求构建器
    fn build_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let url = self.get_http_url(path);
        Ok(match method {
            Method::GET => self.client.get(url),
            Method::PUT => self.client.put(url),
            Method::DELETE => self.client.delete(url),
            Method::PATCH => self.client.patch(url),
            Method::POST => self.client.post(url),
            _ => return Err(IpcError::MethodNotSupported(method.to_string())),
        })
    }

    /// 关闭 websocket 连接
    pub async fn close_connect(&self, id: WebSocketId) -> Result<()> {
        self.ws_connects
            .get(&id)
            .ok_or(IpcError::Other)?
            .send(WsControl::Close)
            .map_err(|e| IpcError::Mpsc(e.to_string()))
    }

    /// 连接websocket 自定义谓词
    pub async fn connect_to_websocket_with<T, F>(&self, url: Url, handle_message: F) -> Result<WebSocketId>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(WebSocketMessage<T>) + Send + 'static,
    {
        let id = WebSocketId(random());
        let sender = websocket::connect_with::<T, F>(self.protocol.clone(), url, handle_message).await?;
        self.ws_connects.insert(id, sender);
        Ok(id)
    }

    /// clash 业务实现

    /// 获取后端版本
    pub async fn get_version(&self) -> Result<BackendVersion> {
        let req = self.build_request(Method::GET, "version")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("get version failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<BackendVersion>().await?)
    }

    /// 清理 FakeIP 缓存
    pub async fn flush_fakeip(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/cache/fakeip/flush")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("flush fakeip failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

    /// 清理 DNS 缓存
    pub async fn flush_dns(&self) -> Result<()> {
        let req = self.build_request(Method::POST, "/cache/dns/flush")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("flush dns failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(())
    }

     /// 获取所有的代理组
    pub async fn get_groups(&self) -> Result<Groups> {
        let req = self.build_request(Method::GET, "/group")?;
        let res = req.send().await?;
        if !res.status().is_success() {
            let err_msg = res.json::<ResponseError>().await.map_or_else(
                |msg| format!("flush dns failed: {msg}"),
                |err| err.message.to_string(),
            );
            return Err(IpcError::ResponseError(err_msg));
        }
        Ok(res.json::<Groups>().await?)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn backend() -> Result<Backend> {
        Ok(Backend::builder()
            .set_unix_socket("/tmp/verge/verge-mihomo.sock")
            .build()?)
    }

    #[test]
    fn build_backend() -> Result<()> {
        let _ = backend();
        Ok(())
    }

    #[tokio::test]
    async fn test_get_version() -> Result<()> {
        let backend = backend()?;
        let result = backend.get_version().await;

        assert!(result.is_ok());
        let version = result.unwrap();
        println!("version: {:?}", version);
        Ok(())
    }

    #[tokio::test]
    async fn flush_fakeip() -> Result<()> {
        let backend = backend()?;
        let result = backend.flush_fakeip().await;
        assert!(result.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn flush_dns() -> Result<()> {
        let backend = backend()?;
        let result = backend.flush_dns().await;

        assert!(result.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn request_group() -> Result<()> {
        let backend = backend()?;
        let result = backend.get_groups().await;

        assert!(result.is_ok());
        println!("group: {:?}", result.unwrap());

        Ok(())
    }
}

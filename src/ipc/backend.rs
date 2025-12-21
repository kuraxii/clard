use std::{net::SocketAddr, path::PathBuf};

use dashmap::DashMap;
use http::Method;
use rand::random;
use reqwest::{Client, RequestBuilder, Url};
use tokio::sync::mpsc::UnboundedSender;

use super::{
    error::{IpcError, Result},
    websocket::WsControl,
};
use crate::ipc::websocket::{self, WebSocketMessage};

/// websocket id 通过id索引
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebSocketId(u32);

/// 后端类型
/// 支持 uds TCP
#[derive(Debug, Clone)]
pub enum BackendType {
    /// unix domain socket
    UDS(PathBuf),
    /// tcp/ip socket
    TCP(SocketAddr),
}

/// mihomo 后端管理
#[derive(Debug)]
pub struct Backend {
    backend_type: BackendType,
    client: Client,
    ws_connects: DashMap<WebSocketId, UnboundedSender<WsControl>>,
}

impl Backend {
    /// 初始化后端
    /// 建立reqwest client连接池
    /// 准备websocket管理池
    pub fn init(backend_type: BackendType) -> Result<Self> {
        Ok(Self {
            backend_type: backend_type.clone(),
            client: match backend_type {
                BackendType::UDS(unix_path) => Client::builder().unix_socket(unix_path).build()?,
                BackendType::TCP(_) => Client::new(),
            },
            ws_connects: DashMap::new(),
        })
    }

    /// http 请求构建器
    pub fn request(&self, method: Method, url: Url) -> Result<RequestBuilder> {
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
        T: serde::de::DeserializeOwned + Send + 'static,
        F: Fn(WebSocketMessage<T>) + Send + 'static,
    {
        let id = WebSocketId(random());
        let sender = websocket::connect_with::<T, F>(self.backend_type.clone(), url, handle_message).await?;
        self.ws_connects.insert(id, sender);
        Ok(id)
    }
}

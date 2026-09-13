//! helper RPC 客户端（clard-tui 侧；对端 clard-helper）。
//!
//! 访问控制（doc/01 §4.2）：socket 0666 默认全开放，无需任何凭据。
//! 帧格式：u32 BE 长度前缀 + JSON。

use std::{
    io,
    path::PathBuf,
};

use clard_proto::{Request, Response};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// RPC socket 路径：`CLARD_SOCKET` 覆盖，默认 `/run/clard/helper.sock`。
pub fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLARD_SOCKET") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("/run/clard/helper.sock")
}

/// 调用一次请求，返回对端响应（`Response::Error` 已转为 [`RpcError::Helper`]）。
pub async fn call(req: &Request) -> Result<Response, RpcError> {
    let mut stream = tokio::net::UnixStream::connect(socket_path())
        .await
        .map_err(|e| RpcError::Connect(e.to_string()))?;

    let buf = serde_json::to_vec(req).map_err(|e| RpcError::Encode(e.to_string()))?;
    stream.write_all(&(buf.len() as u32).to_be_bytes()).await?;
    stream.write_all(&buf).await?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response =
        serde_json::from_slice(&buf).map_err(|e| RpcError::Decode(e.to_string()))?;
    match resp {
        Response::Error { message } => Err(RpcError::Helper(message)),
        other => Ok(other),
    }
}

/// 期望无载荷成功；载荷不符视为协议错误。
pub fn expect_ok(resp: Response) -> Result<(), RpcError> {
    match resp {
        Response::Ok => Ok(()),
        other => Err(unexpected(other)),
    }
}

/// 响应变体与预期不符（`call` 已把 `Response::Error` 转为 `RpcError::Helper`）。
pub fn unexpected(resp: Response) -> RpcError {
    RpcError::Helper(format!("响应类型不匹配: {resp:?}"))
}

#[derive(Debug, Error)]
pub enum RpcError {
    #[error("无法连接后台服务: {0}（systemctl status clard-helper 或 i 安装）")]
    Connect(String),
    #[error("请求序列化失败: {0}")]
    Encode(String),
    #[error("响应解析失败: {0}")]
    Decode(String),
    #[error("IO 错误: {0}")]
    Io(#[from] io::Error),
    #[error("helper 返回错误: {0}")]
    Helper(String),
}

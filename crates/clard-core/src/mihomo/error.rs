use std::result;

use thiserror::Error;

pub type Result<T, E = IpcError> = result::Result<T, E>;

#[derive(Error, Debug)]
pub enum IpcError {
    #[error("网络连接失败 (UDS): {0}")]
    Connection(#[from] std::io::Error),

    #[error("HTTP错误 : {0}")]
    Http(#[from] http::Error),

    #[error("WebSocket 协议错误: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("JSON 序列化/反序列化失败: {0}")]
    Json(#[from] serde_json::Error),

    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("invalid url")]
    InvalidUrl,

    #[error("连接意外断开 (Stream End)")]
    StreamClosed,

    #[error("mpsc 错误: {0}")]
    Mpsc(String),

    #[error("不支持的方法: {0}")]
    MethodNotSupported(String),

    #[error("backend 初始化失败: {0}")]
    FailedBackend(String),

    /// 核心 API 返回错误（如 delay test 503）：与 backend 初始化无关，
    /// 文案必须准确（曾误写为「backend 初始化失败」导致排查误导）。
    #[error("核心 API 返回错误: {0}")]
    ResponseError(String),

    #[error("其他错误 todo: 待实现")]
    Other,
}

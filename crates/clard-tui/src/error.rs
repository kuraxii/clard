//! Error types

use thiserror::Error;

use clard_core::mihomo::error::IpcError;

pub type Result<T, E = ClardError> = std::result::Result<T, E>;
#[derive(Debug, Error)]
pub enum ClardError {
    #[error("HTTP错误 : {0}")]
    Http(#[from] reqwest::Error),

    #[error("Json : {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO错误 : {0}")]
    Io(#[from] std::io::Error),

    #[error("IPC Failed: {0}")]
    Ipc(#[from] IpcError),

    #[error("订阅下载失败: {0}")]
    Download(#[from] clard_core::profiles::DownloadError),

    #[error("配置生成失败: {0}")]
    ConfigGen(#[from] clard_config::config_gen::ConfigGenError),

    #[error("helper 调用失败: {0}")]
    Rpc(#[from] crate::rpc::RpcError),

    #[error("other error")]
    Other,
}

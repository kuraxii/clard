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

    #[error("配置管理错误: {0}")]
    Profiles(#[from] clard_core::profiles::ProfilesError),

    #[error("配置生成失败: {0}")]
    ConfigGen(#[from] clard_core::config_gen::ConfigGenError),

    #[error("other error")]
    Other,
}

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

    #[error("other error")]
    Other,
}

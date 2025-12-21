//! Error types

use thiserror::Error;

use crate::ipc::error::IpcError;

pub(crate) type Result<T, E = ClardError> = std::result::Result<T, E>;
#[derive(Debug, Error)]
pub(crate) enum ClardError {
    #[error("HTTP错误 : {0}")]
    Http(#[from] reqwest::Error),

    #[error("Json : {0}")]
    Json(#[from] serde_json::Error),

    #[error("IPC Failed: {0}")]
    Ipc(#[from] IpcError),
}


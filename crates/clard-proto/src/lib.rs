//! clard-proto：TUI ↔ helper 的 IPC 契约（两侧唯一耦合点）
//!
//! 方法清单与语义见 doc/01-方案设计.md §5.6；payload 类型随
//! 里程碑 M1（helper 骨架）逐步补全。协议不兼容时 `Hello` 握手返回
//! [`ProtoError::VersionMismatch`]。

#![forbid(unsafe_code)]
#![warn(
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications,
    clippy::perf,
    clippy::style,
    clippy::redundant_closure
)]

use serde::{Deserialize, Serialize};

/// 当前协议版本。任何不兼容变更都必须递增并在 `Hello` 握手中核对。
pub const PROTO_VERSION: u32 = 1;

/// 协议层错误
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("协议版本不兼容：helper={helper}，客户端={client}")]
    VersionMismatch { helper: u32, client: u32 },
}

/// TUI → helper 的请求方法（对应 doc/01 §5.6）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// 握手：核对 proto_version，返回 helper 版本 / authorized_uid / 能力位 / 当前状态
    Hello,
    /// 全量状态：核心状态 + TUN 状态 + 核心版本
    Status,
    /// 投递配置 bundle（yaml + 资产清单），见 §5.5
    ApplyConfig,
    /// 启停与重启核心（幂等）
    StartCore,
    StopCore,
    RestartCore,
    /// 开/关 TUN（托管字段注入 + 热重载 + 回读校验）
    SetTun,
    /// 手动兜底清理 TUN 残留（幂等）
    CleanupTun,
    /// 审计日志分页查询
    AuditQuery,
    /// 核心日志分页读取
    LogTail,
    /// 订阅事件流（断线重连后先 Status 全量同步再增量订阅）
    Subscribe,
    /// 升级核心（inbox 哈希校验 → 原子替换）
    InstallCore,
}

/// helper → TUI 的事件推送
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    CoreStatusChanged,
    TunChanged,
    LogLine,
    AuditLine,
    /// 核心反复崩溃，已 fail-open 恢复直连
    Degraded,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_version_is_stable_for_this_release() {
        assert_eq!(PROTO_VERSION, 1);
    }
}

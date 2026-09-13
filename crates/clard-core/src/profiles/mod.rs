//! 订阅配置（profiles）领域层：URL 导入 / 多配置管理 / 同 URL 覆盖更新 / 持久化。
//!
//! 所有权归 TUI（用户 XDG 目录，doc/01 §7 / §3.3）；helper 不读此目录
//! （`ProtectHome=yes`，配置经 IPC + inbox 投递）。

pub mod download;
pub mod store;

pub use download::{DownloadError, HttpFetcher, SubscriptionFetcher};
pub use store::{
    ImportOutcome, NodeSelection, Profile, ProfileKind, ProfilesError, ProfilesIndex,
    ProfilesStore, default_config_dir,
};

//! 订阅获取（download）：TUI 侧下载订阅原始内容（helper 不依赖本 crate）。
//!
//! 订阅内容的**存储与管理在 helper**（doc/01 §7）；TUI 只负责：
//! 下载 → `config_gen` 归一化/转换 → 经 IPC `ProfileImport` 提交给 helper。

pub mod download;

pub use download::{DownloadError, HttpFetcher, SubscriptionFetcher};

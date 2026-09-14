//! 设置页状态（doc/03 §5.7）：通用 / TUN / 核心 / 后台服务 / 备份 / 日志 / 关于。
//!
//! 当前为导航骨架；设置项与页签随「设置」增量补全。

/// 设置页状态。切换页面后保留内部状态（doc/03 §1 原则 3）。
#[derive(Debug, Default)]
pub struct SettingsState {}

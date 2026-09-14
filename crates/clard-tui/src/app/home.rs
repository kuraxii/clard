//! 主页状态（doc/03 §5.1）：一屏总览（后台服务 / 核心 / TUN / 当前配置 / 流量摘要）。
//!
//! 当前为导航骨架；状态字段随「主页」增量补全（IPC `Status`/`Hello`、核心版本、流量 WS）。

/// 主页状态。切换页面后保留内部状态（doc/03 §1 原则 3）。
#[derive(Debug, Default)]
pub struct HomeState {}

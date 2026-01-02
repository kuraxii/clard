//! ClardRs

// Tip: Deny warnings with `RUSTFLAGS="-D warnings"` environment variable in CI

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications,
    // --- 以下是为你新增的工程化建议 ---
    missing_debug_implementations, // 强制所有 pub 结构体必须派生 Debug，方便 TUI 调试
    clippy::perf,                  // 性能建议，避免不必要的 clone 或内存分配
    clippy::style,                 // 代码风格建议，让代码更符合社区习惯
    clippy::redundant_closure      // 移除多余的闭包调用
)]
// 如果某些函数确实不需要文档，可以用 #[allow(missing_docs)] 局部屏蔽，而不是全局关闭

pub mod application;
pub mod commands;
pub mod config;
pub mod error;
pub mod ipc;

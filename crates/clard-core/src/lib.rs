//! clard-core：领域逻辑（profile / mihomo 客户端 / logging / store）
//!
//! 分层约束（见 doc/01-方案设计.md §3.1）：
//! - 禁止依赖 ratatui，禁止任何特权代码；
//! - 只被 `clard-tui` 使用；`clard-helper` 不链接本 crate。

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


pub mod mihomo;
pub mod profiles;
pub mod upgrade;

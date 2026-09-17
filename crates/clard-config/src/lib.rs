//! clard-config：配置解析/生成（config_gen）。
//!
//! 独立 crate（doc/01 §3.1）：无 ratatui、无特权代码、无持久文件。
//! 被 `clard-tui` 使用（当前生成时机在 TUI 操作时）；`clard-helper`
//! 未来承担配置拼装（设置变更即重生成）时可按 §3.1 边界规则链接本 crate。

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(clippy::panic, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]
#![warn(
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications,
    clippy::perf,
    clippy::style,
    clippy::redundant_closure
)]

pub mod config_gen;

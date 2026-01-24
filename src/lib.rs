//! ClardRs

// Tip: Deny warnings with `RUSTFLAGS="-D warnings"` environment variable in CI

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications,
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
pub mod app;
pub mod event;
use serde_json::ser;
use tokio_util::sync::CancellationToken;

use tokio::sync::mpsc;

use error::Result;
use event::{ClardEvent, handle_key_event, handle_mouse_event};
use app::APP;

use crate::event::listen_input_event;


pub async fn start_clard() -> Result<()> {
    let (sender, mut receiver) = mpsc::channel::<ClardEvent>(32);
    let token = CancellationToken::new();

    let mut app = APP::init();

    tokio::spawn(listen_input_event(token.clone(), sender.clone()));

    loop{
        if let Some(recv) = receiver.recv().await{
            match recv{
                ClardEvent::Resize => {},
                ClardEvent::KeyInput(event) => {
                    handle_key_event(event, &mut app);
                },
                ClardEvent::PasteEvent(paste) => {}
                ClardEvent::MouseInput(event) => {
                    handle_mouse_event(event, &mut app);
                },
                ClardEvent::Terminal => {token.cancel(); break;}
            }
        }
    }



    Ok(())
}

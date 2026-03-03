//! ClardRs

// Tip: Deny warnings with `RUSTFLAGS="-D warnings"` environment variable in CI

#![forbid(unsafe_code)]
#![warn(
    rust_2018_idioms,
    trivial_casts,
    unused_lifetimes,
    unused_qualifications,
    clippy::perf,                  // 性能建议，避免不必要的 clone 或内存分配
    clippy::style,                 // 代码风格建议，让代码更符合社区习惯
    clippy::redundant_closure      // 移除多余的闭包调用
)]
// 如果某些函数确实不需要文档，可以用 #[allow(missing_docs)] 局部屏蔽，而不是全局关闭

pub mod app;
mod canvas;
pub mod commands;
pub mod config;

pub mod error;
pub mod event;
pub mod ipc;
use std::{
    io::stdout,
    panic::{self, PanicHookInfo},
};

use app::APP;
use canvas::Painter;
use crossterm::{
    self,
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use error::Result;
use event::{ClardEvent, handle_key_event, handle_mouse_event};
use ratatui::{Terminal, prelude::CrosstermBackend};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::event::listen_input_event;

/// A panic hook to properly restore the terminal in the case of a panic.
/// Originally based on [spotify-tui's implementation](https://github.com/Rigellute/spotify-tui/blob/master/src/main.rs).
fn panic_hook(panic_info: &PanicHookInfo<'_>) {
    let msg = match panic_info.payload().downcast_ref::<&'static str>() {
        Some(s) => *s,
        None => match panic_info.payload().downcast_ref::<String>() {
            Some(s) => &s[..],
            None => "Box<Any>",
        },
    };

    let backtrace = format!("{:?}", std::backtrace::Backtrace::capture());

    reset_stdout();

    // Print stack trace. Must be done after!
    if let Some(panic_info) = panic_info.location() {
        println!("thread '<unnamed>' panicked at '{msg}', {panic_info}\n\r{backtrace}")
    }

    // TODO: Might be cleaner in the future to use a cancellation token, but that causes some fun issues with
    // lifetimes; for now if it panics then shut down the main program entirely ASAP.
    std::process::exit(1);
}

/// This manually resets stdout back to normal state.
pub fn reset_stdout() {
    let mut stdout = stdout();
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show,
    );
}

fn init_terminal() -> Terminal<CrosstermBackend<std::io::Stdout>> {
    // 原始模式 原样捕捉所有的按键事件
    enable_raw_mode().unwrap();
    let mut stdout = stdout();
    execute!(
        stdout,
        Hide,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )
    .unwrap();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.clear().unwrap();
    terminal.hide_cursor().unwrap();
    terminal
}

/// 还原终端
fn reset_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) {
    disable_raw_mode().unwrap();
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    )
    .unwrap();
    terminal.show_cursor().unwrap();
}

use std::sync::Arc;
use crate::ipc::backend::Backend;

pub async fn start_clard() -> Result<()> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<ClardEvent>();
    let token = CancellationToken::new();

    // Try TCP by default, fallback to unix socket if needed
    // In a real app we would read config here
    let backend = Backend::builder().set_unix_socket("/tmp/verge/verge-mihomo.sock").build().unwrap_or_else(|_| {
        Backend::builder().set_unix_socket("/tmp/verge/verge-mihomo.sock").build().expect("Failed to build backend")
    });

    let mut app = APP::init(sender.clone(), Arc::new(backend));

    tokio::spawn(listen_input_event(token.clone(), sender.clone()));

    let mut painter = Painter::default();

    let mut terminal = init_terminal();

    panic::set_hook(Box::new(panic_hook));

    painter.draw(&mut terminal, &app);
    loop {
        if let Some(recv) = receiver.recv().await {
            match recv {
                ClardEvent::Resize => {}
                ClardEvent::KeyInput(event) => {
                    handle_key_event(event, &mut app, sender.clone());
                }
                ClardEvent::PasteEvent(_paste) => {}
                ClardEvent::MouseInput(event) => {
                    handle_mouse_event(event, &mut app);
                }
                ClardEvent::UpdateGroups(groups) => {
                    if let app::WindowState::Proxy(ref mut state) = app.current_page {
                        state.update_groups(groups);
                    }
                }
                ClardEvent::UpdateVersion(version) => {
                    if let app::WindowState::Preview(ref mut state) = app.current_page {
                        state.update_version(version);
                    }
                }
                ClardEvent::UpdateConnections(conns) => {
                    if let app::WindowState::Preview(ref mut state) = app.current_page {
                        state.update_connections(conns.clone());
                    } else if let app::WindowState::Connects(ref mut state) = app.current_page {
                        state.update_connections(conns);
                    }
                }
                ClardEvent::NodeTested(node, delay) => {
                    app.message = Some(format!("Node '{}' delay: {}ms", node, delay));
                }
                ClardEvent::Error(msg) => {
                    app.message = Some(msg);
                }
                ClardEvent::Terminal => {
                    break;
                }
            }
            painter.draw(&mut terminal, &app);
        }
    }

    // 退出循环 取消所有异步操作
    token.cancel();

    reset_terminal(&mut terminal);
    Ok(())
}

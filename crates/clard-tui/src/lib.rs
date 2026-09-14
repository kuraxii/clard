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
pub mod error;
pub mod event;
pub mod rpc;
use std::{
    io::stdout,
    panic::{self, PanicHookInfo},
    path::Path,
    time::Duration,
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
use tokio::{sync::mpsc, time::MissedTickBehavior};
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

fn init_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    // 原始模式 原样捕捉所有的按键事件
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(
        stdout,
        Hide,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

/// 还原终端
fn reset_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    )?;
    terminal.show_cursor()?;
    Ok(())
}

use std::sync::Arc;

use clard_core::mihomo::backend::Backend;

const DEFAULT_UNIX_SOCKET: &str = "/tmp/verge/verge-mihomo.sock";
const DEFAULT_TCP_ADDR: &str = "127.0.0.1:9090";

fn default_backend() -> Result<Backend> {
    let backend = if Path::new(DEFAULT_UNIX_SOCKET).exists() {
        Backend::builder().set_unix_socket(DEFAULT_UNIX_SOCKET).build()?
    } else {
        Backend::builder().set_tcp_addr(DEFAULT_TCP_ADDR)?.build()?
    };

    Ok(backend)
}

pub async fn start_clard() -> Result<()> {
    let (sender, mut receiver) = mpsc::unbounded_channel::<ClardEvent>();
    let token = CancellationToken::new();

    // In a real app we would read config here. For now, prefer the Verge socket
    // when it exists and otherwise use the common mihomo external controller port.
    let backend = default_backend()?;

    let mut app = APP::init(sender.clone(), Arc::new(backend.clone()));

    tokio::spawn(listen_input_event(token.clone(), sender.clone()));

    let mut painter = Painter;

    let mut terminal = init_terminal()?;

    panic::set_hook(Box::new(panic_hook));

    let mut connections_refresh = tokio::time::interval(Duration::from_secs(1));
    connections_refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);

    painter.draw(&mut terminal, &app);
    loop {
        tokio::select! {
            _ = connections_refresh.tick() => {
                if app.current_page == app::page::Page::Connections {
                    app.fetch_connections();
                }
            }
            recv = receiver.recv() => {
                let Some(recv) = recv else {
                    break;
                };

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
                        app.proxies.update_groups(groups);
                    }
                    ClardEvent::UpdateConnections(conns) => {
                        app.connections.update_connections(conns);
                    }
                    ClardEvent::UpdateTraffic(traffic) => {
                        app.connections.update_traffic(traffic);
                    }
                    ClardEvent::ProfilesUpdated { current, items } => {
                        app.apply_profiles(current, items);
                    }
                    ClardEvent::Notify(msg) => {
                        app.message = Some(msg);
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
    }

    // 退出循环 取消所有异步操作
    token.cancel();

    reset_terminal(&mut terminal)?;
    Ok(())
}

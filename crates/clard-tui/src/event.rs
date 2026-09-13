//! 处理按键 鼠标等事件

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use futures_util::StreamExt;
use tokio::{sync::mpsc, time::Instant};
use tokio_util::sync::CancellationToken;

use crate::app::APP;
#[derive(Debug)]
pub enum ClardEvent {
    Resize,
    KeyInput(KeyEvent),
    MouseInput(MouseEvent),
    PasteEvent(String),
    Terminal,
    UpdateGroups(clard_core::mihomo::models::Groups),
    UpdateConnections(clard_core::mihomo::models::Connections),
    UpdateTraffic(clard_core::mihomo::models::Traffic),
    NodeTested(String, u16),
    NetTestNodesReady(Vec<String>),
    NetTestError(String, String),
    AnalysisResultUpdated(Box<crate::app::checker::AnalysisResult>),
    Error(String),
}

pub fn handle_key_event(event: KeyEvent, app: &mut APP, sender: mpsc::UnboundedSender<ClardEvent>) {
    if app.show_help {
        if event.modifiers.is_empty() {
            match event.code {
                KeyCode::Esc => app.on_esc_key(),
                KeyCode::Char('?') | KeyCode::Char('q') => {
                    if let KeyCode::Char(c) = event.code {
                        app.on_char(c);
                    }
                }
                _ => {}
            }
        } else if event.modifiers == KeyModifiers::CONTROL && matches!(event.code, KeyCode::Char('c')) {
            tokio::spawn(async move {
                let _ = sender.send(ClardEvent::Terminal);
            });
        }
        return;
    }

    if event.modifiers.is_empty() {
        match event.code {
            KeyCode::Up => app.on_up_key(),
            KeyCode::Left => app.on_left_key(),
            KeyCode::Right => app.on_right_key(),
            KeyCode::Down => app.on_down_key(),
            KeyCode::Home => app.on_home_key(),
            KeyCode::End => app.on_end_key(),
            KeyCode::PageDown => app.on_pagedown_key(),
            KeyCode::PageUp => app.on_pageup_key(),
            KeyCode::Backspace => app.on_backspace_key(),
            KeyCode::Delete => app.on_delete_key(),
            KeyCode::Tab => app.on_tab_key(),
            KeyCode::Esc => app.on_esc_key(),
            KeyCode::Enter => app.on_enter_key(),
            KeyCode::Char(caught_cahr) => app.on_char(caught_cahr),
            _ => {}
        }
    } else {
        if let KeyModifiers::CONTROL = event.modifiers {
            match event.code {
                KeyCode::Char('c') => {
                    tokio::spawn(async move {
                        let _ = sender.send(ClardEvent::Terminal);
                    });
                }
                KeyCode::Char(_caught_cahr) => {}
                _ => {}
            }
        }
    }
}

pub fn handle_mouse_event(event: MouseEvent, _app: &mut APP) {
    match event.kind {
        MouseEventKind::ScrollUp => {}
        MouseEventKind::ScrollDown => {}
        MouseEventKind::Down(_button) => {}
        _ => {}
    }
}

/// 监听输入事件  按键、鼠标、粘贴
pub async fn listen_input_event(
    cancel_token: CancellationToken,
    clard_event_sender: mpsc::UnboundedSender<ClardEvent>,
) {
    let mut reader = EventStream::new();
    let mut mouse_timer = Instant::now();

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                break;
            }

            maybe_event = reader.next() => {
                match maybe_event{
                    Some(Ok(Event::Key(key_event))) if key_event.kind == KeyEventKind::Press => {
                        if clard_event_sender.send(ClardEvent::KeyInput(key_event)).is_err(){
                            break;
                        }
                    },
                    Some(Ok(Event::Mouse(mouse))) => {
                        match mouse.kind{
                            MouseEventKind::Moved | MouseEventKind::Drag(..) => {}
                            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                                if Instant::now().duration_since(mouse_timer).as_millis() >= 20
                                {
                                    if clard_event_sender.send(ClardEvent::MouseInput(mouse)).is_err() {
                                        break;
                                    }
                                    mouse_timer = Instant::now();
                                }
                                }
                                _ => {
                                    if clard_event_sender.send(ClardEvent::MouseInput(mouse)).is_err() {
                                        break;
                                    }
                                }
                        }
                    },
                    Some(Ok(Event::Resize(_, _))) => {
                        if clard_event_sender.send(ClardEvent::Resize).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Event::Paste(paste))) => {
                        if clard_event_sender.send(ClardEvent::PasteEvent(paste)).is_err(){
                            break;
                        }
                    }
                    Some(Ok(Event::FocusGained)) => {}
                    Some(Ok(Event::FocusLost)) => {}
                    Some(Ok(Event::Key(_))) => {}
                    Some(Err(e)) => {
                        // 打印错误或记录日志，防止因为一次读取失败导致死循环
                        eprintln!("Error reading input: {:?}", e);
                    }
                    None => {
                        break;
                    }
                }

            }
        }
    }
}

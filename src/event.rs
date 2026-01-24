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
}

pub fn handle_key_event(event: KeyEvent, app: &mut APP) {
    if event.modifiers.is_empty() {
        match event.code {
            KeyCode::Char('q') => {}
            KeyCode::Up => {}
            KeyCode::Left => {}
            KeyCode::Right => {}
            KeyCode::Down => {}
            KeyCode::Home => {}
            KeyCode::End => {}
            KeyCode::PageDown => {}
            KeyCode::PageUp => {}
            KeyCode::Backspace => {}
            KeyCode::Delete => {}
            KeyCode::Tab => {}
            KeyCode::Esc => {}
            KeyCode::Enter => {}
            KeyCode::Char(caught_cahr) => {}
            _ => {}
        }
    } else {
        if let KeyModifiers::CONTROL = event.modifiers {
            match event.code {
                KeyCode::Char('c') => {}
                KeyCode::Char(caught_cahr) => {}
                _ => {}
            }
        }
    }
}

pub fn handle_mouse_event(event: MouseEvent, app: &mut APP) {
    match event.kind {
        MouseEventKind::ScrollUp => {}
        MouseEventKind::ScrollDown => {}
        MouseEventKind::Down(button) => {}
        _ => {}
    }
}


/// 监听输入事件  按键、鼠标、粘贴
pub async fn listen_input_event(cancel_token: CancellationToken, clard_event_sender: mpsc::Sender<ClardEvent>) {
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
                        if clard_event_sender.send(ClardEvent::KeyInput(key_event)).await.is_err(){
                            break;
                        }
                    },
                    Some(Ok(Event::Mouse(mouse))) => {
                        match mouse.kind{
                            MouseEventKind::Moved | MouseEventKind::Drag(..) => {}
                            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                                if Instant::now().duration_since(mouse_timer).as_millis() >= 20
                                {
                                    if clard_event_sender.send(ClardEvent::MouseInput(mouse)).await.is_err() {
                                        break;
                                    }
                                    mouse_timer = Instant::now();
                                }
                                }
                                _ => {
                                    if clard_event_sender.send(ClardEvent::MouseInput(mouse)).await.is_err() {
                                        break;
                                    }
                                }
                        }
                    },
                    Some(Ok(Event::Resize(_, _))) => {
                        if clard_event_sender.send(ClardEvent::Resize).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Event::Paste(paste))) => {
                        if clard_event_sender.send(ClardEvent::PasteEvent(paste)).await.is_err(){
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

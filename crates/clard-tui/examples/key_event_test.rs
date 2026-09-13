use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use std::{collections::HashMap, io, time::Instant};
use tokio::signal::ctrl_c;

#[tokio::main]
async fn main() -> io::Result<()> {
    enable_raw_mode()?;

    println!("=== 真实按键事件测试 ===");
    println!("请连续按下方向键（上/下/左/右），观察事件行为");
    println!("按 Ctrl+C 退出测试");
    println!();

    let mut reader = EventStream::new();
    let mut key_press_times: HashMap<KeyCode, Instant> = HashMap::new();
    let mut event_count = 0u64;
    let start_time = Instant::now();

    // Ctrl+C 处理
    let (ctrl_c_tx, mut ctrl_c_rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        if ctrl_c().await.is_ok() {
            let _ = ctrl_c_tx.send(()).await;
        }
    });

    loop {
        tokio::select! {
            _ = ctrl_c_rx.recv() => {
                println!("\n\n=== 测试结束 ===\r");
                print_statistics(event_count, start_time);
                disable_raw_mode()?;
                return Ok(());
            }

            maybe_event = reader.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key_event))) => {
                        let now = Instant::now();
                        event_count += 1;

                        match key_event.kind {
                            KeyEventKind::Press => {
                                key_press_times.insert(key_event.code, now);
                                println!(
                                    "[{:6}] [Press]   {:?} at {:>6}ms\r",
                                    event_count,
                                    key_event.code,
                                    now.duration_since(start_time).as_millis()
                                );
                            }
                            KeyEventKind::Release => {
                                if let Some(press_time) = key_press_times.remove(&key_event.code) {
                                    let duration = now.duration_since(press_time);
                                    println!(
                                        "[{:6}] [Release] {:?} at {:>6}ms (duration: {:>4}ms)\r",
                                        event_count,
                                        key_event.code,
                                        now.duration_since(start_time).as_millis(),
                                        duration.as_millis()
                                    );

                                    // 分析按键持续时间
                                    if duration.as_millis() < 100 {
                                        println!("         -> 短按 (< 100ms)\r");
                                    } else if duration.as_millis() < 500 {
                                        println!("         -> 中等按 (100-500ms)\r");
                                    } else {
                                        println!("         -> 长按 (> 500ms)\r");
                                    }
                                } else {
                                    println!(
                                        "[{:6}] [Release] {:?} at {:>6}ms (没有对应的 Press 事件)\r",
                                        event_count,
                                        key_event.code,
                                        now.duration_since(start_time).as_millis()
                                    );
                                }
                            }
                            KeyEventKind::Repeat => {
                                println!(
                                    "[{:6}] [Repeat]  {:?} at {:>6}ms\r",
                                    event_count,
                                    key_event.code,
                                    now.duration_since(start_time).as_millis()
                                );
                            }
                        }
                    }
                    Some(Ok(Event::Resize(width, height))) => {
                        println!(
                            "[{:6}] [Resize]  {}x{}\r",
                            event_count, width, height
                        );
                    }
                    Some(Ok(Event::Mouse(mouse))) => {
                        println!(
                            "[{:6}] [Mouse]   kind={:?}\r",
                            event_count, mouse.kind
                        );
                    }
                    Some(Ok(Event::Paste(text))) => {
                        println!(
                            "[{:6}] [Paste]   {} chars\r",
                            event_count,
                            text.len()
                        );
                    }
                    Some(Ok(Event::FocusGained)) => {
                        println!("[{:6}] [Focus]   Gained\r", event_count);
                    }
                    Some(Ok(Event::FocusLost)) => {
                        println!("[{:6}] [Focus]   Lost\r", event_count);
                    }
                    Some(Err(e)) => {
                        println!("[{:6}] [Error]   {:?}\r", event_count, e);
                    }
                    None => {
                        println!("[{:6}] [End]     事件流结束\r", event_count);
                        disable_raw_mode()?;
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn print_statistics(total_events: u64, start_time: Instant) {
    let duration = start_time.elapsed();
    let seconds = duration.as_secs_f64();

    println!();
    println!("=== 统计信息 ===\r");
    println!("总事件数: {}\r", total_events);
    println!("测试时长: {:.2} 秒\r", seconds);
    if seconds > 0.0 {
        println!("平均事件率: {:.2} 事件/秒\r", total_events as f64 / seconds);
    }
    println!();
    println!("=== 分析建议 ===\r");
    println!("1. 观察按键重复间隔：\r");
    println!("   - 如果间隔 < 50ms，系统按键重复率高\r");
    println!("   - 如果间隔 > 100ms，系统按键重复率低\r");
    println!();
    println!("2. 观察按键持续时间：\r");
    println!("   - 短按 (< 100ms)：适合单次导航\r");
    println!("   - 长按 (> 500ms)：需要实现连续滚动\r");
    println!();
    println!("3. 观察事件类型：\r");
    println!("   - 如果有 Repeat 事件：可以直接利用\r");
    println!("   - 如果只有 Press/Release：需要自己实现连续滚动\r");
}

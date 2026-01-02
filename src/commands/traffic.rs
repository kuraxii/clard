use colored::Colorize;
use ipc::websocket::*;
use serde::{Deserialize, Serialize};
use crate::error::Result;

use crate::ipc::{self};

/// `traffic` subcommand
#[derive(clap::Parser, Debug)]
pub struct TrafficCmd;


impl TrafficCmd {
    async fn get(&self) -> Result<()> {
        // let url = Url::parse(&get_websocket_url("traffic")).unwrap();
        // APP.backend.connect_to_websocket_with(url, Self::print).await.unwrap();
        // tokio::signal::ctrl_c().await.expect("无法监听 Ctrl+C 信号");
        Ok(())
    }

    pub fn print(traffic: WebSocketMessage<Traffic>) {
        let traffic = match traffic {
            WebSocketMessage::Text(traffic) => traffic,
            WebSocketMessage::Close(_) => {
                println!("websocket close");
                return;
            }
            _ => {
                return;
            }
        };

        println!(
            "{}:{}{}, {}:{}{}",
            "down".green(),
            traffic.down,
            "kbps".blue(),
            "up".green(),
            traffic.up,
            "kbps".blue()
        );
    }
}

fn send(chunk: &str) {
    todo!()
}


#[derive(Debug, Serialize, Deserialize)]
pub struct Traffic {
    pub up: u64,
    pub down: u64,
}

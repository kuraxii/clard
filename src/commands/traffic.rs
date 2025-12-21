//! `start` subcommand - example of how to write a subcommand

use std::path::PathBuf;

use abscissa_core::{Command, FrameworkError, Runnable, config, error::message, trace::Tracing};
use colored::Colorize;
use ipc::websocket::*;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

use crate::{
    application::APP,
    config::ClardRsConfig,
    ipc::{self, error::IpcError},
    prelude::*,
};

/// `traffic` subcommand
#[derive(clap::Parser, Command, Debug)]
pub struct TrafficCmd;


impl TrafficCmd {
    async fn get(&self) -> Result<()> {
        let url = Url::parse(&get_websocket_url("traffic")).unwrap();
        APP.backend.connect_to_websocket_with(url, Self::print).await.unwrap();
        tokio::signal::ctrl_c().await.expect("无法监听 Ctrl+C 信号");
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

impl Runnable for TrafficCmd {
    /// Start the application.
    fn run(&self) {
        let rt = Runtime::new().unwrap();
        rt.block_on(self.get()).unwrap();
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Traffic {
    pub up: u64,
    pub down: u64,
}

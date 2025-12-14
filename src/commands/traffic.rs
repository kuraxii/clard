//! `start` subcommand - example of how to write a subcommand

/// App-local prelude includes `app_reader()`/`app_writer()`/`app_config()`
/// accessors along with logging macros. Customize as you see fit.
use crate::{commands::metaversion::BackendVersionCmd, prelude::*};

use crate::config::ClardRsConfig;
use abscissa_core::{Command, FrameworkError, Runnable, config, trace::Tracing};

use colored::Colorize;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json;

use tokio::runtime::Runtime;
/// `start` subcommand
///
/// The `Parser` proc macro generates an option parser based on the struct
/// definition, and is defined in the `clap` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/clap/>
#[derive(clap::Parser, Command, Debug)]
pub struct TrafficCmd;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Traffic {
    up: u32,
    down: u32,
}

impl TrafficCmd {
    async fn get<F>(&self, unix_sock: &str, action: F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: Fn(&str),
    {
        let client = Client::builder().unix_socket(unix_sock).build()?;
        let mut response = client
            .get("http://localhost/traffic")
            .send()
            .await?
            .error_for_status()?;

        while let Some(chunk) = response.chunk().await? {
            let chunk_text = String::from_utf8_lossy(&chunk);
            action(&chunk_text);
        }
        Ok(())
    }

    pub fn print(chunk: &str) {
        let traffic: Traffic = serde_json::from_str(chunk).unwrap();
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

    fn send(chunk: &str) {
        todo!()
    }
}

impl Runnable for TrafficCmd {
    /// Start the application.
    fn run(&self) {
        let unix_sock = "/tmp/verge/verge-mihomo.sock";

        let rt = Runtime::new().unwrap();
        rt.block_on(self.get(unix_sock, |chunk| Self::print(chunk)))
            .unwrap();
    }
}

//! `start` subcommand - example of how to write a subcommand

/// App-local prelude includes `app_reader()`/`app_writer()`/`app_config()`
/// accessors along with logging macros. Customize as you see fit.
use crate::prelude::*;

use crate::config::ClardRsConfig;
use abscissa_core::{Command, FrameworkError, Runnable, config};

use reqwest::Client;
use tokio::runtime::Runtime;

use crate::utils::protocol::Config;

/// `start` subcommand
///
/// The `Parser` proc macro generates an option parser based on the struct
/// definition, and is defined in the `clap` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/clap/>
#[derive(clap::Parser, Command, Debug)]
pub struct GroupsCmd;

impl GroupsCmd {
    fn get(&self, unix_sock: &str) -> Result<(), Box<dyn std::error::Error>> {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let client = Client::builder().unix_socket(unix_sock).build()?;

            let response = client
                .get("http://localhost/group")
                .send()
                .await?
                .error_for_status()?;
            let body = response.text().await?;
            println!("body: \n{}", body);
            let json_config: Config = serde_json::from_str(&body).map_err(|err| {
                eprintln!("failed to parse response: {err}");
                eprintln!("raw response:\n{body}");
                err
            })?;

            // let pretty_json = serde_json::to_string_pretty(&json_config)?;

            // println!("version:\n{:?}", pretty_json);
            Ok(())
        })
    }
}

impl Runnable for GroupsCmd {
    /// Start the application.
    fn run(&self) {
        let unix_sock = "/tmp/verge/verge-mihomo.sock";
        self.get(unix_sock).unwrap();
    }
}



//! `start` subcommand - example of how to write a subcommand

use abscissa_core::{Command, FrameworkError, Runnable, config};
use reqwest::Client;
use tokio::runtime::Runtime;

use crate::config::ClardRsConfig;
/// App-local prelude includes `app_reader()`/`app_writer()`/`app_config()`
/// accessors along with logging macros. Customize as you see fit.
use crate::prelude::*;
/// `start` subcommand
///
/// The `Parser` proc macro generates an option parser based on the struct
/// definition, and is defined in the `clap` crate. See their documentation
/// for a more comprehensive example:
///
/// <https://docs.rs/clap/>
#[derive(clap::Parser, Command, Debug)]
pub struct ProxiesCmd;


impl ProxiesCmd {
    fn get(&self, unix_sock: &str) -> Result<()> {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let client = Client::builder().unix_socket(unix_sock).build()?;

            let response = client.get("http://localhost/proxies").send().await?;
            println!("{}", response.error_for_status()?.text().await?);
            Ok(())
        })
    }
}

impl Runnable for ProxiesCmd {
    /// Start the application.
    fn run(&self) {
        let unix_sock = "/tmp/verge/verge-mihomo.sock";
        self.get(unix_sock).unwrap();
    }
}

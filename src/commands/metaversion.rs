//! `start` subcommand - example of how to write a subcommand

use abscissa_core::{Command, FrameworkError, Runnable, config};

use serde::{Deserialize, Serialize};

use crate::{config::ClardRsConfig, ipc::http, prelude::*};
use crate::application::APP;

use reqwest::{Method, Url};
/// `BackendVersion` subcommand
///
/// The `Parser` proc macro generates an option parser based on the struct
/// definition, and is defined in the `clap` crate. See their documentation
/// for a more comprehensive example:
#[derive(clap::Parser, Command, Debug)]
pub struct BackendVersionCmd;

impl BackendVersionCmd {
    fn get(&self, unix_sock: &str) -> Result<()> {

        let url = http::get_http_url("version");
        let bv = http::blocking::get_uds::<BackendVersion>(unix_sock, &url);
        println!("version: {:?}", bv);

        Ok(())
    }
}

impl Runnable for BackendVersionCmd {
    /// Start the application.
    fn run(&self) {
        let unix_sock = "/tmp/verge/verge-mihomo.sock";
        self.get(unix_sock).unwrap();
    }
}

///
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendVersion {
    pub meta: bool,
    pub version: String,
}

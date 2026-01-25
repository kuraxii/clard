//! Main entry point for ClardRs

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]

use clap::Parser;
use clard::{
    commands::{ClardRsCmd, Cli},
    error::Result,
    start_clard,
};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // --tui 优先
    if !cli.tui && cli.cmd.is_some() {
        match cli.cmd.unwrap() {
            ClardRsCmd::Test(name) => println!("Test! {:?}", name),
        }
    } else {
        start_clard().await?
    }

    Ok(())
}

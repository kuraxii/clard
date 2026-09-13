//! Main entry point for ClardRs

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]

use clap::Parser;
use clard_tui::{
    commands::{ClardRsCmd, Cli},
    error::Result,
    start_clard,
};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // --tui 优先
    if !cli.tui {
        match cli.cmd {
            Some(cmd) => match cmd {
                ClardRsCmd::Test(name) => println!("Test! {:?}", name),
            },
            None => start_clard().await?,
        };
    } else {
        start_clard().await?
    }

    Ok(())
}

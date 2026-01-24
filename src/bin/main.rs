//! Main entry point for ClardRs

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]

use clap::Parser;
use clard::{commands::{ClardRsCmd, Cli}, start_clard};
use clard::error::Result;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()>{
    let cli = Cli::parse();

    if cli.tui {
        let _ = start_clard().await?;
    }

    match cli.cmd {
        Some(ClardRsCmd::Test(name)) => println!("test! {:?}", name),
        None =>println!("none"),
    };

    Ok(())
}



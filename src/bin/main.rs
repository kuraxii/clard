//! Main entry point for ClardRs

#![deny(warnings, missing_docs, trivial_casts, unused_qualifications)]




use clap::Parser;
use clard::commands::{Cli, ClardRsCmd};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let cli = Cli::parse();

    if cli.tui {
        println!("do tui");
        return;
    }

    match cli.cmd {
        Some(ClardRsCmd::Test(name)) => println!("test! {:?}", name),
        None =>println!("none"),
    };
}


/*
 let cli = Cli::parse();
    let addr = format!("{}:{}", cli.host, cli.port);

    let result = match cli.command {
        Command::Get { key } => run_get(&addr, &key).await,
        Command::Set { key, value } => run_set(&addr, &key, value).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {:?}", e);
        std::process::exit(1);
    }

*/
//! Standalone classic ELIZA compatibility server.

#![feature(register_tool)]
#![register_tool(rlib)]

mod cli;

use clap::Parser;
use miette::Result;

use crate::cli::{Cli, Commands};

/// Run the standalone ELIZA CLI.
///
/// # Errors
///
/// Returns a diagnostic when CLI arguments fail runtime validation or the
/// server cannot bind or run.
///
/// # Panics
///
/// Panics if Tokio cannot initialize the async runtime for the process.
#[tokio::main]
async fn main() -> Result<()> {
    let Commands::Serve(args) = Cli::parse().command;
    args.into_server_config().run().await?;
    Ok(())
}

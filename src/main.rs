//! Standalone classic ELIZA compatibility server.

mod errors;
mod eliza;
mod routes;
mod serve;
mod cli;
mod types;

use clap::Parser;
use miette::Result;

use crate::cli::{Cli, Commands};
use crate::serve::ServerConfig;

/// Run the standalone ELIZA CLI.
///
/// # Errors
///
/// Returns a diagnostic when CLI arguments fail runtime validation or the
/// server cannot bind/run.
///
/// # Panics
///
/// Panics if Tokio cannot initialize the async runtime for the process.
#[tokio::main]
async fn main() -> Result<()> {
    let Commands::Serve(args) = Cli::parse().command;
    ServerConfig::try_from(args)?.run().await?;
    Ok(())
}

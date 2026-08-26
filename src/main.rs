//! Standalone classic ELIZA compatibility server.

mod errors;
mod eliza;
mod provider;
mod serve;
mod cli;

#[cfg(test)]
mod test_support;

use clap::Parser;
use miette::Result;

use crate::cli::{Cli, Commands};
use crate::serve::{ServerConfig, run_server};

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
    run_server(ServerConfig::try_from(args)?).await?;
    Ok(())
}

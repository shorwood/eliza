//! Standalone classic ELIZA compatibility server.
#![feature(register_tool)]
#![register_tool(rlib)]
#![allow(
    rlib::long_method_chains,
    reason = "Bon typestate builders are clearest as uninterrupted fluent construction"
)]

mod errors;
mod eliza;
mod embedding;
mod problem;
mod routes;
mod serve;
mod speech;
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

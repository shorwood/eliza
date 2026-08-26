use clap::Parser;
use eliza::cli::{Cli, Commands};
use eliza::serve::{ServerConfig, serve};
use miette::Result;

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
    match Cli::parse().command {
        Commands::Serve(args) => serve(ServerConfig::try_from(args)?).await?,
    }
    Ok(())
}

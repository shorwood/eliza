//! Standalone classic ELIZA compatibility server.

#![feature(register_tool)]
#![register_tool(rlib)]

mod cli;

use clap::Parser;
use miette::{IntoDiagnostic, Result};

use crate::cli::{Cli, Commands};

/// Run the standalone ELIZA CLI.
///
/// # Errors
///
/// Returns a diagnostic when CLI arguments fail runtime validation or the
/// server cannot bind or run.
///
/// Runtime initialization failures are returned as diagnostics.
fn main() -> Result<()> {
    let Commands::Serve(args) = Cli::parse().command;
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    if let Some(workers) = args.runtime_workers {
        runtime.worker_threads(workers.get());
    }
    let runtime = runtime.enable_all().build().into_diagnostic()?;
    runtime.block_on(args.into_server_config()?.run())?;
    Ok(())
}

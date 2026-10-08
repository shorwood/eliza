//! Standalone classic ELIZA compatibility server.

#![feature(register_tool)]
#![register_tool(rlib)]

mod cli;

use clap::{CommandFactory as _, FromArgMatches as _};
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
///
/// # Panics
/// Panics only if Clap returns a command other than the parsed `serve` command.
fn main() -> Result<()> {
    let matches = Cli::command().get_matches();
    let Commands::Serve(args) = Cli::from_arg_matches(&matches)
        .unwrap_or_else(|error| error.exit())
        .command;
    let args = args.with_config(
        matches
            .subcommand_matches("serve")
            .expect("the parsed command is serve"),
    )?;
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    if let Some(workers) = args.runtime_workers {
        runtime.worker_threads(workers.get());
    }
    let runtime = runtime.enable_all().build().into_diagnostic()?;
    runtime.block_on(args.into_server_config()?.run())?;
    Ok(())
}

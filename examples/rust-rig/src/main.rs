//! Exercise ELIZA through Rig's provider clients.

#![feature(register_tool)]
#![register_tool(rlib)]
#![allow(
    rlib::foreign_type_method_like_free_functions,
    rlib::missing_code_phase_comments,
    rlib::missing_section_dividers,
    rlib::undocumented_items,
    rlib::unseparated_module_items,
    reason = "small private examples favor SDK-shaped code over library API ceremony"
)]

mod catalogs;
mod chat;
mod embeddings;
mod errors;
mod images;
mod providers;
mod reasoning;
mod report;
mod speech;
mod vision;

use anyhow::Result;

use crate::providers::Providers;

/// Run every Rig example against a local ELIZA server.
///
/// # Errors
///
/// Returns a client-construction error or the number of failed examples.
///
/// # Panics
///
/// Panics if Tokio cannot initialize its runtime.
#[tokio::main]
async fn main() -> Result<()> {
    let base_url =
        std::env::var("ELIZA_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8787".to_owned());
    let providers = Providers::connect(&base_url)?;

    let failures = catalogs::run(&providers).await
        + chat::run(&providers).await
        + reasoning::run(&providers).await
        + vision::run(&providers).await
        + embeddings::run(&providers).await
        + images::run(&providers).await
        + speech::run(&providers).await;

    report::ensure_success(failures)
}

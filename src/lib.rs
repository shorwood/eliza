//! Standalone classic ELIZA compatibility server.
//!
//! The crate keeps the historical script engine, provider wire adapters, and
//! binary CLI isolated from Nano crates. Provider modules stay private; tests
//! and the binary use the same public surfaces an embedding tool would need.
//!
//! Public entry points:
//!
//! - [`eliza::doctor_script`] and [`eliza::ElizaSession`] for direct engine use.
//! - [`serve::router`] and [`serve::ServerConfig`] for embedding the HTTP app.
//! - [`cli::Cli`] for the standalone binary.
//! - [`ModelId`] for CLI and provider-visible model identifiers.

pub mod cli;
pub mod eliza;
pub mod errors;
pub mod serve;

mod provider;

#[cfg(test)]
mod test_support;

pub use provider::ModelId;

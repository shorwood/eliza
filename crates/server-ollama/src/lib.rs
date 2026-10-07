//! Ollama-compatible routes.

#![feature(register_tool)]
#![register_tool(rlib)]

mod chat;
mod context;
mod embeddings;
pub mod errors;
mod models;
pub mod router;
mod types;

//! Ollama-compatible routes.

#![feature(register_tool)]
#![register_tool(rlib)]

mod embeddings;
mod errors;
mod routes;
pub mod router;
mod types;

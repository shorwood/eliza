//! Gemini-compatible provider adapter.

#![feature(register_tool)]
#![register_tool(rlib)]

mod content;
mod embeddings;
mod errors;
mod models;
pub mod router;
mod speech;
mod types;

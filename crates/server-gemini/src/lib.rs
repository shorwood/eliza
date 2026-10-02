//! Gemini-compatible provider adapter.

#![feature(register_tool)]
#![register_tool(rlib)]

mod context;
mod embeddings;
mod errors;
mod generate;
mod images;
mod models;
pub mod router;
mod speech;
mod types;

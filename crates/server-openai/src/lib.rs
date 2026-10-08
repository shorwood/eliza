//! OpenAI-compatible provider adapter.

#![feature(register_tool)]
#![register_tool(rlib)]

mod chat_completions;
mod compatibility;
mod context;
mod embeddings;
pub mod errors;
mod images;
mod models;
mod responses;
pub mod router;
mod speech;
mod types;

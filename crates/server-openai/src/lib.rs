//! OpenAI-compatible provider adapter.

#![feature(register_tool)]
#![register_tool(rlib)]

pub mod alias;
mod chat_completions;
mod embeddings;
mod errors;
mod models;
mod responses;
pub mod router;
mod speech;
mod types;

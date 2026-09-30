//! Anthropic-compatible provider adapter.

#![feature(register_tool)]
#![register_tool(rlib)]

mod errors;
mod messages;
mod models;
pub mod router;
mod types;

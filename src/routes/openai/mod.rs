//! `OpenAI`-compatible routes.

pub(crate) mod chat_completions;
pub(crate) mod errors;
mod models;
mod responses;
mod speech;
pub(super) mod router;
pub(crate) mod types;

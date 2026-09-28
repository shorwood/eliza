//! Provider-compatible HTTP routes and their wire contracts.

pub(crate) mod context;
mod anthropic_messages;
mod anthropic_models;
mod docs;
mod gemini_content;
mod gemini_models;
mod health;
mod openai_chat_completions;
mod openai_models;
mod openai_responses;
mod ollama;
pub(crate) mod router;

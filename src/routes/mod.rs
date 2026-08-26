//! Provider-compatible HTTP routes and their wire contracts.

pub(crate) mod context;
mod anthropic_messages;
mod docs;
mod gemini_content;
mod gemini_models;
mod health;
mod openai_chat_completions;
mod openai_models;
mod openai_responses;
pub(crate) mod router;
#[cfg(test)]
mod compatibility_tests;

//! Deterministic provider-neutral chat execution.

#![feature(register_tool)]
#![register_tool(rlib)]

mod eliza;
pub mod errors;
pub mod json;
pub mod structured_output;
pub mod turn;

//! Shared HTTP boundary for provider adapters.

#![feature(register_tool)]
#![register_tool(rlib)]

pub mod context;
pub mod errors;
pub mod extraction;
pub mod model;
pub mod problem;
pub mod response;

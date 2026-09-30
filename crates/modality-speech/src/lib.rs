//! Small, deterministic speech engine shared by provider adapters.

#![feature(register_tool)]
#![register_tool(rlib)]

mod audio;
pub mod core;
pub mod errors;
pub mod service;
mod synth;

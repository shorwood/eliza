//! Standalone MAD-SLIP ELIZA engine.

#![feature(register_tool)]
#![register_tool(rlib)]

pub mod engine;
mod programs;
mod runtime;
mod script;

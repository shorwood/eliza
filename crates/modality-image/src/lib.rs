//! Bounded deterministic PNG and JPEG analysis.

#![feature(register_tool)]
#![register_tool(rlib)]

pub mod analysis;
pub mod errors;
pub mod generation;
pub mod limits;
pub mod source;

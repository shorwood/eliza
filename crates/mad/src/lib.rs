//! A deliberately bounded interpreter for the MAD subset used by ELIZA.
//!
//! The parser preserves fixed-form card positions for all archival modules.
//! The linker accepts a small executable subset rather than pretending to be a
//! general-purpose MAD compiler.

#![feature(register_tool)]
#![register_tool(rlib)]

pub mod machine;
pub mod program;
pub mod source;
pub mod word;

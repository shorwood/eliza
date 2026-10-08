//! ELIZA HTTP server composition and process runtime.

#![feature(register_tool)]
#![register_tool(rlib)]

mod docs;
mod health;
mod limits;
mod routes;
pub mod serve;

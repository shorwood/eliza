//! ELIZA HTTP server composition and process runtime.

#![feature(register_tool)]
#![register_tool(rlib)]

mod admission;
mod connection;
mod docs;
mod health;
mod routes;
mod work;
pub mod serve;

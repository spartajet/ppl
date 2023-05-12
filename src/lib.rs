//! Parallelo Structured Parallel Processing is a simple parallel processing
//! library written in Rust.
//!
//#![warn(missing_docs)]
#![feature(unsized_fn_params)]
#![feature(box_into_inner)]
#![feature(once_cell)]
#![feature(let_chains)]

pub mod mpsc;
pub mod map;
pub mod core;
pub mod pipeline;
pub mod pspp;
mod task;
pub mod thread_pool;

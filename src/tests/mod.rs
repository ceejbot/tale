//! Integration tests etc in their own module.

use std::path::PathBuf;

mod adaptation;
mod integration;
mod simulation;

// pub use simulation::*;

pub fn create_large_test_file(_lines: usize) -> PathBuf {
    // TODO stub
    PathBuf::from("./fixtures/benchmarks/large.log")
    // todo!()
}

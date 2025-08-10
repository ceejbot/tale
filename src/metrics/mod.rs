//! Reading files by adaptive chunk sizes to adapt to memory pressure
//! and file size. We want to scale down if we're on a puny container
//! on some Raspberry Pi in the cloud, and scale up if we're on monstro
//! the memory-stuffed real-hardware luxury box. We also want to cope with
//! what we're being asked to juggle.

mod memory;
mod metrics;

pub use memory::*;
pub use metrics::*;

pub fn is_memory_constrained() -> bool {
    // Check if we're in a container, low memory system, etc.
    // The check can get more complex than this
    let avail_bytes = memory::available_memory_bytes();
    eprintln!("memory::available_memory_bytes() => {avail_bytes}");
    eprintln!("pressure is: {:#?}", detect_memory_pressure(None));

    memory::available_memory_bytes() < 500
}

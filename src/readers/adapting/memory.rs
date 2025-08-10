//! Pressure pushing down on me pressing down on you
//! no man ask for under pressure
//! that burns a building down
//! splits a family in two
//! puts people on streets

pub use memimpl::*;

use crate::config::config;
use crate::constants::MEMORY_LIMIT_MB;
use crate::readers::ChunkStrategy;

#[derive(Debug, Clone, Copy)]
pub enum MemoryPressure {
    None,     // < 50% usage
    Low,      // 50-70%
    Medium,   // 70-85%
    High,     // 85-95%
    Critical, // > 95%
}

pub fn detect_memory_pressure(limit_mb: Option<usize>) -> MemoryPressure {
    let our_usage_mb = process_memory_mb();
    let limit = limit_mb.unwrap_or(MEMORY_LIMIT_MB);

    let percent = (our_usage_mb as f64 / limit as f64) * 100.0;

    match percent {
        p if p < 50.0 => MemoryPressure::None,
        p if p < 70.0 => MemoryPressure::Low,
        p if p < 85.0 => MemoryPressure::Medium,
        p if p < 95.0 => MemoryPressure::High,
        _ => MemoryPressure::Critical,
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryConfig {
    // User-specified limit
    pub max_total_mb: Option<usize>,
    // Count of files being processed
    pub num_files: usize,
}

// TODO placeholder!
fn get_system_ram_mb() -> Option<usize> {
    todo!()
}

impl MemoryConfig {
    const MEMORY_CEILING_MB: usize = 200;
    const PER_FILE_MB: usize = 10;

    /// Responds with a limit in megabytes.
    pub fn calculate_limit_mb(&self) -> usize {
        if let Some(user_limit) = self.max_total_mb {
            return user_limit;
        }

        // If we have the space, we give each file a fairly
        // roomy 10MB. This is probably more than we need, and we
        // can tighten after measuring.
        let total_for_files = MemoryConfig::PER_FILE_MB * self.num_files;

        // System limit: 10% of RAM or 200MB, whichever is smaller
        let system_limit = if let Some(total_ram_mb) = get_system_ram_mb() {
            std::cmp::min(total_ram_mb / 10, MemoryConfig::MEMORY_CEILING_MB)
        } else {
            MemoryConfig::MEMORY_CEILING_MB // Conservative fallback
        };

        std::cmp::min(total_for_files, system_limit)
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStrategy {
    // TODO
}

impl ChunkStrategy for MemoryStrategy {
    fn initial_chunk_size(&self) -> usize {
        todo!()
    }

    /// Given the current chunk size and current metrics, make a decision about
    /// what chunk size we should use next.
    fn adapt_size(&mut self, _metrics: &super::ChunkMetrics, current_size: usize) -> usize {
        let max = config().max_memory;
        match detect_memory_pressure(Some(max)) {
            MemoryPressure::None => current_size,
            MemoryPressure::Low => todo!(),
            MemoryPressure::Medium => todo!(),
            MemoryPressure::High => todo!(),
            MemoryPressure::Critical => todo!(),
        }
    }

    /// Should we do the work of making an adaptation decision right now or just
    /// keep sailing?
    fn should_adapt(&self, _metrics: &super::ChunkMetrics) -> bool {
        // metrics.should_adapt(interval, chunks_processed)
        todo!()
    }
}

#[cfg(not(feature = "sysinfo"))]
pub mod memimpl {

    use memory_stats::*;

    use super::*;

    impl From<f64> for MemoryPressure {
        fn from(p: f64) -> Self {
            match p {
                p if p < 50.0 => MemoryPressure::None,
                p if p < 70.0 => MemoryPressure::Low,
                p if p < 85.0 => MemoryPressure::Medium,
                p if p < 95.0 => MemoryPressure::High,
                _ => MemoryPressure::Critical,
            }
        }
    }

    /// Are we under memory pressure?
    pub fn detect_memory_pressure(max_allowed_mb: Option<usize>) -> MemoryPressure {
        let Some(stats) = memory_stats() else {
            return MemoryPressure::None;
        };
        let max_allowed = max_allowed_mb.unwrap_or(MEMORY_LIMIT_MB) * 1024;
        if stats.physical_mem > max_allowed {
            MemoryPressure::Critical
        } else {
            let percent = (stats.physical_mem * 100) as f64 / max_allowed as f64;
            MemoryPressure::from(percent)
        }
    }

    /// Available memory in megabytes.
    pub fn available_memory_mb() -> usize {
        todo!();
    }

    /// Process RSS in megabytes
    pub fn process_memory_mb() -> usize {
        let Some(stats) = memory_stats() else {
            return 0;
        };
        stats.physical_mem / 1024
    }
}

#[cfg(feature = "sysinfo")]
pub mod memimpl {
    use std::sync::{LazyLock, Mutex, OnceLock};

    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

    use super::*;

    static SYSTEM: LazyLock<Mutex<System>> = LazyLock::new(|| {
        let processes = ProcessRefreshKind::nothing().with_cpu().with_memory();
        let memory = MemoryRefreshKind::nothing().with_ram();
        let cpu = CpuRefreshKind::nothing().with_cpu_usage();
        let refreshes = RefreshKind::nothing()
            .with_cpu(cpu)
            .with_memory(memory)
            .with_processes(processes);
        Mutex::new(sysinfo::System::new_with_specifics(refreshes))
    });
    pub fn system() -> std::sync::MutexGuard<'static, System> {
        SYSTEM
            .lock()
            .expect("Unrecoverable runtime problem: cannot acquire system lock. Exiting.")
    }

    static PID: OnceLock<Pid> = OnceLock::new();
    pub fn pid() -> Option<Pid> {
        if let Some(pid) = PID.get() {
            Some(pid.clone())
        } else {
            match sysinfo::get_current_pid() {
                Ok(p) => {
                    PID.set(p.clone());
                    Some(p)
                }
                Err(_) => None,
            }
        }
    }

    /// Are we under memory pressure?
    pub fn detect_memory_pressure(max_allowed_mb: Option<usize>) -> MemoryPressure {
        let Some(stats) = memory_stats() else {
            return MemoryPressure::None;
        };
        if stats.physical_mem > max_allowed_mb * 1024 {
            MemoryPressure::Critical
        } else {
            let percent = (stats.physical_mem * 100) as f64 / (max_allowed_mb * 1024) as f64;
            MemoryPressure::from(percent)
        }
        // Return appropriate level
        let Some(pid) = pid() else {
            return MemoryPressure::None;
        };
        let proc_list = vec![pid];
        let to_update = ProcessesToUpdate::Some(proc_list.as_slice());
        let mut system = system();
        system.refresh_processes(to_update, true);
        let Some(process) = system.process(pid) else {
            return MemoryPressure::None;
        };
        let rss = process.memory();
        let max_allowed = config().max_memory;
        let avail = system.available_memory();
        let total = system.total_memory();

        todo!();
    }

    pub fn available_memory_mb() -> usize {
        todo!();
    }

    pub fn process_memory_mb() -> usize {
        let Some(pid) = pid() else {
            return 0;
        };
        let system = system();
        let Some(process) = system.process(pid) else {
            return 0;
        };
        process.memory()
    }
}

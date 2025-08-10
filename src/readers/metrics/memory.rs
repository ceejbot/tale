//! Pressure pushing down on me pressing down on you
//! no man ask for under pressure
//! that burns a building down
//! splits a family in two
//! puts people on streets

pub use memimpl::*;

use crate::constants::MEMORY_LIMIT_MB;

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

    // TODO placeholder!
    pub fn get_system_ram_mb() -> Option<usize> {
        Some(8 * 1024 * 1024 * 1024)
    }

    /// Available memory in megabytes.
    pub fn available_memory_mb() -> usize {
        // TODO look at system memory
        let our_usage_mb = process_memory_mb();
        MEMORY_LIMIT_MB.saturating_sub(our_usage_mb)
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
        // Return appropriate level
        let Some(pid) = pid() else {
            return MemoryPressure::Unknown;
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
        let system = system();
        // TODO
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

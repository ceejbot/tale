//! Pressure pushing down on me pressing down on you
//! no man ask for under pressure
//! that burns a building down
//! splits a family in two
//! puts people on streets

pub use memimpl::*;

use crate::constants::MEMORY_LIMIT_BYTES;

#[derive(Debug, Clone, Copy)]
pub enum MemoryPressure {
    Unknown,
    None,     // < 50% usage
    Low,      // 50-70%
    Medium,   // 70-85%
    High,     // 85-95%
    Critical, // > 95%
}

pub fn detect_memory_pressure(limit_bytes: Option<usize>) -> MemoryPressure {
    memimpl::detect_memory_pressure(limit_bytes)
}

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

pub mod memimpl {
    use std::sync::{LazyLock, Mutex, OnceLock};

    use sysinfo::{CpuRefreshKind, MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

    use super::*;
    use crate::config;

    static SYSTEM: LazyLock<Mutex<System>> = LazyLock::new(|| {
        let processes = ProcessRefreshKind::nothing().with_cpu().with_memory();
        let memory = MemoryRefreshKind::nothing().with_ram();
        let cpu = CpuRefreshKind::nothing(); // .with_cpu_usage();
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
                    PID.set(p.clone()).expect("unexpected failure to store PID in oncelock");
                    Some(p)
                }
                Err(_) => None,
            }
        }
    }

    /// Are we under memory pressure?
    pub fn detect_memory_pressure(max_allowed_bytes: Option<usize>) -> MemoryPressure {
        // Return appropriate level
        let Some(pid) = pid() else {
            return MemoryPressure::Unknown;
        };
        let proc_list = vec![pid];
        let to_update = ProcessesToUpdate::Some(proc_list.as_slice());
        let mut system = system();
        system.refresh_processes(to_update, true);
        let Some(process) = system.process(pid) else {
            return MemoryPressure::Unknown;
        };
        let rss = process.memory() as usize;
        let max_allowed = max_allowed_bytes.unwrap_or_else(|| config().max_memory.unwrap_or(MEMORY_LIMIT_BYTES));
        let free = system.free_memory() as usize;
        // let total = system.total_memory();
        // eprintln!("memory: rss={rss}; max_allowed={max_allowed}; free={free};");

        if rss > max_allowed {
            MemoryPressure::Critical
        } else if rss > free {
            MemoryPressure::Critical
        } else {
            let percent = (rss * 100) as f64 / max_allowed as f64;
            MemoryPressure::from(percent)
        }
    }

    pub fn get_system_ram_bytes() -> usize {
        let system = system();
        system.total_memory() as usize
    }

    pub fn available_memory_bytes() -> usize {
        let Some(pid) = pid() else {
            return 0;
        };
        let proc_list = vec![pid];
        let to_update = ProcessesToUpdate::Some(proc_list.as_slice());
        let mut system = system();
        system.refresh_processes(to_update, true);
        let free = system.free_memory() as usize;
        let total = system.total_memory() as usize;

        let Some(process) = system.process(pid) else {
            return free as usize;
        };
        let rss = process.memory() as usize;
        let max_allowed = config().max_memory.unwrap_or(MEMORY_LIMIT_BYTES);

        eprintln!("system: total={total} free={free}; process: rss={rss}; max: {max_allowed}");

        let remaining_budget = max_allowed.saturating_sub(rss);
        let available = std::cmp::min(free, remaining_budget);
        available
    }

    pub fn process_memory_bytes() -> usize {
        let Some(pid) = pid() else {
            return 0;
        };
        let system = system();
        let Some(process) = system.process(pid) else {
            return 0;
        };
        process.memory() as usize
    }
}

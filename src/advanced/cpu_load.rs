//! Background CPU and memory load threads to simulate contention on shared core.
//!
//! Each worker: pins to shared_core → executes busy-loop with memory pressure (1MB buffer).
//! Measures real-time deadline degradation under increasing CPU load (0, 2, 4, 8, 12, 16, 18, 20 threads).


use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    hint::black_box,
};
use log::{info, warn};
use core_affinity::{get_core_ids, set_for_current};

/// Spawn background best-effort CPU and Memory load threads
/// 
/// Spawns background CPU/memory load threads pinned to shared core
///
/// Each worker:
/// 1. **Core pinning:** Pins to shared_core_index (same core as RT threads → contention)
/// 2. **Memory pressure:** 1MB buffer for cache/bus thrashing (exceeds L1/L2 cache)
/// 3. **Busy-loop:** Tight loop with writes to buffer until running flag is false.
/// 4. **Occasional yield:** Every 1M iterations to maintain "background" priority
///
/// # Arguments
/// * threads — Number of load threads to spawn (0=baseline, >0=stress)
/// * running — Atomic flag; workers exit when false (graceful shutdown).
/// * shared_core_index — CPU core to pin (contention point with RT threads)
///
/// Returns
/// Vector of joinable thread handles; caller responsible for `join()
pub fn spawn_cpu_load(
    threads: usize,
    running: Arc<AtomicBool>,
    shared_core_index: usize,
) -> Vec<thread::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(threads);
    let cores = get_core_ids().unwrap_or_default();

    for id in 0..threads {
        let flag = running.clone();

        // Pick the core; if index is out of bounds, default to the first available
        let core_id = cores.get(shared_core_index)
            .cloned()
            .or_else(|| cores.first().cloned());

        let handle = thread::Builder::new()
            .name(format!("load_worker_{}", id))
            .spawn(move || {
                // 1. Set Affinity (Cross-platform)
                if let Some(core) = core_id {
                    if set_for_current(core) {
                        info!("Worker {}: Pinned to core {:?}", id, core);
                    } else {
                        warn!("Worker {}: Failed to set affinity", id);
                    }
                }

                // 2. Memory Pressure Simulation (Cache Thrashing)
                // Using a 1MB buffer to exceed typical L1/L2 cache lines
                let mut pressure_buf = vec![0u8; 1_000_000];
                let mut iter: usize = 0;

                // 3. Busy loop with interference
                while flag.load(Ordering::Relaxed) {
                    // CPU Pressure: tight loop logic
                    let val = black_box(id.wrapping_mul(31).wrapping_add(iter));
                    
                    // Memory Pressure: write to buffer to force cache/bus contention
                    let idx = iter % pressure_buf.len();
                    pressure_buf[idx] = val as u8;

                    iter = iter.wrapping_add(1);

                    // Optional: Yield occasionally to simulate "background" priority
                    if iter % 1_000_000 == 0 {
                        thread::yield_now(); 
                    }
                }

                info!("Worker {} exiting", id);
            })
            .expect("Failed to spawn load thread");

        handles.push(handle);
    }

    handles
}

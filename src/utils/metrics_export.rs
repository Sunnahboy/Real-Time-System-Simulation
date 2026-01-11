
//! CSV export for sweep results: aggregated metrics per CPU load level.
//!
//! Writes single row per experiment: cpu_load_threads, deadline_miss_count, total_cycles,
//! miss_rate (%), max_jitter_us, avg_latency_us. Appends to persistent file for multi-run sweeps.

use crate::utils::metrics::SharedMetrics;
use std::{
    fs::{OpenOptions,create_dir_all},
    io::Write,
    path::{Path},
};

/// Exports aggregated metrics for one experiment run to CSV.
///
/// Appends row to `data/cpu_load_results.csv`. Creates file with header on first write.
/// Computes: deadline miss rate (%), max jitter, average latency from shared metrics.
///
/// # Arguments
/// * `metrics` — Shared metrics buffer (locked to read final state).
/// * `cpu_load_threads` — Number of background threads for this experiment (row identifier).
pub fn export_summary_csv(metrics: &SharedMetrics, cpu_load_threads: usize) {
    let _ = create_dir_all("data");

    let csv_path = "data/logs/cpu_load_results.csv";
    let file_exists = Path::new(csv_path).exists();

    // Lock metrics; recover from poisoned state (shouldn't happen but safe)
    let m = match metrics.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    // Calculate statistics from bounded metrics buffers
    let max_jitter = m.jitter_us.iter().copied().max().unwrap_or(0);
    let avg_latency = if m.latency_us.is_empty() {
        0
    } else {
        m.latency_us.iter().sum::<u64>() / m.latency_us.len() as u64
    };

    let deadline_miss = m.deadline_miss;
    let total_cycles = m.total_cycles;

    let header = "cpu_load_threads,deadline_miss,total_cycles,deadline_miss_rate,max_jitter_us,avg_latency_us\n";

    // Compute miss rate as percentage; 0 if no cycles recorded
    let miss_rate = if total_cycles > 0 {
        (deadline_miss as f64 / total_cycles as f64) * 100.0
    } else {
        0.0
    };

    let row = format!(
        "{},{},{},{:.2},{},{}\n",
        cpu_load_threads,
        deadline_miss,
        total_cycles,
        miss_rate,
        max_jitter,
        avg_latency
    );

    // Append to CSV; write header if new file
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(csv_path)
    {
        Ok(mut file) => {
            if !file_exists {
                if let Err(e) = file.write_all(header.as_bytes()) {
                    eprintln!("[ERROR] Failed to write CSV header: {}", e);
                    return;
                }
            }

            match file.write_all(row.as_bytes()) {
                Ok(_) => println!("Summary exported to: {}", csv_path),
                Err(e) => eprintln!("[ERROR] Failed to write CSV row: {}", e),
            }
        }
        Err(e) => eprintln!("[ERROR] Failed to open CSV file: {}", e),
    }
}


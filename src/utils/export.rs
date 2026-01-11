
//! Comprehensive metrics export: per-experiment summaries and consolidated sweep results.
//!
//! Three outputs per experiment:
//! - `metrics_summary_load_X.csv` — Aggregated stats (min/max/avg) for sensors, actuators, latency.
//! - `sensors_all.csv` — Appended rows: all sensor samples across sweep levels (for trending).
//! - `actuators_all.csv` — Appended rows: all actuator commands across sweep levels.
//! - `feedback_events.csv` — Feedback loop events (state, errors, acks) with microsecond timestamps.

use crate::utils::{
    metrics::{SharedMetrics, calculate_stats, calculate_stats_u64},
    metrics_export::export_summary_csv,
};
use crate::component_b::{
    feedback::{FeedbackKind,Feedback},
};
use std::{
    path::{Path,PathBuf},
    io::Write,
    thread,
    fs::{create_dir_all, OpenOptions,write},
    collections::VecDeque,
};

use log::{info, error};

/// Exports detailed metrics for one experiment run: summary + consolidated rows.
///
/// Creates per-experiment summary (stats), appends sensor/actuator rows to sweep-wide CSVs.
/// Consolidation enables cross-load trending analysis without re-parsing event logs.
pub fn export_metrics_to_csv(metrics: SharedMetrics, cpu_load_threads: usize) {
    let export_dir = Path::new("data/dash_live_results");
    if let Err(e) = create_dir_all(export_dir) {
        error!("Failed to create export directory: {}", e);
        return;
    }

    let m = match metrics.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    // Build summary: deadline misses, latency/jitter stats, sensor/actuator stats
    let mut csv_content = String::from("metric,value,description\n");
    
    csv_content.push_str(&format!("deadline_misses,{},Total deadline miss events\n", m.deadline_miss));
    
    if let Some(jitter_stats) = calculate_stats_u64(&m.jitter_us) {
        csv_content.push_str(&format!("jitter_min_us,{:.2},Minimum jitter\n", jitter_stats.min));
        csv_content.push_str(&format!("jitter_max_us,{:.2},Maximum jitter\n", jitter_stats.max));
        csv_content.push_str(&format!("jitter_avg_us,{:.2},Average jitter\n", jitter_stats.mean));
        csv_content.push_str(&format!("jitter_samples,{},Jitter measurements\n", jitter_stats.count));
    }

    if let Some(latency_stats) = calculate_stats_u64(&m.latency_us) {
        csv_content.push_str(&format!("latency_min_us,{:.2},Minimum end-to-end latency\n", latency_stats.min));
        csv_content.push_str(&format!("latency_max_us,{:.2},Maximum end-to-end latency\n", latency_stats.max));
        csv_content.push_str(&format!("latency_avg_us,{:.2},Average end-to-end latency\n", latency_stats.mean));
        csv_content.push_str(&format!("latency_samples,{},Latency measurements\n", latency_stats.count));
    }

    if let Some(force_stats) = calculate_stats(&m.force) {
        csv_content.push_str(&format!("force_min,{:.2},Minimum force reading\n", force_stats.min));
        csv_content.push_str(&format!("force_max,{:.2},Maximum force reading\n", force_stats.max));
        csv_content.push_str(&format!("force_avg,{:.2},Average force reading\n", force_stats.mean));
    }

    if let Some(pos_stats) = calculate_stats(&m.position) {
        csv_content.push_str(&format!("position_min,{:.2},Minimum position reading\n", pos_stats.min));
        csv_content.push_str(&format!("position_max,{:.2},Maximum position reading\n", pos_stats.max));
        csv_content.push_str(&format!("position_avg,{:.2},Average position reading\n", pos_stats.mean));
    }

    if let Some(temp_stats) = calculate_stats(&m.temperature) {
        csv_content.push_str(&format!("temp_min,{:.2},Minimum temperature reading\n", temp_stats.min));
        csv_content.push_str(&format!("temp_max,{:.2},Maximum temperature reading\n", temp_stats.max));
        csv_content.push_str(&format!("temp_avg,{:.2},Average temperature reading\n", temp_stats.mean));
    }

    // Add sample counts
    csv_content.push_str(&format!("force_readings,{},Total force samples\n", m.force.len()));
    csv_content.push_str(&format!("position_readings,{},Total position samples\n", m.position.len()));
    csv_content.push_str(&format!("temperature_readings,{},Total temperature samples\n", m.temperature.len()));

    csv_content.push_str(&format!("gripper_commands,{},Total gripper actuations\n", m.gripper.len()));
    csv_content.push_str(&format!("motor_commands,{},Total motor actuations\n", m.motor.len()));
    csv_content.push_str(&format!("stabiliser_commands,{},Total stabiliser adjustments\n", m.stabiliser.len()));

    let summary_path = export_dir.join(format!("metrics_summary_load_{}.csv", cpu_load_threads));
    match write(&summary_path, csv_content) {
        Ok(_) => info!("Summary metrics exported to: {:?}", summary_path),
        Err(e) => error!("Failed to export metrics: {}", e),
    }

    // Append sensor data to sweep-wide CSV (enables trending across load levels)
    append_to_consolidated_csv(
        export_dir.join("sensors_all.csv"),
        cpu_load_threads,
        &m.force,
        &m.position,
        &m.temperature,
    );

    // Append actuator data to sweep-wide CSV
    append_to_consolidated_csv_actuators(
        export_dir.join("actuators_all.csv"),
        cpu_load_threads,
        &m.gripper,
        &m.motor,
        &m.stabiliser,
    );

    info!("Consolidated metrics exported to data/export/");
}

/// Appends sensor readings to sweep-wide CSV: load_level,sample_index,force,position,temperature.
/// Creates header on first write; enables trending across multiple CPU load experiments.
fn append_to_consolidated_csv(
    path: PathBuf,
    load_level: usize,
    force: &VecDeque<f64>,
    position: &VecDeque<f64>,
    temperature: &VecDeque<f64>,
) {
    let file_exists = path.exists();
    
    let mut file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to open sensor CSV: {}", e);
            return;
        }
    };

    // Write header on first experiment
    if !file_exists {
        if let Err(e) = writeln!(file, "load_level,sample_index,force,position,temperature") {
            error!("Failed to write sensor CSV header: {}", e);
            return;
        }
    }

    let max_len = force.len().max(position.len()).max(temperature.len());

    // Append rows for all samples; missing data filled with 0.0
    for i in 0..max_len {
        let f = force.get(i).copied().unwrap_or(0.0);
        let p = position.get(i).copied().unwrap_or(0.0);
        let t = temperature.get(i).copied().unwrap_or(0.0);

        if let Err(e) = writeln!(file, "{},{},{:.4},{:.4},{:.4}", load_level, i, f, p, t) {
            error!("Failed to write sensor CSV row: {}", e);
            return;
        }
    }

    info!("Appended {} sensor samples to sensors_all.csv", max_len);
}

/// Appends actuator commands to sweep-wide CSV: load_level,sample_index,gripper,motor,stabiliser.
/// Creates header on first write; enables trending of actuator behavior across load levels.
fn append_to_consolidated_csv_actuators(
     path: PathBuf,
    load_level: usize,
    gripper: &VecDeque<f64>,
    motor: &VecDeque<f64>,
    stabiliser: &VecDeque<f64>,
) {
    let file_exists = path.exists();
    
    let mut file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to open actuator CSV: {}", e);
            return;
        }
    };

    // Write header on first experiment
    if !file_exists {
        if let Err(e) = writeln!(file, "load_level,sample_index,gripper,motor,stabiliser") {
            error!("Failed to write actuator CSV header: {}", e);
            return;
        }
    }

    let max_len = gripper.len().max(motor.len()).max(stabiliser.len());

    // Append rows for all commands; missing data filled with 0.0
    for i in 0..max_len {
        let g = gripper.get(i).copied().unwrap_or(0.0);
        let m = motor.get(i).copied().unwrap_or(0.0);
        let s = stabiliser.get(i).copied().unwrap_or(0.0);

        if let Err(e) = writeln!(file, "{},{},{:.4},{:.4},{:.4}", load_level, i, g, m, s) {
            error!("Failed to write actuator CSV row: {}", e);
            return;
        }
    }

    info!("Appended {} actuator samples to actuators_all.csv", max_len);
}

/// Calls all export functions: metrics summary + sweep-wide CSVs + deadline miss rate CSV.
pub fn run_exports(metrics: SharedMetrics, cpu_load_threads: usize) {
    export_metrics_to_csv(metrics.clone(), cpu_load_threads);
    export_summary_csv(&metrics, cpu_load_threads);
}

/// Spawns background thread logging feedback loop events (state, errors, acks).
/// Appends rows to `data/logs/feedback_events.csv`: timestamp_us,actuator,feedback_kind,value.
/// Runs until feedback channel closes (simulator shutdown).
pub fn spawn_feedback_handler(
    feedback_rx: crossbeam::channel::Receiver<Feedback>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let log_path = "data/logs/feedback_events.csv";
        let _ = create_dir_all("data/logs");

        // Write header on first run
        let file_exists = Path::new(log_path).exists();
        if !file_exists {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .write(true)
                .open(log_path)
            {
                let _ = file.write_all(b"timestamp_us,actuator,feedback_kind,value\n");
            }
        }

        // Drain feedback channel until closed (simulator shutdown)
        while let Ok(msg) = feedback_rx.recv() {
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_path)
            {
                let timestamp_us = msg.timestamp.elapsed().as_micros();
                
                let csv_row = match msg.kind {
                    FeedbackKind::ActuatorState(value) => {
                        format!("{},{},ActuatorState,{:.2}\n", timestamp_us, msg.actuator, value)
                    }
                    FeedbackKind::Error(e) => {
                        format!("{},{},Error,{}\n", timestamp_us, msg.actuator, e)
                    }
                    FeedbackKind::Ack => {
                        format!("{},{},Ack,0\n", timestamp_us, msg.actuator)
                    }
                };

                let _ = file.write_all(csv_row.as_bytes());
            }
        }
    })
}
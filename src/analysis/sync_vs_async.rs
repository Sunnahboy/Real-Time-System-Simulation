
//! Performance analysis: threaded vs async comparison with combined dashboard.
//!
//! Reads CSV logs (sync_events_load_0.csv, async_events.csv) → extracts latency/jitter metrics
//! → generates comparison report (percentiles, throughput, drops) → outputs 2x2 dashboard HTML.

use polars::prelude::*;
use plotly::{
    common::Mode,
    layout::{Axis, Layout},
    Bar, Plot, Scatter,
};
use std::{error::Error, fs, path::Path};

/// Aggregated metrics summary for one execution mode (threaded or async).
#[derive(Debug, Clone)]
struct Summary {
    label: String,
    jitter_mean: f64,
    jitter_p95: f64,
    jitter_max: f64,
    latency_mean: f64,
    latency_p99: f64,
    latency_p95: f64,
    tx_drops: u32,
    throughput_events_sec: f64,
    latency_samples_sec: f64,
}

/// Time-series data for plotting (timestamps + values).
#[derive(Debug, Clone)]
struct TimeSeries {
    timestamps: Vec<f64>,
    latencies: Vec<f64>,
    jitters: Vec<f64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    fs::create_dir_all("data/results")?;

    // Analyze both implementations from CSV logs
    let (sync, sync_ts) = analyze_csv("Threaded", "data/logs/sync_events_load_0.csv")?;
    let (async_, async_ts) = analyze_csv("Async", "data/logs/async_events.csv")?;

    // Print individual summaries
    println!("=== THREADED IMPLEMENTATION ===");
    print_summary(&sync);

    println!("\n=== ASYNC IMPLEMENTATION ===");
    print_summary(&async_);

    // Print comparison table with percentage differences
    println!("\n=== COMPARISON ===");
    compare_implementations(&sync, &async_);

    // Generate combined 2x2 dashboard
    generate_combined_dashboard(&sync, &async_, &sync_ts, &async_ts)?;

    println!("\nDashboard generated: data/Report_results_sync_vs_async/async_vs_sync_report.html");

    Ok(())
}

/// Analyzes CSV log file: extracts latency/jitter metrics, computes percentiles, counts drops.
///
/// **Metrics extracted:**
/// - Jitter (µs) — scheduling variance from expected 5ms interval.
/// - Latency (µs) — end-to-end message latency.
/// - TX Drops — failed transmissions (backpressure indicator).
///
/// **Returns:** Summary (aggregated stats) + TimeSeries (for plotting).
fn analyze_csv(label: &str, path: &str) -> Result<(Summary, TimeSeries), Box<dyn Error>> {
    if !Path::new(path).exists() {
        println!("  Warning: {} CSV not found at {}", label, path);
        return Ok((
            empty_summary(label),
            TimeSeries {
                timestamps: vec![],
                latencies: vec![],
                jitters: vec![],
            },
        ));
    }

    // Read CSV with polars; select relevant columns
    let df = LazyCsvReader::new(path)
        .with_has_header(true)
        .finish()?
        .select([
            col("ts_epoch_us").cast(DataType::Float64),
            col("event"),
            col("value").cast(DataType::Float64),
        ])
        .collect()?;

    let timestamps = df.column("ts_epoch_us")?.f64()?;
    let events = df.column("event")?.str()?;
    let values = df.column("value")?.f64()?;

    let mut jitter_vals = Vec::new();
    let mut latency_vals = Vec::new();
    let mut time_points = Vec::new();
    let mut tx_drops = 0u32;

    // Iterate rows; categorize by event type
    for i in 0..df.height() {
        let ts = timestamps.get(i).unwrap_or(0.0);
        let event = events.get(i).unwrap_or("");
        let value = values.get(i).unwrap_or(0.0);

        if event.starts_with("jitter:") {
            jitter_vals.push(value);
        } else if event.starts_with("rx_latency:") {
            latency_vals.push(value);
            time_points.push(ts);
        } else if event == "tx_drop" || event == "tx_drop\n" {
            tx_drops += 1;
        }
    }

    // Compute duration (min-max timestamps) for throughput calculation
    let duration_sec = if time_points.is_empty() {
        1.0
    } else {
        let min_ts = time_points.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_ts = time_points.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let duration_us = (max_ts - min_ts).max(1.0);
        duration_us / 1_000_000.0
    };

    let throughput = if duration_sec > 0.0 {
        latency_vals.len() as f64 / duration_sec
    } else {
        0.0
    };

    let latency_samples_sec = if duration_sec > 0.0 {
        latency_vals.len() as f64 / duration_sec
    } else {
        0.0
    };

    let ts = TimeSeries {
        timestamps: time_points,
        latencies: latency_vals.clone(),
        jitters: jitter_vals.clone(),
    };

    Ok((
        Summary {
            label: label.to_string(),
            jitter_mean: mean(&jitter_vals),
            jitter_p95: percentile(&jitter_vals, 0.95),
            jitter_max: max(&jitter_vals),
            latency_mean: mean(&latency_vals),
            latency_p99: percentile(&latency_vals, 0.99),
            latency_p95: percentile(&latency_vals, 0.95),
            tx_drops,
            throughput_events_sec: throughput,
            latency_samples_sec,
        },
        ts,
    ))
}

/// Compares threaded vs async: prints mean, percentiles, throughput with % difference.
///
/// Handles zero-values gracefully (avoids division by zero in percentage calculation).
fn compare_implementations(threaded: &Summary, async_: &Summary) {
    println!("Latency (µs):");
    println!("  Threaded Mean:  {:.2}", threaded.latency_mean);
    println!("  Async Mean:     {:.2}", async_.latency_mean);
    if threaded.latency_mean.abs() < std::f64::EPSILON {
        println!("  Difference:     N/A (threaded mean is zero)");
    } else {
        let latency_diff = ((async_.latency_mean - threaded.latency_mean) / threaded.latency_mean) * 100.0;
        println!("  Difference:     {:+.1}%", latency_diff);
    }

    println!("\nJitter (µs):");
    println!("  Threaded P95:   {:.2}", threaded.jitter_p95);
    println!("  Async P95:      {:.2}", async_.jitter_p95);
    if threaded.jitter_p95.abs() < std::f64::EPSILON {
        println!("  Difference:     N/A (threaded jitter P95 is zero)");
    } else {
        let jitter_diff = ((async_.jitter_p95 - threaded.jitter_p95) / threaded.jitter_p95) * 100.0;
        println!("  Difference:     {:+.1}%", jitter_diff);
    }

    println!("\nP99 Latency (µs):");
    println!("  Threaded:       {:.2}", threaded.latency_p99);
    println!("  Async:          {:.2}", async_.latency_p99);
    if threaded.latency_p99.abs() < std::f64::EPSILON {
        println!("  Difference:     N/A (threaded P99 is zero)");
    } else {
        let p99_diff = ((async_.latency_p99 - threaded.latency_p99) / threaded.latency_p99) * 100.0;
        println!("  Difference:     {:+.1}%", p99_diff);
    }

    println!("\nThroughput (events/sec):");
    println!("  Threaded:       {:.2}", threaded.throughput_events_sec);
    println!("  Async:          {:.2}", async_.throughput_events_sec);
    if threaded.throughput_events_sec.abs() < std::f64::EPSILON {
        println!("  Difference:     N/A (threaded throughput is zero)");
    } else {
        let throughput_diff = ((async_.throughput_events_sec - threaded.throughput_events_sec) / threaded.throughput_events_sec) * 100.0;
        println!("  Difference:     {:+.1}%", throughput_diff);
    }

    println!("\nTX Drops:");
    println!("  Threaded:       {}", threaded.tx_drops);
    println!("  Async:          {}", async_.tx_drops);

    println!("\nCPU Utilization:");
    println!("  Measure with /usr/bin/time -v:");
    println!("    /usr/bin/time -v target/release/rts_simulation (threaded)");
    println!("    /usr/bin/time -v target/release/async_main (async)");
}

/// Generates combined 2x2 HTML dashboard: timing comparison, throughput, latency time-series, jitter distribution.
///
/// **Layout:**
/// - Top-left: P99 latency + P95 jitter bar chart.
/// - Top-right: Throughput comparison.
/// - Bottom-left: Latency over time (scatter).
/// - Bottom-right: Jitter distribution (scatter).
fn generate_combined_dashboard(
    s1: &Summary,
    s2: &Summary,
    ts1: &TimeSeries,
    ts2: &TimeSeries,
) -> Result<(), Box<dyn Error>> {
    use plotly::layout::GridPattern;
    
    let mut plot = Plot::new();

    // Chart 1 (top-left): P99 latency + P95 jitter bars
    plot.add_trace(
        Bar::new(
            vec![s1.label.clone(), s2.label.clone()],
            vec![s1.latency_p99, s2.latency_p99],
        )
        .name("P99 Latency (µs)")
        .x_axis("x")
        .y_axis("y"),
    );

    plot.add_trace(
        Bar::new(
            vec![s1.label.clone(), s2.label.clone()],
            vec![s1.jitter_p95, s2.jitter_p95],
        )
        .name("P95 Jitter (µs)")
        .x_axis("x")
        .y_axis("y"),
    );

    // Chart 2 (top-right): Throughput comparison
    plot.add_trace(
        Bar::new(
            vec![s1.label.clone(), s2.label.clone()],
            vec![s1.throughput_events_sec, s2.throughput_events_sec],
        )
        .name("Throughput (events/s)")
        .x_axis("x2")
        .y_axis("y2"),
    );

    // Chart 3 (bottom-left): Latency time-series
    if !ts1.timestamps.is_empty() {
        plot.add_trace(
            Scatter::new(ts1.timestamps.clone(), ts1.latencies.clone())
                .name(&format!("{} Latency", s1.label))
                .mode(Mode::Markers)
                .x_axis("x3")
                .y_axis("y3"),
        );
    }

    if !ts2.timestamps.is_empty() {
        plot.add_trace(
            Scatter::new(ts2.timestamps.clone(), ts2.latencies.clone())
                .name(&format!("{} Latency", s2.label))
                .mode(Mode::Markers)
                .x_axis("x3")
                .y_axis("y3"),
        );
    }

    // Chart 4 (bottom-right): Jitter distribution
    if !ts1.jitters.is_empty() {
        plot.add_trace(
            Scatter::new(
                (0..ts1.jitters.len()).map(|i| i as f64).collect(),
                ts1.jitters.clone(),
            )
            .name(&format!("{} Jitter", s1.label))
            .mode(Mode::Markers)
            .x_axis("x4")
            .y_axis("y4"),
        );
    }

    if !ts2.jitters.is_empty() {
        plot.add_trace(
            Scatter::new(
                (0..ts2.jitters.len()).map(|i| i as f64).collect(),
                ts2.jitters.clone(),
            )
            .name(&format!("{} Jitter", s2.label))
            .mode(Mode::Markers)
            .x_axis("x4")
            .y_axis("y4"),
        );
    }

    // Build 2x2 grid layout
    let layout = Layout::new()
        .title("RTS Simulation: Threaded vs Async Performance")
        .height(1000)
        .width(1600)
        .show_legend(true)
        .grid(
            plotly::layout::LayoutGrid::new()
                .rows(2)
                .columns(2)
                .pattern(GridPattern::Independent),
        )
        // Top-left: Timing comparison
        .x_axis(Axis::new().title("Implementation").domain(&[0.0, 0.48]))
        .y_axis(Axis::new().title("Microseconds (µs)").domain(&[0.55, 1.0]))
        // Top-right: Throughput
        .x_axis2(Axis::new().title("Implementation").domain(&[0.52, 1.0]))
        .y_axis2(Axis::new().title("Events per second").domain(&[0.55, 1.0]))
        // Bottom-left: Latency time-series
        .x_axis3(Axis::new().title("Timestamp (µs)").domain(&[0.0, 0.48]))
        .y_axis3(Axis::new().title("Latency (µs)").domain(&[0.0, 0.45]))
        // Bottom-right: Jitter distribution
        .x_axis4(Axis::new().title("Sample Index").domain(&[0.52, 1.0]))
        .y_axis4(Axis::new().title("Jitter (µs)").domain(&[0.0, 0.45]));

    plot.set_layout(layout);
    plot.write_html("data/results/async_vs_sync_report.html");

    Ok(())
}

/// Prints detailed summary for one implementation: all metrics.
fn print_summary(s: &Summary) {
    println!("  Label:                   {}", s.label);
    println!("  Latency Mean:            {:.2} µs", s.latency_mean);
    println!("  Latency P95:             {:.2} µs", s.latency_p95);
    println!("  Latency P99:             {:.2} µs", s.latency_p99);
    println!("  Jitter Mean:             {:.2} µs", s.jitter_mean);
    println!("  Jitter P95:              {:.2} µs", s.jitter_p95);
    println!("  Jitter Max:              {:.2} µs", s.jitter_max);
    println!("  TX Drops:                {}", s.tx_drops);
    println!("  Throughput:              {:.2} events/sec", s.throughput_events_sec);
    println!("  Latency Sample Rate:     {:.2} /sec", s.latency_samples_sec);
}

// ============================================================
// Helper Functions
// ============================================================

/// Creates empty summary (for missing CSV files).
fn empty_summary(label: &str) -> Summary {
    Summary {
        label: label.to_string(),
        jitter_mean: 0.0,
        jitter_p95: 0.0,
        jitter_max: 0.0,
        latency_mean: 0.0,
        latency_p99: 0.0,
        latency_p95: 0.0,
        tx_drops: 0,
        throughput_events_sec: 0.0,
        latency_samples_sec: 0.0,
    }
}

/// Computes arithmetic mean of dataset.
fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Finds maximum value in dataset.
fn max(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
    }
}

/// Computes percentile (e.g., p=0.95 for P95). Sorts data, interpolates index.
fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut sorted = v.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = (((sorted.len() - 1) as f64) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
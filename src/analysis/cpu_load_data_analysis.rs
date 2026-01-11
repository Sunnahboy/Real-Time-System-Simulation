//! CPU load impact analysis: reads sweep results CSV → prints comparison table & ASCII plots
//! → generates interactive Chart.js HTML dashboard.
//!
//! Analyzes performance degradation across load levels 
//! Metrics: deadline misses, jitter, latency, throughput loss.

use std::{
    io::{BufRead, BufReader},
    fs::{File, write},
};

/// Single experiment result from CSV row: load level and corresponding metrics.
#[derive(Debug, Clone)]
struct ExperimentResult {
    cpu_load_threads: usize,
    deadline_miss: u64,
    total_cycles: u64,
    max_jitter_us: u64,
    avg_latency_us: u64,
}

fn main() {
    let csv_path = "data/cpu_load_results.csv";
    
    println!(" RTS Performance Analysis");
    println!("============================\n");
    
    // Read and parse CSV sweep results
    let results = read_csv(csv_path);
    
    if results.is_empty() {
        eprintln!(" No results found. Run experiments first!");
        return;
    }
    
    // Print tabular summary
    print_table(&results);
    
    // Calculate baseline vs peak degradation
    print_statistics(&results);
    
    // Generate ASCII bar charts (terminal output)
    println!("\nPERFORMANCE DEGRADATION");
    println!("============================\n");
    
    plot_deadline_misses(&results);
    plot_jitter(&results);
    plot_latency(&results);
    plot_throughput_loss(&results);
    
    // Generate interactive Chart.js HTML dashboard
    generate_html_report(&results);
}

/// Parses CSV: skip header, extract columns (load, miss, cycles, jitter, latency).
/// Handles parsing errors gracefully (skips malformed rows).
fn read_csv(path: &str) -> Vec<ExperimentResult> {
    let mut results = Vec::new();
    
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(" Failed to open {}: {}", path, e);
            return results;
        }
    };
    
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    
    // Skip header row
    let _ = lines.next();
    
    for line in lines {
        if let Ok(line) = line {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 6 {
                // CSV columns: cpu_load_threads, deadline_miss, total_cycles, miss_rate (%), jitter (us), latency (us)
                if let (Ok(cpu), Ok(miss), Ok(cycles), Ok(jitter), Ok(latency)) = (
                    parts[0].parse::<usize>(),
                    parts[1].parse::<u64>(),
                    parts[2].parse::<u64>(),
                    parts[4].parse::<u64>(),  // Index 4: max_jitter_us
                    parts[5].parse::<u64>(),  // Index 5: avg_latency_us
                ) {
                    results.push(ExperimentResult {
                        cpu_load_threads: cpu,
                        deadline_miss: miss,
                        total_cycles: cycles,
                        max_jitter_us: jitter,
                        avg_latency_us: latency,
                    });
                }
            }
        }
    }
    
    results
}

/// Prints formatted table: load level vs all metrics (human-readable).
fn print_table(results: &[ExperimentResult]) {
    println!("EXPERIMENT RESULTS");
    println!("=====================\n");
    println!(
        "{:<6} {:<15} {:<15} {:<15} {:<15}",
        "Load", "Deadline Miss", "Total Cycles", "Max Jitter (μs)", "Avg Latency (μs)"
    );
    println!("{}", "=".repeat(76));
    
    for r in results {
        println!(
            "{:<6} {:<15} {:<15} {:<15} {:<15}",
            r.cpu_load_threads, r.deadline_miss, r.total_cycles, r.max_jitter_us, r.avg_latency_us
        );
    }
    println!();
}

/// Compares baseline (load 0) vs peak (highest load): % increase in misses/jitter/latency, throughput loss.
fn print_statistics(results: &[ExperimentResult]) {
    if results.len() < 2 {
        return;
    }
    
    let first = &results[0];
    let last = &results[results.len() - 1];
    
    // Calculate percentage increases (handle zero baseline safely)
    let miss_increase = if first.deadline_miss == 0 {
        if last.deadline_miss == 0 { 0.0 } else { 100.0 }
    } else {
        ((last.deadline_miss as f64 - first.deadline_miss as f64) / first.deadline_miss as f64) * 100.0
    };
    
    let jitter_increase = if first.max_jitter_us == 0 {
        0.0
    } else {
        ((last.max_jitter_us as f64 - first.max_jitter_us as f64) / first.max_jitter_us as f64) * 100.0
    };
    
    let latency_increase = if first.avg_latency_us == 0 {
        0.0
    } else {
        ((last.avg_latency_us as f64 - first.avg_latency_us as f64) / first.avg_latency_us as f64) * 100.0
    };
    
    let throughput_loss = ((first.total_cycles as f64 - last.total_cycles as f64) / first.total_cycles as f64) * 100.0;
    
    println!("IMPACT ANALYSIS (Load: {} → {} threads)", first.cpu_load_threads, last.cpu_load_threads);
    println!("==========================================\n");
    println!("  Deadline Misses: +{:.1}%", miss_increase);
    println!("  Max Jitter:      +{:.1}%", jitter_increase);
    println!("  Avg Latency:     +{:.1}%", latency_increase);
    println!("  Throughput Loss: -{:.1}% (cycles drop from {} to {})\n", 
        throughput_loss, first.total_cycles, last.total_cycles);
}

/// ASCII bar chart: deadline misses vs CPU load (█ block).
fn plot_deadline_misses(results: &[ExperimentResult]) {
    println!("Deadline Misses vs CPU Load:");
    let max_val = results.iter().map(|r| r.deadline_miss).max().unwrap_or(1).max(1) as f64;
    
    for r in results {
        let width = if max_val > 0.0 { ((r.deadline_miss as f64 / max_val) * 40.0) as usize } else { 0 };
        println!(
            "  Load {:2}: {} {} ({})",
            r.cpu_load_threads,
            "█".repeat(width),
            " ".repeat(40usize.saturating_sub(width)),
            r.deadline_miss
        );
    }
    println!();
}

/// ASCII bar chart: max jitter vs CPU load (▓ block).
fn plot_jitter(results: &[ExperimentResult]) {
    println!("Max Jitter vs CPU Load:");
    let max_val = results.iter().map(|r| r.max_jitter_us).max().unwrap_or(1).max(1) as f64;
    
    for r in results {
        let width = if max_val > 0.0 { ((r.max_jitter_us as f64 / max_val) * 40.0) as usize } else { 0 };
        println!(
            "  Load {:2}: {} {} ({}μs)",
            r.cpu_load_threads,
            "▓".repeat(width),
            " ".repeat(40usize.saturating_sub(width)),
            r.max_jitter_us
        );
    }
    println!();
}

/// ASCII bar chart: avg latency vs CPU load (▒ block).
fn plot_latency(results: &[ExperimentResult]) {
    println!("Avg Latency vs CPU Load:");
    let max_val = results.iter().map(|r| r.avg_latency_us).max().unwrap_or(1).max(1) as f64;
    
    for r in results {
        let width = if max_val > 0.0 { ((r.avg_latency_us as f64 / max_val) * 40.0) as usize } else { 0 };
        println!(
            "  Load {:2}: {} {} ({}μs)",
            r.cpu_load_threads,
            "▒".repeat(width),
            " ".repeat(40usize.saturating_sub(width)),
            r.avg_latency_us
        );
    }
    println!();
}

/// ASCII bar chart: throughput (cycles) vs CPU load with % loss vs baseline (░ block).
fn plot_throughput_loss(results: &[ExperimentResult]) {
    println!("Throughput (cycles/30s) vs CPU Load:");
    let max_val = results.iter().map(|r| r.total_cycles).max().unwrap_or(1).max(1) as f64;
    
    for r in results {
        let width = if max_val > 0.0 { ((r.total_cycles as f64 / max_val) * 40.0) as usize } else { 0 };
        let loss = if let Some(first) = results.first() {
            (1.0 - r.total_cycles as f64 / first.total_cycles as f64) * 100.0
        } else {
            0.0
        };
        println!(
            "  Load {:2}: {} {} ({} cycles, -{:.1}%)",
            r.cpu_load_threads,
            "░".repeat(width),
            " ".repeat(40usize.saturating_sub(width)),
            r.total_cycles,
            loss
        );
    }
    println!();
}

/// Generates interactive 2x2 Chart.js dashboard: deadline misses, jitter, latency, throughput.
/// Creates colorful line/bar charts with responsive layout; outputs to HTML file.
fn generate_html_report(results: &[ExperimentResult]) {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>RTS Performance Analysis</title>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/Chart.js/3.9.1/chart.min.js"></script>
    <style>
        body { 
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; 
            margin: 20px; 
            background: linear-gradient(135deg, #36373aff 0%, #333135ff 100%);
            min-height: 100vh;
        }
        .container { 
            max-width: 1400px; 
            margin: 0 auto; 
        }
        h1 { 
            color: white; 
            text-align: center;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.3);
            margin-bottom: 10px;
        }
        .subtitle {
            color: rgba(255,255,255,0.9);
            text-align: center;
            margin-bottom: 30px;
            font-size: 1.1em;
        }
        .chart-container { 
            background: white; 
            padding: 25px; 
            margin: 20px 0; 
            border-radius: 12px; 
            box-shadow: 0 8px 32px rgba(0,0,0,0.2);
            backdrop-filter: blur(10px);
        }
        .chart-container h2 {
            color: #333;
            margin-top: 0;
            border-bottom: 3px solid #232325ff;
            padding-bottom: 10px;
        }
        canvas { max-height: 400px; }
        .grid-2 {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 20px;
        }
        @media (max-width: 900px) {
            .grid-2 {
                grid-template-columns: 1fr;
            }
        }
    </style>
</head>
<body>
    <div class="container">
        <h1> Real-Time System Performance Analysis</h1>
        <p class="subtitle">CPU Load Interference Study: Deadline Misses, Jitter & Latency</p>
        
        <div class="grid-2">
            <div class="chart-container">
                <h2>Deadline Misses vs CPU Load</h2>
                <canvas id="missChart"></canvas>
            </div>
            
            <div class="chart-container">
                <h2>Maximum Jitter vs CPU Load</h2>
                <canvas id="jitterChart"></canvas>
            </div>
        </div>
        
        <div class="grid-2">
            <div class="chart-container">
                <h2>Average Latency vs CPU Load</h2>
                <canvas id="latencyChart"></canvas>
            </div>
            
            <div class="chart-container">
                <h2>Throughput Loss (Total Cycles)</h2>
                <canvas id="throughputChart"></canvas>
            </div>
        </div>
    </div>
    
    <script>
"#
    );
    
    // Extract data arrays for Chart.js
    let loads: Vec<usize> = results.iter().map(|r| r.cpu_load_threads).collect();
    let misses: Vec<u64> = results.iter().map(|r| r.deadline_miss).collect();
    let jitters: Vec<u64> = results.iter().map(|r| r.max_jitter_us).collect();
    let latencies: Vec<u64> = results.iter().map(|r| r.avg_latency_us).collect();
    let cycles: Vec<u64> = results.iter().map(|r| r.total_cycles).collect();
    
    html.push_str(&format!(
        r#"
        const loads = {:?};
        const misses = {:?};
        const jitters = {:?};
        const latencies = {:?};
        const cycles = {:?};
        
        const commonOptions = {{
            responsive: true,
            maintainAspectRatio: true,
            plugins: {{
                legend: {{ display: true, position: 'top' }},
                title: {{ display: false }}
            }},
            scales: {{
                y: {{ beginAtZero: true, grid: {{ color: 'rgba(200, 200, 200, 0.1)' }} }}
            }}
        }};
        
        // Chart 1: Deadline Misses (line chart)
        new Chart(document.getElementById('missChart'), {{
            type: 'line',
            data: {{
                labels: loads.map(l => l + ' threads'),
                datasets: [{{
                    label: 'Deadline Misses',
                    data: misses,
                    borderColor: '#ff6384',
                    backgroundColor: 'rgba(255, 99, 132, 0.15)',
                    borderWidth: 3,
                    fill: true,
                    tension: 0.4,
                    pointRadius: 6,
                    pointBackgroundColor: '#ff6384',
                    pointBorderColor: '#fff',
                    pointBorderWidth: 2
                }}]
            }},
            options: commonOptions
        }});
        
        // Chart 2: Jitter (line chart)
        new Chart(document.getElementById('jitterChart'), {{
            type: 'line',
            data: {{
                labels: loads.map(l => l + ' threads'),
                datasets: [{{
                    label: 'Max Jitter (μs)',
                    data: jitters,
                    borderColor: '#36a2eb',
                    backgroundColor: 'rgba(54, 162, 235, 0.15)',
                    borderWidth: 3,
                    fill: true,
                    tension: 0.4,
                    pointRadius: 6,
                    pointBackgroundColor: '#36a2eb',
                    pointBorderColor: '#fff',
                    pointBorderWidth: 2
                }}]
            }},
            options: commonOptions
        }});
        
        // Chart 3: Latency (line chart)
        new Chart(document.getElementById('latencyChart'), {{
            type: 'line',
            data: {{
                labels: loads.map(l => l + ' threads'),
                datasets: [{{
                    label: 'Avg Latency (μs)',
                    data: latencies,
                    borderColor: '#4bc0c0',
                    backgroundColor: 'rgba(75, 192, 192, 0.15)',
                    borderWidth: 3,
                    fill: true,
                    tension: 0.4,
                    pointRadius: 6,
                    pointBackgroundColor: '#4bc0c0',
                    pointBorderColor: '#fff',
                    pointBorderWidth: 2
                }}]
            }},
            options: commonOptions
        }});
        
        // Chart 4: Throughput (bar chart with gradient colors)
        new Chart(document.getElementById('throughputChart'), {{
            type: 'bar',
            data: {{
                labels: loads.map(l => l + ' threads'),
                datasets: [{{
                    label: 'Total Cycles (30s)',
                    data: cycles,
                    backgroundColor: [
                        'rgba(102, 126, 234, 0.8)',
                        'rgba(118, 75, 162, 0.8)',
                        'rgba(237, 100, 166, 0.8)',
                        'rgba(255, 154, 158, 0.8)',
                        'rgba(255, 193, 7, 0.8)',
                        'rgba(156, 39, 176, 0.8)',
                        'rgba(76, 175, 80, 0.8)',
                        'rgba(255, 152, 0, 0.8)',
                        'rgba(33, 150, 243, 0.8)',
                        'rgba(244, 67, 54, 0.8)'
                    ],
                    borderWidth: 2,
                    borderColor: '#333'
                }}]
            }},
            options: {{
                responsive: true,
                maintainAspectRatio: true,
                plugins: {{
                    legend: {{ display: true, position: 'top' }}
                }},
                scales: {{
                    y: {{ beginAtZero: true, grid: {{ color: 'rgba(200, 200, 200, 0.1)' }} }}
                }}
            }}
        }});
    </script>
</body>
</html>"#,
        loads, misses, jitters, latencies, cycles
    ));
    
    if let Ok(()) = write("data/Report_results_sync_vs_async/cpu_load_analysis_report.html", html) {
        println!("HTML report generated: data/results/cpu_load_analysis_report.html");
    }
}
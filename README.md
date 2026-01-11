# RTS Simulation

A comprehensive Rust-based implementation of a real-time sensor-actuator system designed to simulate and analyze the performance characteristics of real-time embedded systems. This project focuses on modeling critical aspects of real-time systems including latency, jitter, synchronization, and resource contention under various load conditions.

## Features

### Core Features
- **Component-based Architecture**: Modular design separating sensor data generation, processing, and actuation
- **Real-time Constraints**: Simulation of periodic sensor sampling and processing deadlines
- **Multiple Synchronization Modes**: Support for both lock-free and mutex-based synchronization
- **CPU Load Interference**: Configurable background CPU load to simulate resource contention
- **Comprehensive Metrics**: Collection of latency, jitter, throughput, and system utilization metrics
- **Visualization Dashboard**: Real-time web-based monitoring interface
- **Async Support**: Comparison between traditional threading and async/await implementations
- **Benchmarking Suite**: Micro-benchmarks for individual system components

### Advanced Features
- **Fault Injection & Recovery**: Simulated sensor dropouts and fault detection mechanisms
- **Fail-Safe Mode**: Graceful degradation with fallback behavior
- **Real-Time Dashboards**: Live monitoring interface for sensor, actuator, and timing metrics
- **Async vs. Multi-threaded Comparison**: Performance analysis across concurrency models
- **Message Broker Integration**: Optional distributed architecture support
- **Feedback Loop Analysis**: Closed-loop system with acknowledgments and dynamic recalibration

## Prerequisites

- **Rust 2021+**: Install via [rustup.rs](https://rustup.rs/)
- **Git**: For cloning the repository
- **System**: Linux/macOS/Windows with 4+ cores
- **Linux (optional)**: `taskset` for process-level core pinning

## Quick Start

```bash
# Clone the repository
git clone <repository-url>
cd rts_simulation

# Build the project (use --release for accurate timing measurements)
cargo build --release

# Run the main simulation
cargo run --release --bin rts_simulation

# Linux: Pin to core 0 for deterministic CPU load simulation
taskset -c 0 cargo run --release --bin rts_simulation

# Run async/await variant
cargo run --release --bin async_main

# Execute all benchmarks
cargo bench

# Run specific benchmark
cargo bench feedback_bench

# Analyze CPU load impact
cargo run --release --bin analyze

# Compare sync vs async performance
cargo run --release --bin sync_vs_async
```

## System Overview

This real-time simulation models an automated manufacturing control system with two integrated components:

### Main Simulation

To run the main real-time simulation with default parameters:

```bash
cargo run --release --bin rts_simulation
```

The `--release` flag is recommended for accurate performance benchmarks and simulations.

### Async Pipeline

To run the async/await implementation for comparison:

```bash
cargo run --release --bin async_main
```

## Running Benchmarks

The project includes comprehensive benchmarks for performance analysis:

```bash
# Run all benchmarks
cargo bench

# Run a specific benchmark
cargo bench feedback_bench
```

Available benchmarks include:

- `sensor_bench`: Sensor data generation performance
- `processing_bench`: Data processing pipeline timing
- `feedback_bench`: Feedback loop performance
- `multi_actuator_bench`: Multi-actuator command generation
- `pid_bench`: PID controller computation
- `actuator_deadline_bench`: Deadline scheduling performance
- `receiver_latency_bench`: Data reception latency
- `transmit_bench`: Data transmission throughput
- `sync_contention_bench`: Synchronization contention analysis
- `filter_bench`: Filtering performance
- `ipc_bench`: Inter-process communication benchmarks

## Data Analysis

Run the analysis tools to process simulation results:

```bash
# Analyze CPU load impact on latency
cargo run --release --bin analyze

# Compare sync vs async performance
cargo run --release --bin sync_vs_async
```

## Viewing Results

Simulation results and visualizations are generated in the `data/` directory:

### Live Dashboard

- **While running**: Navigate to `http://127.0.0.1:8080` for real-time metrics visualization
- **Offline**: Open `data/results/dashboard.html` or `data/LiveDashboard/dashboard.html` in a web browser

### Generated Data

- **Raw Logs**: Event data in `data/logs/` directory
  - Sync events: `sync_events_load_*.csv`
  - Async events: `async_events.csv`
  - Feedback events: `feedback_events.csv`
  - CPU load results: `cpu_load_results.csv`

- **Processed Data**: Aggregated results in `data/export/`
  - `sensors_all.csv`: Sensor data summary
  - `actuators_all.csv`: Actuator data summary
  - `metrics_summary_load_*.csv`: Metrics for different CPU loads (0, 2, 4, 8, 12, 16, 18, 20 threads)

- **Analysis Reports**: HTML reports in `data/results/`
  - `async_vs_sync_report.html`: Comparison of synchronous vs asynchronous implementations
  - `cpu_load_analysis_report.html`: CPU load impact analysis
  - `latency_timeseries.html`: Latency visualization over time
  - `jitter_distribution.html`: Jitter distribution analysis
  - `dashboard.svg`: Dashboard visualization

## Configuration

The simulation can be customized via source code parameters in `src/main.rs`:

- **Sensor sampling rates**: Modify periodic sensor generation intervals
- **Processing deadlines**: Adjust data processing deadline constraints
- **CPU load levels**: Configure background thread counts (0, 2, 4, 8, 12, 16, 18, 20 threads)
- **Synchronization modes**: Toggle between lock-free and mutex-based synchronization in `src/component_a/sync_manager.rs`
- **Buffer sizes**: Modify inter-component communication buffer configurations

Interactive menu options during execution allow selection of CPU load levels without code changes.

## Reproducible Testing

For consistent and reproducible results:

**Critical points:**

- Always use `--release` flag (debug builds produce inaccurate timing measurements)
- Run benchmarks 3-5 times and average the results
- Use `taskset -c 0` on Linux for strict single-core pinning
- Minimize background processes during benchmarking
- Ensure consistent hardware conditions across test runs

**Customize for specific scenarios:**

- Modify deadlines/sampling in `src/main.rs` constants
- Select CPU load levels via the interactive CLI menu
- Toggle synchronization modes in `src/component_a/sync_manager.rs`
- Choose architecture: `--release` (threaded) vs `--bin async_main` (async)

## Project Structure

```
rts_simulation/
├── Cargo.toml                 # Project configuration & dependencies
├── Cargo.lock                 # Dependency lock file
│
├── src/                       # Source code
│ ├── main.rs                  # Main threaded simulation entry point
│ ├── lib.rs                   # Library exports
│ │
│ ├── advanced/                # Async implementations & CPU load simulation
│ │ ├── mod.rs
│ │ ├── async_sensor.rs        # Async sensor generation
│ │ ├── async_processor.rs     # Async data processing
│ │ ├── async_transmitter.rs   # Async data transmission
│ │ ├── async_pipeline.rs      # Complete async pipeline
│ │ ├── cpu_load.rs            # CPU load interference simulation
│ │ └── dashboard.rs           # Web dashboard server
│ │
│ ├── analysis/                # Data analysis tools
│ │ ├── cpu_load_data_analysis.rs  # CPU load impact analysis
│ │ └── sync_vs_async.rs           # Sync vs async comparison
│ │
│ ├── bin/                     # Additional binaries
│ │ └── async_main.rs          # Async simulation entry point
│ │
│ ├── component_a/             # Sensor generation & processing
│ │ ├── mod.rs
│ │ ├── sensor.rs              # Sensor data generation
│ │ ├── processor.rs           # Data filtering & anomaly detection
│ │ ├── transmitter.rs         # Data transmission to component B
│ │ └── sync_manager.rs        # Synchronization management
│ │
│ ├── component_b/             # Actuation & control
│ │ ├── mod.rs
│ │ ├── receiver.rs            # Data reception from component A
│ │ ├── controller.rs          # PID controller implementation
│ │ ├── multi_actuator.rs      # Multi-actuator command generation
│ │ ├── feedback.rs            # Feedback loop processing
│ │ └── sync_manager.rs        # Synchronization management
│ │
│ └── utils/                   # Utility functions
│ ├── mod.rs
│ ├── metrics.rs               # Performance metrics collection
│ ├── export.rs                # Data export utilities
│ └── metrics_export.rs        # Metrics export functions
│
├── benches/                   # Performance benchmarks
│ ├── actuator_deadline_bench.rs    # Deadline scheduling benchmarks
│ ├── feedback_bench.rs             # Feedback loop benchmarks
│ ├── filter_bench.rs               # Filtering benchmarks
│ ├── ipc_bench.rs                  # Inter-process communication benchmarks
│ ├── multi_actuator_bench.rs       # Multi-actuator benchmarks
│ ├── pid_bench.rs                  # PID controller benchmarks
│ ├── processing_bench.rs           # Data processing benchmarks
│ ├── receiver_latency_bench.rs     # Data reception latency benchmarks
│ ├── sensor_bench.rs               # Sensor generation benchmarks
│ ├── sync_contention_bench.rs      # Synchronization contention benchmarks
│ └── transmit_bench.rs             # Data transmission benchmarks
│
├── data/                      # Generated results & logs
│ ├── dash_live_results/       # Live dashboard results
│ │ ├── actuators_all.csv
│ │ ├── sensors_all.csv
│ │ └── metrics_summary_load_*.csv   # Metrics for loads 0,2,4,8,12,16,18,20
│ │
│ ├── LiveDashboard/           # Live dashboard files
│ │ ├── dashboard.html
│ │ └── dashboard.svg
│ │
│ ├── logs/                    # Raw event logs
│ │ ├── async_events.csv       # Async simulation events
│ │ ├── sync_events_load_*.csv # Sync events for specific loads
│ │ ├── events_load_*.csv      # Events for loads 0,2,4,8,12,16,18,20
│ │ ├── feedback_events.csv    # Feedback loop events
│ │ └── cpu_load_results.csv   # CPU load results
│ │
│ └── results/                 # Analysis reports
│ ├── async_vs_sync_report.html        # Sync vs async comparison
│ ├── cpu_load_analysis_report.html    # CPU load impact analysis
│ ├── latency_timeseries.html          # Latency visualization
│ ├── jitter_distribution.html         # Jitter analysis
│ └── dashboard.svg                    # Dashboard visualization
│
└── config/                    # Configuration files
└── config.toml               # Configuration parameters (uses defaults if empty)
```

## Dependencies

Key dependencies include:

- `tokio`: Async runtime for async/await implementations
- `crossbeam`: Lock-free concurrency primitives
- `criterion`: Benchmarking framework for performance analysis
- `polars`: Data analysis and manipulation
- `plotly`: Interactive visualization library
- `plotters`: Chart and graph generation

## System Architecture

### Component A – Sensor Data Simulator

Generates and processes sensor data with strict real-time constraints:

- **Sensor Generation**: Simulates force, position, and temperature sensors with fixed sampling intervals (e.g., 5 ms)
- **Data Processing**: Applies noise-reduction filters (moving average) and anomaly detection
- **Processing Deadline**: Each processing cycle completes within 0.2 ms
- **Real-Time Transmission**: Data transmitted within 0.1 ms after processing
- **Shared Resource Access**: Periodic access to diagnostic logs and configuration buffers with appropriate synchronization

### Component B – Actuator Commander

Controls robotic arm based on sensor inputs with predictive control:

- **Data Reception**: Efficient receiver module with minimal latency
- **Predictive Control**: PID controller dynamically adjusts actuation based on sensor input
- **Multi-Actuator Support**: Handles multiple concurrent actuators (grippers, motors, stabilizers) with 1–2 ms per-actuator deadlines
- **Feedback Loop**: Sends acknowledgments, actuator state, and error data back to sensor module within 0.5 ms
- **Dynamic Recalibration**: Threshold adjustment based on feedback from actuator performance

### Integration Architecture

Choose one of the following:

**Multi-Threaded Integration**
- Both modules as concurrent threads within single Rust program
- Shared memory with synchronization primitives (Mutex, RwLock, Arc, channels)
- Deterministic timing and contention safety

**Asynchronous Integration**
- Uses Rust's async/await model with Tokio runtime
- Asynchronous tasks sharing data via async channels
- Comparable benchmarking against threaded version

## Real-Time System Requirements

### Timing Constraints

The following deadlines simulate real-world manufacturing constraints:

- **Sensor Sampling Period**: 5 ms fixed intervals
- **Processing Deadline**: 0.2 ms per cycle
- **Transmission Deadline**: 0.1 ms after processing
- **Actuator Deadline**: 1–2 ms per actuator command
- **Feedback Deadline**: 0.5 ms after actuation

**Note**: These deadlines are intentionally challenging for standard OS environments. Students are expected to measure actual performance, identify bottlenecks, and discuss trade-offs in their reports.

### Safety & Robustness Requirements

- Handle timing violations gracefully without data corruption
- Implement shared resource synchronization preventing race conditions
- Monitor and log missed deadlines
- Detect and recover from sensor faults and dropouts
- Graceful degradation under high load conditions

## Shared Resource Synchronization

A critical feature of this system is managing concurrent access to shared resources:

- **Diagnostic Log**: Accessed by both components for error/timing data
- **Configuration Buffer**: Shared state that can be updated dynamically
- **Status Memory**: Real-time state shared across modules

Synchronization is implemented using:
- Mutexes for standard protection
- Atomic operations for lock-free reads/writes
- RwLock for read-heavy scenarios
- Crossbeam channels for message passing

Benchmark and discuss lock contention and potential priority inversion effects in your analysis.

## Optional Advanced Features

Implement any of the following for distinction-level marks:

1. **Fault Injection & Sensor Dropouts**: Simulate delayed/missing packets with recovery mechanisms
2. **Fail-Safe Mode**: Implement graceful degradation when timing constraints are violated
3. **CPU Load Simulation**: Background workloads measuring real-time performance degradation
4. **Real-Time Visualization Dashboard**: Live monitoring of sensor, actuator, and timing metrics
5. **Async vs. Multi-Threaded Comparison**: Detailed performance analysis across concurrency models
6. **Message Broker Integration**: Distributed architecture using RabbitMQ, ZeroMQ, or MQTT

## Performance Metrics & Benchmarking

The system collects and analyzes the following metrics:

- **Latency**: Time from sensor reading to actuator response
- **Jitter**: Variance in response times (predictability measure)
- **Throughput**: Sensor readings and commands per second
- **Missed Deadlines**: Count and analysis of constraint violations
- **Lock Contention**: Time spent waiting on shared resources
- **CPU Utilization**: Overall system load and per-component breakdown

All metrics are collected under varying conditions:
- Normal load (0 background threads)
- High load (2, 4, 8, 12, 16, 18, 20 background threads)
- Different synchronization modes
- Threading vs. async architectures

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make changes with appropriate tests
4. Run benchmarks to ensure performance characteristics remain unchanged (use `--release` builds)
5. Document any design decisions and trade-offs
6. Submit a pull request

## License


## References

See `DOCUMENTATION.md` for detailed technical documentation, system design, and comprehensive analysis results.

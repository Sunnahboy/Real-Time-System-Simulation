# RTS Simulation Project Documentation

## Introduction

The RTS Simulation project is a comprehensive Rust-based implementation of a real-time sensor-actuator system designed to simulate and analyze the performance characteristics of real-time embedded systems. The project focuses on modeling the critical aspects of real-time systems including latency, jitter, synchronization, and resource contention under various load conditions.

### Purpose and Scope

This simulation serves multiple purposes:

- **Performance Analysis**: Measure and analyze timing characteristics of real-time system components
- **Synchronization Strategies**: Compare different synchronization mechanisms (lock-free vs traditional locking)
- **Concurrency Models**: Evaluate threaded vs asynchronous programming approaches
- **Resource Contention**: Study the impact of CPU load interference on real-time task execution
- **Benchmarking**: Provide micro-benchmarks for individual system components

### Key Features

- **Component-based Architecture**: Modular design separating sensor data generation, processing, and actuation
- **Real-time Constraints**: Simulation of periodic sensor sampling and processing deadlines
- **Multiple Synchronization Modes**: Support for both lock-free and mutex-based synchronization
- **CPU Load Interference**: Configurable background CPU load to simulate resource contention
- **Comprehensive Metrics**: Collection of latency, jitter, throughput, and system utilization metrics
- **Visualization Dashboard**: Real-time web-based monitoring interface
- **Async Support**: Comparison between traditional threading and async/await implementations

## Related Work

### Real-Time Systems

Real-time systems are characterized by their need to respond to events within strict timing constraints. The RTS Simulation draws from established principles in real-time system design:

- **Rate Monotonic Scheduling (RMS)**: Periodic task scheduling based on execution rates
- **Fixed Priority Scheduling**: Preemptive scheduling with static priority assignment
- **Real-Time Operating Systems (RTOS)**: Concepts from systems like FreeRTOS, QNX, and VxWorks

### Rust for Real-Time Systems

Rust's memory safety guarantees and zero-cost abstractions make it increasingly popular for real-time applications:

- **Memory Safety**: Compile-time prevention of data races and null pointer dereferences
- **Performance**: Native code performance comparable to C/C++
- **Concurrency**: Rich concurrency primitives including channels, mutexes, and atomics
- **Async Runtime**: Tokio and async-std provide high-performance async runtimes

### Sensor-Actuator Systems

The project models a typical sensor-actuator control loop:

- **Sensor Fusion**: Combining multiple sensor inputs for robust state estimation
- **Control Theory**: PID controllers for actuator command generation
- **Feedback Loops**: Closed-loop control with feedback from actuators to sensors
- **Signal Processing**: Filtering and anomaly detection in sensor data streams

### Performance Analysis Tools

The benchmarking and analysis components build upon:

- **Criterion.rs**: Statistical benchmarking framework for Rust
- **Polars**: High-performance data analysis library
- **Plotly**: Interactive visualization for performance metrics
- **Crossbeam**: Lock-free data structures and channels

## System Design

### Architecture Overview

The system is organized into two main components:

- **Component A**: Sensor data generation, processing, and transmission
- **Component B**: Data reception, control computation, and actuation

Data flows from sensors through processors to actuators, with feedback loops providing closed-loop control.

### Component A: Sensor Data Simulator

Component A handles the generation, processing, and transmission of sensor data:

#### Sensors

- **Types**: Force, Position, Temperature sensors
- **Generation**: Periodic sampling with configurable rates
- **Synchronization**: Lock-free event logging for timing analysis

#### Processor

- **Filtering**: Moving average noise reduction
- **Anomaly Detection**: Statistical outlier detection
- **Real-time Constraints**: 200µs processing deadline simulation

#### Transmitter

- **Buffering**: Channel-based data transmission
- **Synchronization**: Lock-free queue management
- **Flow Control**: Configurable buffer sizes and drop policies

### Component B: Actuator Commander

Component B receives processed data and generates actuator commands:

#### Receiver

- **Data Reception**: Channel-based data consumption
- **Latency Measurement**: End-to-end timing analysis

#### Multi-Actuator System

- **PID Control**: Proportional-Integral-Derivative controllers
- **Multiple Actuators**: Parallel actuator command generation
- **Deadline Scheduling**: Real-time deadline enforcement

#### Feedback Loop

- **Closed-Loop Control**: Feedback from actuators to sensors
- **Event Recording**: CSV logging of feedback events

### Synchronization Mechanisms

The system supports multiple synchronization strategies:

- **Lock-Free Mode**: Uses crossbeam channels and atomic operations
- **Mutex Mode**: Traditional mutex-based synchronization
- **Event Recording**: Timestamped event logging for analysis

### CPU Load Interference

To simulate real-world resource contention:

- **Background Threads**: Configurable number of CPU-intensive threads
- **Core Pinning**: Affinity control for processor and load threads
- **Contention Analysis**: Measurement of interference impact on real-time tasks

### Metrics and Monitoring

Comprehensive metrics collection:

- **Latency**: End-to-end processing latency
- **Jitter**: Variation in timing intervals
- **Throughput**: Events processed per second
- **CPU Utilization**: System resource usage
- **Drop Rates**: Packet loss under load

### Dashboard System

Real-time visualization:

- **Web Interface**: HTTP server with live updates
- **SVG Rendering**: Vector graphics for performance plots
- **Live Metrics**: Real-time display of system state

## Results and Discussion

### Performance Benchmarks

The system includes comprehensive benchmarks for individual components:

- **Sensor Generation**: Periodic sampling latency
- **Processing Pipeline**: Filter and anomaly detection timing
- **Transmission**: Channel throughput and latency
- **Actuation**: PID computation and command generation

### Synchronization Analysis

Comparison of synchronization strategies:

- **Lock-Free vs Mutex**: Performance trade-offs
- **Scalability**: Behavior under increasing concurrency
- **Contention**: Impact of shared resource access patterns

### CPU Load Interference Study

Analysis of background load impact:

- **Latency Degradation**: How CPU contention affects timing
- **Jitter Increase**: Timing variation under load
- **Throughput Reduction**: Capacity reduction with interference

### Threaded vs Async Implementation

The project compares traditional threading with async/await:

- **Latency Characteristics**: Mean and percentile latency measurements
- **Jitter Analysis**: Timing consistency comparison
- **Throughput Performance**: Events per second for each approach
- **Resource Utilization**: CPU and memory usage patterns

### Key Findings

Based on the analysis tools and benchmarks:

1. **Synchronization Overhead**: Lock-free approaches show reduced latency under low contention
2. **CPU Interference**: Background load significantly impacts real-time task jitter
3. **Async Benefits**: Async implementations may offer better resource utilization for I/O-bound workloads
4. **Real-time Constraints**: The system successfully simulates sub-millisecond timing requirements

### Limitations and Future Work

- **Simulation Fidelity**: Current model approximates real hardware timing
- **Hardware Abstraction**: No direct hardware interface simulation
- **Extended Scenarios**: Limited to sensor-actuator control loops
- **Scalability Testing**: Current benchmarks focus on single-core scenarios


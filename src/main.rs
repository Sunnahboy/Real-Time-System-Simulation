
//! # Real-Time System Simulation Entry Point
//!It is intended for reproducible experiments that study scheduler contention,
//!latency, and feedback stability under varying CPU background load.
//! 
//! 
//! 
//! 
//! Orchestrates sensor → processor → actuator pipeline with closed-loop feedback.
//! Measures real-time performance degradation under CPU contention via core pinning.
//!
//! ## Modes
//! - **Single Run:** 30-second simulation with fixed CPU load (0 or user-specified threads).
//! - **Sweep:** Iterates through load levels [0,2,4,8,12] measuring performance envelope.
//!
//! ## Key Architecture
//! - **Sensors (3x):** Force, Position, Temperature at 5ms intervals → bounded channel (2048).
//! - **Processor:** Anomaly detection (200µs deadline) with dynamic feedback-driven thresholds.
//! - **Actuators:** Execute commands, send feedback to processor (loop-close).
//! - **CPU Load:** Background threads on `shared_core` create contention.
//!
//! ## Concurrency
//! - Bounded channels with backpressure (sizes: 2048→1024→64).
//! - Atomic flags for graceful shutdown (`Ordering::Relaxed`).
//! - Lock-free sync option: nanosecond-precision logging without mutex overhead.
//!
//! ## Outputs
//! - `data/events_load_X.csv` — Sensor/actuator events (microsecond precision).
//! - `data/logs/sync_events_load_X.csv` — Lock-free sync log (nanosecond precision).
//! - Dashboard: `http://127.0.0.1:8080`.



mod component_a;
mod component_b;
mod utils;
mod advanced;

use component_a::{
    sensor::{Sensor, SensorType, SensorData},
    processor::Processor,
    sync_manager::{SyncManager, SyncMode},
    transmitter::Transmitter,
};

use component_b::{
    receiver::Receiving,
    multi_actuator::MultiActuator,
    feedback::{FeedbackLoop},
};

use utils::{
    metrics::{
    SharedMetrics, Metrics, EventRecorder},
    export::{run_exports, spawn_feedback_handler},
};

use advanced::{
    dashboard::start_dashboard_system,
    cpu_load::spawn_cpu_load,
};

use crossbeam::channel::bounded;
use std::{
    io::{ Write},
    path::Path,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    thread,
    time::Duration,
    io::stdout,
    io::stdin,
    fs::{create_dir_all},
    collections::{HashMap},
};
use log::{info, error};

const DEFAULT_SIMULATION_DURATION_SECS: u64 = 30;
const CPU_LOAD_SWEEP: &[usize] = &[0, 2, 4, 8, 12, 16, 18, 20];
const DEFAULT_SHARED_CORE: usize = 0;

//Maps sensor IDs to their respective names.
fn sensor_name_map() -> HashMap<u16, String> {
    let mut map = HashMap::new();
    map.insert(1, "Force".into());
    map.insert(2, "Position".into());
    map.insert(3, "Temperature".into());
    map
}

// Main entry point for the RTS simulation project.
fn main() {
    env_logger::init();
    info!("=== RTS SIMULATION START ===");
    println!("check Dashboard live at: http://127.0.0.1:8080 ");

    loop {
        let choice = prompt_menu();
        match choice.as_str() {
            "1" => {
                let cpu_load_threads = prompt_cpu_threads();
                let shared_core = prompt_core_id();
                run_simulation_with_dashboard(cpu_load_threads, shared_core);
                println!("\n Simulation completed. Returning to menu...\n");
                thread::sleep(Duration::from_secs(2));
            }
            "2" | "" => {
                println!("Running without CPU background load.");
                run_simulation_with_dashboard(0, DEFAULT_SHARED_CORE);
                println!("\n Simulation completed. Returning to menu...\n");
                thread::sleep(Duration::from_secs(2));
            }
            "3" => {
                println!("Running automatic sweep over {:?}", CPU_LOAD_SWEEP);
                run_sweep_series(CPU_LOAD_SWEEP);
                println!("\n Sweep completed. Returning to menu...\n");
                thread::sleep(Duration::from_secs(2));
            }
            "4" => {
                println!("Exiting. Goodbye!");
                info!("=== RTS SIMULATION FINISHED ===");
                return;
            }
            other => {
                println!("Unrecognized option '{}', please try again.", other);
            }
        }
    }
}

//iteractive menu for simulation selection( threaded only)
fn prompt_menu() -> String {
    println!("\n┌─────────────────────────────────────────────┐");
    println!("│     SELECT SIMULATION MODE              │");
    println!("├─────────────────────────────────────────────┤");
    println!("│  1) WITH CPU load (single run)         │");
    println!("│  2) NO CPU load (single run)           │");
    println!("│  3) AUTO SWEEP [0,2,4,8,26]           │");
    println!("│  4) Exit                               │");
    println!("└─────────────────────────────────────────────┘");
    print!("Select [1/2/3/4] (default: 2): ");
    let _ = stdout().flush();

    let mut input = String::new();
    let _ = stdin().read_line(&mut input);
    input.trim().to_string()
}

fn prompt_cpu_threads() -> usize {
    print!("Enter number of CPU load threads [default: 4]: ");
    let _ = stdout().flush();
    let mut input = String::new();
    let _ = stdin().read_line(&mut input);
    input.trim().parse::<usize>().unwrap_or(4)
}

fn prompt_core_id() -> usize {
    print!("Enter core ID to pin RT + load threads [default: 0]: ");
    let _ = stdout().flush();
    let mut input = String::new();
    let _ = stdin().read_line(&mut input);
    input.trim().parse::<usize>().unwrap_or(DEFAULT_SHARED_CORE)
}

fn run_simulation_with_dashboard(cpu_load_threads: usize, shared_core: usize) {
    let metrics: SharedMetrics = Arc::new(Mutex::new(Metrics::default()));
    {
        let mut m = metrics.lock().unwrap_or_else(|e| e.into_inner());
        m.cpu_load_threads = cpu_load_threads;
    }

    let (render_handle, web_handle, dashboard_running) = start_dashboard_system(metrics.clone());
    info!("Dashboard:");
    thread::sleep(Duration::from_millis(1500));

    run_simulation_internal(cpu_load_threads, shared_core, metrics, Some(render_handle), Some(web_handle), Some(dashboard_running));
}


/// Run a sweep series of simulations with a shared dashboard.
/// This function takes a list of CPU load thread counts and runs a simulation for each level.
/// It starts a shared dashboard and reuses the same dashboard for all simulations in the sweep.
/// stopped after all simulations are complete.

fn run_sweep_series(sweep_levels: &[usize]) {
    println!("Starting automatic sweep with shared dashboard...");
    println!("Levels: {:?}", sweep_levels);
    println!("Core pinning: {}", DEFAULT_SHARED_CORE);
    
    let dashboard_metrics: SharedMetrics = Arc::new(Mutex::new(Metrics::default()));
    let (render_handle, web_handle, dashboard_running) = start_dashboard_system(dashboard_metrics.clone());
    info!("Dashboard: http://127.0.0.1:8080 (shared for entire sweep)");
    thread::sleep(Duration::from_millis(1500));

    for &level in sweep_levels {
        {
            let mut m = dashboard_metrics.lock().unwrap_or_else(|e| e.into_inner());
            *m = Metrics::default();
            m.cpu_load_threads = level;
        }

        info!("\n[SWEEP] Running level: cpu_load_threads={} on core {}", level, DEFAULT_SHARED_CORE);
        
        run_simulation_internal(
            level, 
            DEFAULT_SHARED_CORE, 
            dashboard_metrics.clone(),
            None,
            None, 
            None
        );
        
        thread::sleep(Duration::from_millis(500));
    }

    info!("[SWEEP] Stopping dashboard...");
    dashboard_running.store(false, Ordering::Release);
    thread::sleep(Duration::from_millis(800));

    info!("[SWEEP] Joining render thread...");
    match render_handle.join() {
        Ok(_) => info!("[SWEEP]  Render thread joined"),
        Err(_) => error!("[SWEEP]  Render thread join failed"),
    }
    
    info!("[SWEEP] Releasing web server...");
    drop(web_handle);
    thread::sleep(Duration::from_millis(200));

    info!("[SWEEP]  All experiments completed successfully!");
}


fn run_simulation_internal(
    cpu_load_threads: usize,
    shared_core: usize,
    metrics: SharedMetrics,
    render_handle: Option<thread::JoinHandle<()>>,
    web_handle: Option<thread::JoinHandle<()>>,
    dashboard_running: Option<Arc<AtomicBool>>,
) {
    info!(
        "[Experiment] Starting: cpu_load_threads={}, shared_core={}",
        cpu_load_threads, shared_core
    );

    // ========================================================================
    // Event Recording System
    // ========================================================================
    let event_recorder = Arc::new(EventRecorder::new());
    
    let csv_path = format!("data/logs/events_load_{}.csv", cpu_load_threads);
    create_dir_all("data").ok();
    let _exporter_handle = event_recorder.start_exporter(csv_path.clone(), cpu_load_threads);

    let running = Arc::new(AtomicBool::new(true));
    let sync = Arc::new(SyncManager::new(SyncMode::LockFree));

    if sync.mode == SyncMode::LockFree {
        let log_dir = Path::new("data/logs");
        if let Err(e) = create_dir_all(log_dir) {
            error!("Failed to create log directory {:?}: {}", log_dir, e);
            return;
        }

        let log_file = format!("data/logs/sync_events_load_{}.csv", cpu_load_threads);
        sync.start_log_consumer(
            log_file.into(),
            Some(sensor_name_map()),
        ).expect("Failed to start log consumer");
    }

    {
        let mut m = metrics.lock().unwrap_or_else(|e| e.into_inner());
        m.cpu_load_threads = cpu_load_threads;
    }
    
    // Channel sizes tuned for 5ms sensor interval + processing latency.
    // 2048: accommodates ~10 samples per processor deadline (200µs) before dropping backpressure.
    // 1024: processor output; ~5 actuator commands in flight.
    let (tx_sensors, rx_proc) = bounded::<SensorData>(2048);
    let (tx_proc, rx_act) = bounded::<component_a::processor::ProcessedPacket>(1024);

    // Feedback loop enables dynamic threshold adjustment: actuators inform processor of state.
    let (feedback_loop, feedback_rx_raw) = FeedbackLoop::new(64, event_recorder.clone());

   
    // Duplicate feedback: non-blocking sends to logger (CSV) and processor (recalibration).
    // Prevents feedback thread blocking on either channel
    let (tx_log, rx_log) = bounded(64);
    let (tx_proc_feedback, rx_proc_feedback) = bounded(64);

    thread::spawn(move || {
        while let Ok(msg) = feedback_rx_raw.recv() {
            // Non-blocking send to logger
            let _ = tx_log.try_send(msg.clone());
            // Non-blocking send to processor (for threshold adjustment)
            let _ = tx_proc_feedback.try_send(msg);
        }
    });

    // Spawn feedback handler thread (logs feedback to CSV)
    let _feedback_handler = spawn_feedback_handler(rx_log);

    let transmitter = Arc::new(
        Transmitter::new(tx_proc.clone(), 1024, sync.clone())
    );

    // Spawn three sensors pinned to shared_core.
    // All contend for same core; CPU load threads amplify contention.
    let sensors = vec![
        spawn_sensor("Force", SensorType::Force, tx_sensors.clone(), running.clone(), sync.clone(), metrics.clone(), event_recorder.clone()),
        spawn_sensor("Position", SensorType::Position, tx_sensors.clone(), running.clone(), sync.clone(), metrics.clone(), event_recorder.clone()),
        spawn_sensor("Temperature", SensorType::Temperature, tx_sensors.clone(), running.clone(), sync.clone(), metrics.clone(), event_recorder.clone()),
    ];

    // Processor: consumes SensorData → applies anomaly detection + thresholds → produces commands.
    // Pinned to shared_core. Deadline: 200µs. Feedback adjusts thresholds dynamically.
    let processor_handle = {
        let sync_p = sync.clone();
        let tx_p = transmitter.clone();
        let metrics_p = metrics.clone();
        let core = shared_core;
        let recorder = event_recorder.clone();

        thread::spawn(move || {
            // Pin processor to shared_core (contention point with CPU load)
            let core_ids = core_affinity::get_core_ids().unwrap_or_default();
            if let Some(core_id) = core_ids.get(core) {
                if core_affinity::set_for_current(*core_id) {
                    info!(" Processor pinned to core {}", core);
                } else {
                    error!("Failed to pin processor to core {}", core);
                }
            } else {
                error!("Core {} not found available system cores", core);
            }

            // Create processor with feedback channel for dynamic threshold adjustment
            let mut proc = Processor::new(
                rx_proc,              // Sensor data channel
                rx_proc_feedback,     // Feedback channel (actuator → processor)
                10,                   // window_size
                3.0,                  // anomaly_threshold (initial value)
                200,                  // deadline_us (200 µs)
                5_000,                // expected_interval_us (5 ms)
                sync_p,
                tx_p,
                metrics_p,
                recorder,
            );
            proc.run();
        })
    };

    // Receiver: consumes processor commands → drives actuators → sends feedback.
    let receiver_handle = {
        let sync_r = sync.clone();
        let metrics_r = metrics.clone();
        let feedback_r = feedback_loop.clone();
        let recorder = event_recorder.clone();

        thread::spawn(move || {
            let multi = MultiActuator::new(sync_r.clone(), feedback_r.clone(), metrics_r.clone(), recorder.clone());
            let mut receiver = Receiving::new(rx_act, sync_r, multi, feedback_r, metrics_r, recorder);
            receiver.run();
        })
    };

    //CPU load threads: background CPU-bound work on shared_core.
    // Higher thread counts increase contention; measures real-time performance degradation.
    let cpu_load_handles = spawn_cpu_load(
        cpu_load_threads,
        running.clone(),
        shared_core,
    );

    info!(
        "[Main] Spawned {} background CPU load threads on core {}",
        cpu_load_threads, shared_core
    );

    info!("[Main] Running simulation for {} seconds...", DEFAULT_SIMULATION_DURATION_SECS);
    thread::sleep(Duration::from_secs(DEFAULT_SIMULATION_DURATION_SECS));
    
    info!("[Main] Time's up! Setting running = false");
    running.store(false, Ordering::Relaxed);

    info!("[Main] Simulation complete, shutting down...");
    thread::sleep(Duration::from_millis(500));


    // Drop channel senders to signal EOF to all receivers.
    // Causes blocked recv() calls to return Err, allowing threads to join.
    drop(tx_sensors);
    drop(tx_proc);
    drop(transmitter);

    if let Some(ref db_flag) = dashboard_running {
        db_flag.store(false, Ordering::Relaxed);
    }

 // Join all worker threads (all should have exited cleanly)
    for s in sensors {
        let _ = s.join();
    }

    for h in cpu_load_handles {
        let _ = h.join();
    }

    let _ = processor_handle.join();
    let _ = receiver_handle.join();

    if let Some(handle) = render_handle {
        match handle.join() {
            Ok(_) => info!("Render thread joined"),
            Err(_) => error!("Render thread join failed"),
        }
    }
    
    if let Some(handle) = web_handle {
        info!("Web server thread stopped");
        drop(handle);
    }

    if sync.mode == SyncMode::LockFree {
        let _ = sync.stop_consumer();
    }

    thread::sleep(Duration::from_millis(500));

    run_exports(metrics, cpu_load_threads);

    info!("[Experiment] Completed: cpu_load_threads={}", cpu_load_threads);
    info!("[Experiment] Events exported to: {}", csv_path);
}




/// Spawns a sensor thread pinned to shared_core.
///
/// # Arguments
/// * `name` — Sensor identifier (logging only).
/// * `sensor_type` — Type: Force, Position, or Temperature.
/// * `tx` — Unbounded producer channel for SensorData.
/// * `running` — Atomic shutdown flag; thread exits when false.
/// * `sync` — Synchronization manager (lock-free or mutex-based logging).
/// * `metrics` — Shared metrics; sensor updates latency histograms.
/// * `event_recorder` — Event recorder; logs all sample timestamps.
/// 

fn spawn_sensor(
    name: &'static str,
    sensor_type: SensorType,
    tx: crossbeam::channel::Sender<SensorData>,
    running: Arc<AtomicBool>,
    sync: Arc<SyncManager>,
    metrics: SharedMetrics,
    event_recorder: Arc<EventRecorder>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let sensor = Sensor::new(
            name,
            5, //sample interval
            tx,
            running,
            sensor_type,
            sync,
            metrics,
            event_recorder,
        );
        sensor.run();
    })
}
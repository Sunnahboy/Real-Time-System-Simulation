//! Async pipeline simulation: alternative to threaded RTS (async_main binary).
//!
//! Orchestrates async sensors/processor via tokio → bridges to sync receiver via blocking thread.
//! Measures latency end-to-end; records all events to CSV. 30-second baseline run without CPU load.
//! For comparison with threaded implementation (rts_simulation binary).

use std::{
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Instant,
};

use tokio::{
    time::Duration,
    sync::mpsc,
};

use rts_simulation::advanced::async_pipeline::run_async_pipeline;
use rts_simulation::component_a::sync_manager::{SyncManager, SyncMode};
use rts_simulation::utils::metrics::{Metrics, EventRecorder};

const SIMULATION_DURATION_SECS: u64 = 30;

/// Async pipeline entry point: tokio multi-threaded runtime (4 workers)
/// 
/// **Architecture:**
/// - Async sensors (3x) → async processor → tx_async channel (1024 buffered)
/// - Blocking receiver thread: consumes packets, measures latency, records to sync
/// - Event recorder: logs all events (sensor release, processing, transmission) to CSV
/// - Lock-free sync: optional nanosecond-precision event logging
///
/// **Execution:*
/// 1. Initialize tokio runtime + metrics/sync/recorder
/// 2. Spawn async pipeline (sensors + processor)
/// 3. Spawn blocking receiver thread (measures latency: now - pkt.timestamp)
/// 4. Sleep 30 seconds (simulation duration
/// 5. Graceful shutdown: set running flag → flush logs → exit
///
/// **Output:**
/// - data/logs/events_async_load_0.csv — All events with microsecond timestamps
/// data/logs/async_events.csv — Lock-free sync log (nanosecond precision)
/// 
/// 
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    env_logger::init();
    println!("=== ASYNC PIPELINE START ===");

    // Shared state: metrics, sync manager, event recorder
    let running = Arc::new(AtomicBool::new(true));
    let metrics = Arc::new(Mutex::new(Metrics::default()));
    let sync = Arc::new(SyncManager::new(SyncMode::LockFree));

    // Event recording system: non-blocking queue → background CSV export
    let event_recorder = Arc::new(EventRecorder::new());
    let _exporter_handle = event_recorder.start_exporter(
        "data/logs/events_async_load_0.csv".to_string(),
        0,  // CPU load: 0 (baseline, no contention)
    );

    // Lock-free sync: optional nanosecond-precision logging
    if let Err(e) = sync.start_log_consumer(
        "data/logs/async_events.csv".into(),
        None,
    ) {
        eprintln!("Warning: failed to start log consumer: {}", e);
    }

    // Channel: async processor → blocking receiver (1024 buffered packets)
    let (tx_async, mut rx_async) =
        mpsc::channel::<rts_simulation::component_a::processor::ProcessedPacket>(1024);

    let tx_pipeline = tx_async.clone();

    // Spawn async pipeline: 3 async sensors + async processor
    run_async_pipeline(
        metrics.clone(),
        sync.clone(),
        running.clone(),
        tx_pipeline,
        event_recorder.clone(),
    )
    .await;

    // Blocking receiver thread: consumes processor output, measures latency
    // Bridges async pipeline to sync metrics/logging system
    let sync_bridge = sync.clone();

    thread::spawn(move || {
        while let Some(pkt) = rx_async.blocking_recv() {
            let now = Instant::now();
            // Latency: from processor timestamp to receiver consumption
            let latency_us =
                now.duration_since(pkt.timestamp).as_micros() as u64;

            // Record latency to sync manager (for dashboard + CSV export)
            sync_bridge.record_rx_latency(latency_us);
        }
    });

    // Run simulation for 30 seconds
    println!(
        "Running async simulation for {} seconds...",
        SIMULATION_DURATION_SECS
    );
    tokio::time::sleep(Duration::from_secs(SIMULATION_DURATION_SECS)).await;

    // Graceful shutdown: set flag → wait for threads to exit
    println!("Stopping async simulation...");
    running.store(false, Ordering::Relaxed);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Flush lock-free sync logs before exiting
    if let Err(e) = sync.stop_consumer() {
        eprintln!("Warning: failed to stop consumer: {}", e);
    }

    println!("=== ASYNC PIPELINE FINISHED ===");
}
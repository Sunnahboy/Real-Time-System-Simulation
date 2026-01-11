//! Async sensor-processor pipeline: tokio-based alternative to threaded architecture.
//!
//! Spawns three async sensors (Force, Position, Temperature) → async processor → sync actuators.
//! Sensors sample at 5ms intervals; processor filters, detects anomalies, transmits downstream.
//! Not currently active; provides future async-first execution path if needed.


use tokio::sync::mpsc;

use std::sync::{
    Arc,
    atomic::{AtomicBool},
};

use crate::advanced::{
    async_sensor::async_sensor,
    async_processor::async_processor_task,
};

use crate::component_a::{
    sensor::{SensorData, SensorType},
    processor::ProcessedPacket,
    sync_manager::SyncManager,
};

use crate::utils::metrics::{SharedMetrics, EventRecorder};

/// Spawns async sensor and processor tasks.
///
/// Sensors generate data at fixed intervals and send to the async processor.
/// The processor performs filtering and anomaly detection, then transmits
/// processed packets to the threaded receiver (Component B).
///
/// Tasks are detached; caller controls shutdown via `running` and channel drop.
/// 
#[allow(dead_code)]
pub async fn run_async_pipeline(
    metrics: SharedMetrics,
    sync: Arc<SyncManager>,
    running: Arc<AtomicBool>,
    tx_out: mpsc::Sender<ProcessedPacket>,
    event_recorder: Arc<EventRecorder>,
) {// rts_simulation/src/advanced/async_pipeline.rs
    // Sensor → Processor channel
    let (tx_sensors, rx_processor) = mpsc::channel::<SensorData>(1024);

    // ============================================================
    // Spawn async sensor tasks
    // ============================================================
    for sensor_type in [
        SensorType::Force,
        SensorType::Position,
        SensorType::Temperature,
    ] {
        let tx = tx_sensors.clone();
        let sync = sync.clone();
        let metrics = metrics.clone();
        let running = running.clone();
        let recorder = event_recorder.clone();

        tokio::spawn(async move {
            async_sensor(sensor_type, tx, sync, metrics, running, recorder).await;
            log::debug!("async sensor {:?} exited", sensor_type);
        });
    }

    // Drop parent sender so processor exits once sensors stop
    drop(tx_sensors);

    // ============================================================
    // Spawn async processor task
    // ============================================================
    let sync_p = sync.clone();
    let metrics_p = metrics.clone();
    let running_p = running.clone();
    let recorder_p = event_recorder.clone();

    tokio::spawn(async move {
        async_processor_task(
            rx_processor,
            tx_out,
            sync_p,
            metrics_p,
            running_p,
            recorder_p,
        )
        .await;

        log::debug!("async processor exited");
    });
}
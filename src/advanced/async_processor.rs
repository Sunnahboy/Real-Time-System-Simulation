
// //! Asynchronous Sensor Processing Pipeline
// //!
// //! Implements an async/await version of the sensor → processor → receiver
// //! pipeline using the Tokio runtime. This module enables direct comparison
// //! between async scheduling and traditional multi-threaded execution under
// //! identical workloads and measurement conditions.


use tokio::sync::mpsc;
use tokio::task;

use std::{
    sync::Arc,
    time::Instant,
    collections::{HashMap, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::component_a::{
    sensor::{SensorData, SensorType},
    processor::ProcessedPacket,
    sync_manager::SyncManager,
};
use crate::advanced::async_transmitter::async_transmit;
use crate::utils::metrics::{SharedMetrics, push_capped, EventRecorder, Event, DeadlineComponent,push_capped_u64};


const PROCESS_DEADLINE_US: u64 = 200;
const WINDOW_SIZE: usize = 10;


pub async fn async_processor_task(
    mut rx: mpsc::Receiver<SensorData>,
    tx: mpsc::Sender<ProcessedPacket>,
    sync: Arc<SyncManager>,
    metrics: SharedMetrics,
    running: Arc<AtomicBool>,
    event_recorder: Arc<EventRecorder>,
) {
    let mut buffers: HashMap<SensorType, VecDeque<f64>> = HashMap::new();
    let mut consecutive_overruns: u32 = 0;
    const MISS_CONFIRM_THRESHOLD: u32 = 3;

    while let Some(data) = rx.recv().await {
        if !running.load(Ordering::Relaxed) {
            break;
        }

        let cycle_start = Instant::now();

        // --------------------------------------------------------------------
        // SECTION 1: Moving average filter
        // --------------------------------------------------------------------
        let buf = buffers.entry(data.sensor_type).or_default();
        buf.push_back(data.reading);
        if buf.len() > WINDOW_SIZE {
            buf.pop_front();
        }

        let buf_snapshot = buf.clone();
        let reading_snapshot = data.reading;

        let (avg, anomaly) = match task::spawn_blocking(move || {
            if buf_snapshot.len() < 2 {
                return (reading_snapshot, false);
            }

            let mean = buf_snapshot.iter().sum::<f64>() / buf_snapshot.len() as f64;
            let variance = buf_snapshot
                .iter()
                .map(|v| {
                    let d = v - mean;
                    d * d
                })
                .sum::<f64>()
                / buf_snapshot.len() as f64;

            let std = variance.sqrt();
            let is_anomaly = std > f64::EPSILON && (reading_snapshot - mean).abs() > (3.0 * std);

            (mean, is_anomaly)
        })
        .await
        {
            Ok(r) => r,
            Err(_) => {
                log::error!("async_processor_task: blocking task failed");
                break;
            }
        };

        if anomaly {
            sync.record_custom(100 + sensor_to_id(&data.sensor_type));
        }

        // --------------------------------------------------------------------
        // T1: SensorProcessed
        // --------------------------------------------------------------------
        let t1_ns = event_recorder.now_ns();
        event_recorder.record(Event::SensorProcessed {
            seq: data.seq,
            ts_ns: t1_ns,
            filtered_value: avg,
            is_anomaly: anomaly,
        });

        let pkt = ProcessedPacket {
            sensor_type: data.sensor_type,
            filtered: avg,
            raw: data.reading,
            timestamp: cycle_start,
            seq: data.seq,
        };

        // --------------------------------------------------------------------
        // SECTION 3: Deadline enforcement (Processor – 200 µs)
        // --------------------------------------------------------------------
        let elapsed_us = cycle_start.elapsed().as_micros() as u64;

        {
            let mut m = metrics.lock().unwrap_or_else(|e| e.into_inner());

            push_capped_u64(&mut m.latency_us, elapsed_us);
            m.total_cycles += 1;

            if elapsed_us > PROCESS_DEADLINE_US {
                consecutive_overruns += 1;

                if consecutive_overruns >= MISS_CONFIRM_THRESHOLD {
                    m.record_deadline_miss(DeadlineComponent::Processor);
                    sync.record_proc_miss();
                    consecutive_overruns = 0;
                }
            } else {
                consecutive_overruns = 0;
            }

            // Keep plots aligned with threaded processor
            match data.sensor_type {
                SensorType::Force => push_capped(&mut m.force, avg),
                SensorType::Position => push_capped(&mut m.position, avg),
                SensorType::Temperature => push_capped(&mut m.temperature, avg),
            }
        }

        async_transmit(&tx, pkt, sync.clone(), event_recorder.clone()).await;
    }

    log::debug!("async_processor_task: exiting");
}


#[allow(dead_code)]
pub fn sensor_to_id(t: &SensorType) -> u16 {
    match t {
        SensorType::Force => 1,
        SensorType::Position => 2,
        SensorType::Temperature => 3,
    }
}


// //! Asynchronous Sensor Processing Pipeline
// //! This module enables direct comparison
// //! between async scheduling and traditional multi-threaded execution under
// //! identical workloads and measurement conditions.

use tokio::{
    sync::mpsc,
    time::{self, Duration, MissedTickBehavior},
};

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use crate::component_a::{
    sensor::{SensorData, SensorType},
    sync_manager::SyncManager,
};

use crate::utils::metrics::{SharedMetrics, push_capped_u64, EventRecorder, Event, DeadlineComponent, push_capped};

const PERIOD_MS: u64 = 5;

pub async fn async_sensor(
    sensor_type: SensorType,
    tx: mpsc::Sender<SensorData>,
    sync: Arc<SyncManager>,
    metrics: SharedMetrics,
    running: Arc<AtomicBool>,
    event_recorder: Arc<EventRecorder>,
) {
    let mut interval = time::interval(Duration::from_millis(PERIOD_MS));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_tick = Instant::now();
    let expected_us = PERIOD_MS * 1000;
    let sensor_id = sensor_to_id(&sensor_type);
    let mut seq: u64 = 1;

    while running.load(Ordering::Relaxed) {
        interval.tick().await;
        let now = Instant::now();

        // T0: SensorRelease (relative timestamp)
        let t0_ns = event_recorder.now_ns();
        event_recorder.record(Event::SensorRelease {
            seq,
            ts_ns: t0_ns,
            sensor_type: sensor_type.name().to_string(),
        });

        // Measure actual period and jitter
        let actual_us = now.duration_since(last_tick).as_micros() as u64;
        let jitter_us = actual_us.abs_diff(expected_us);
        last_tick = now;
        sync.record_jitter(sensor_id, jitter_us);

        // If we observed a period longer than expected, treat it as a scheduling miss
        if actual_us > expected_us {
            // Mirror threaded sensor: report scheduling miss to SyncManager (CSV)
            sync.record_proc_miss();

            // Mirror threaded sensor: record deadline miss in SharedMetrics for SENSOR
            let mut m = metrics.lock().expect("metrics mutex poisoned");
            m.record_deadline_miss(DeadlineComponent::Sensor);
            // keep metrics lock short — we'll update other fields below as needed
        }

        // Record jitter and cycle count (shared with threaded sensor)
        {
            let mut m = metrics.lock().expect("metrics mutex poisoned");
            m.total_cycles += 1;
            push_capped_u64(&mut m.jitter_us, jitter_us);
        }

        // Simulate reading
        let base = sensor_type.base_value();
        let (low, high) = sensor_type.noise_range();
        let reading = base + rand::random_range(low..high);

        // T1: SensorProcessed (simple pass-through here; filtering elsewhere)
        let filtered = reading;
        let is_anomaly = false;
        let t1_ns = event_recorder.now_ns();
        event_recorder.record(Event::SensorProcessed {
            seq,
            ts_ns: t1_ns,
            filtered_value: filtered,
            is_anomaly,
        });

        let data = SensorData {
            timestamp: now,
            reading: filtered,
            sensor_type,
            seq,
        };

        // Try to enqueue without blocking; mirror threaded sensor behaviour
        let enqueued = tx.try_send(data).is_ok();
        if enqueued {
            sync.record_sample(sensor_id);
        } else {
            sync.record_tx_drop();
        }

        // T2: SensorSent (after enqueue attempt)
        // Use Sender::capacity() to mirror previous code for queue length semantics
        let queue_len = tx.capacity() as u32;
        let t2_ns = event_recorder.now_ns();
        event_recorder.record(Event::SensorSent {
            seq,
            ts_ns: t2_ns,
            enqueued,
            queue_len,
        });

        // If the sample was enqueued, update the per-sensor data buffers (for plots),
        // matching the threaded sensor which only pushes when send succeeds.
        if enqueued {
            let mut m = metrics.lock().expect("metrics mutex poisoned");
            match sensor_type {
                SensorType::Force => push_capped(&mut m.force, filtered),
                SensorType::Position => push_capped(&mut m.position, filtered),
                SensorType::Temperature => push_capped(&mut m.temperature, filtered),
            }
        }

        seq += 1;
    }
}

#[allow(dead_code)]
pub fn sensor_to_id(t: &SensorType) -> u16 {
    match t {
        SensorType::Force => 1,
        SensorType::Position => 2,
        SensorType::Temperature => 3,
    }
}







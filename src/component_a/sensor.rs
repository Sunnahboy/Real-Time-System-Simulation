
//! sensor.rs
//! Simulates physical sensors (force, position, temperature) with periodic releases.
//! - Real-time scheduling: SpinSleeper maintains consistent sampling rates (5 ms)
//! - Deadline tracking: Reports scheduling misses to both SyncManager (CSV) and SharedMetrics (Dashboard)

use crossbeam::channel::Sender;
use rand::random_range;
use spin_sleep::{SpinSleeper, SpinStrategy};
use std::{
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};
use crate::component_a::sync_manager::SyncManager;
use crate::utils::metrics::{SharedMetrics, push_capped, push_capped_u64, EventRecorder, Event,DeadlineComponent};
use log::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SensorType {
    Force,
    Position,
    Temperature,
}

impl SensorType {
    pub fn base_value(&self) -> f64 {
        match self {
            SensorType::Force => 100.0,
            SensorType::Position => 0.0,
            SensorType::Temperature => 25.0,
        }
    }

    pub fn noise_range(&self) -> (f64, f64) {
        match self {
            SensorType::Force => (-2.0, 2.0),
            SensorType::Position => (-0.5, 0.5),
            SensorType::Temperature => (-0.2, 0.2),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            SensorType::Force => "Force",
            SensorType::Position => "Position",
            SensorType::Temperature => "Temperature",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SensorData {
    pub timestamp: Instant,
    pub reading: f64,
    pub sensor_type: SensorType,
    pub seq: u64,
}

pub struct Sensor {
    pub name: String,
    pub sampling_rate_ms: u64,
    pub tx: Sender<SensorData>,
    pub running: Arc<AtomicBool>,
    pub sensor_type: SensorType,
    pub sync: Arc<SyncManager>,
    pub metrics: SharedMetrics,
    pub event_recorder: Arc<EventRecorder>,
}

impl Sensor {
    pub fn new(
        name: &str,
        sampling_rate_ms: u64,
        tx: Sender<SensorData>,
        running: Arc<AtomicBool>,
        sensor_type: SensorType,
        sync: Arc<SyncManager>,
        metrics: SharedMetrics,
        event_recorder: Arc<EventRecorder>,
    ) -> Self {
        Self {
            name: name.to_string(),
            sampling_rate_ms,
            tx,
            running,
            sensor_type,
            sync,
            metrics,
            event_recorder,
        }
    }

    /// Main sensor loop: periodic release with real-time scheduling.
    /// Reports deadline misses to both SyncManager (CSV) and SharedMetrics (Dashboard).
    pub fn run(&self) {
        // ====================================================================
        // Real-Time Scheduling: Initialize periodic release schedule
        // ====================================================================
        let period = Duration::from_millis(self.sampling_rate_ms);
        let sleeper = SpinSleeper::new(100_000)
            .with_spin_strategy(SpinStrategy::YieldThread);

        let mut next_deadline = Instant::now() + period;
        let mut last_tick = Instant::now();
        let mut seq: u64 = 1;

        while self.running.load(Ordering::Acquire) {
            // ====================================================================
            // Real-Time Scheduling: Wait until next scheduled release
            // ====================================================================
            let now = Instant::now();
            if now < next_deadline {
                sleeper.sleep(next_deadline - now);
            } else {
                // DEADLINE MISS: Sensor woke up late (OS scheduling jitter)
                // Report to SyncManager (CSV logs)
                self.sync.record_proc_miss();
                
                // Report to SharedMetrics (Dashboard visibility)
                // This tracks SENSOR scheduling misses separately from Processor/Actuator
                {
                    let mut m = match self.metrics.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    //m.miss_sensor += 1;  // Specific counter for sensor scheduling
                    m.record_deadline_miss(DeadlineComponent::Sensor);
                }
            }

            let actual_tick = Instant::now();

            // ====================================================================
            // T0: SensorRelease event (at scheduled tick)
            // ====================================================================
            let t0_ns = self.event_recorder.now_ns();
            self.event_recorder.record(Event::SensorRelease {
                seq,
                ts_ns: t0_ns,
                sensor_type: self.sensor_type.name().to_string(),
            });

            // ====================================================================
            // Sampling Rate Consistency: Measure jitter
            // ====================================================================
            let actual_period_us = actual_tick.duration_since(last_tick).as_micros() as u64;
            let jitter_us = actual_period_us.abs_diff(self.sampling_rate_ms * 1_000);
            last_tick = actual_tick;

            // ====================================================================
            // Sensor Simulation: Generate reading with noise
            // ====================================================================
            let base = self.sensor_type.base_value();
            let (lo, hi) = self.sensor_type.noise_range();
            let reading = base + random_range(lo..hi);

            // NOTE: Filtering and anomaly detection happen in Processor (Component A)
            // This sensor only generates raw readings at fixed intervals

            // Build sensor data packet (raw reading)
            let data = SensorData {
                timestamp: actual_tick,
                reading,
                sensor_type: self.sensor_type,
                seq,
            };

            // Try to send without blocking; update sync counters on success/failure
            let mut sent = false;
            match self.tx.try_send(data) {
                Ok(_) => {
                    sent = true;
                    self.sync.record_sample(sensor_to_id(&self.sensor_type));
                }
                Err(e) => {
                    self.sync.record_tx_drop();
                    debug!("[{}] send failed: {:?}", self.name, e);
                    if e.is_disconnected() {
                        break;
                    }
                }
            }

            // ====================================================================
            // T2: SensorSent event (after enqueue attempt)
            // ====================================================================
            let queue_len = self.tx.len() as u32;
            let t2_ns = self.event_recorder.now_ns();
            self.event_recorder.record(Event::SensorSent {
                seq,
                ts_ns: t2_ns,
                enqueued: sent,
                queue_len,
            });

            // Update sensor-local metrics
            {
                let mut m = match self.metrics.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };

                if sent {
                    match self.sensor_type {
                        SensorType::Force => push_capped(&mut m.force, reading),
                        SensorType::Position => push_capped(&mut m.position, reading),
                        SensorType::Temperature => push_capped(&mut m.temperature, reading),
                    }
                }

                // Keep jitter history for diagnostics
                push_capped_u64(&mut m.jitter_us, jitter_us);
            }

            // ====================================================================
            // Real-Time Scheduling: Schedule next release
            // ====================================================================
            next_deadline += period;
            seq += 1;
        }

        debug!("[{}] stopped.", self.name);
    }
}

pub fn sensor_to_id(t: &SensorType) -> u16 {
    match t {
        SensorType::Force => 1,
        SensorType::Position => 2,
        SensorType::Temperature => 3,
    }
}


// for benchMarking

#[allow(dead_code)]
pub fn thread_sleep_tick(period_us: u64, last: &mut Instant) -> i64 {
    std::thread::sleep(Duration::from_micros(period_us));
    let now = Instant::now();
    let actual = now.duration_since(*last).as_micros() as i64;
    *last = now;
    actual - period_us as i64
}

#[allow(dead_code)]
pub fn spin_sleep_tick(
    period_us: u64,
    sleeper: &SpinSleeper,
    last: &mut Instant,
) -> i64 {
    sleeper.sleep(Duration::from_micros(period_us));
    let now = Instant::now();
    let actual = now.duration_since(*last).as_micros() as i64;
    *last = now;
    actual - period_us as i64
}
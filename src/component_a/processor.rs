//! Processor: filter sensor data, detect anomalies, enforce deadlines, adjust thresholds via feedback.
//!
//! Pipeline: raw sensor data → moving average filter → anomaly detection → deadline check → transmit.
//! Feedback loop: dynamically adjusts anomaly_threshold based on actuator state (error, ack, unstable).
//! Deadline: 200µs per cycle; consecutive misses (3x) recorded for per-component tracking.
//! 
//! 
use crossbeam::channel::{Receiver, TryRecvError};
use std::{
    time::{Duration, Instant},
    sync::Arc,
    collections::{HashMap, VecDeque},
    hint::black_box,
    thread::sleep,
};
use crate::utils::metrics::{SharedMetrics, EventRecorder,DeadlineComponent,push_capped,push_capped_u64};

use crate::component_a::{
    sensor::{SensorData, SensorType},
    transmitter::Transmitter,
    sync_manager::SyncManager,
};
use crate::component_b::feedback::{Feedback, FeedbackKind};

#[derive(Clone, Debug)]
pub struct ProcessedPacket {
    pub sensor_type: SensorType,
    pub filtered: f64,
    pub raw: f64,
    pub timestamp: Instant,
    pub seq: u64,
}

/// Processor: Filter, detect anomalies, transmit to actuators.
/// REQUIREMENT 2: Closes feedback loop via dynamic threshold adjustment.
pub struct Processor {
    rx: Receiver<SensorData>,
    feedback_rx: Receiver<Feedback>,      // Feedback channel (Component B → A)
    window_size: usize,
    anomaly_threshold: f64,               // Dynamically adjusted via feedback
    deadline_us: u64,
    expected_interval_us: u64,
    sync: Arc<SyncManager>,
    transmitter: Arc<Transmitter>,
    metrics: SharedMetrics,
    event_recorder: Arc<EventRecorder>,
}

impl Processor {
    pub fn new(
        rx: Receiver<SensorData>,
        feedback_rx: Receiver<Feedback>,  // NEW: Feedback channel
        window_size: usize,
        anomaly_threshold: f64,
        deadline_us: u64,
        expected_interval_us: u64,
        sync: Arc<SyncManager>,
        transmitter: Arc<Transmitter>,
        metrics: SharedMetrics,
        event_recorder: Arc<EventRecorder>,
    ) -> Self {
        Self {
            rx,
            feedback_rx,
            window_size,
            anomaly_threshold,
            deadline_us,
            expected_interval_us,
            sync,
            transmitter,
            metrics,
            event_recorder,
        }
    }

    /// Main processing loop.
    /// - Receives raw sensor data from channel
    /// - Processes (filter, anomaly detection, deadline check)
    /// - Transmits filtered packets downstream
    /// - REQUIREMENT 2: Reads feedback non-blockingly and adjusts anomaly_threshold
    pub fn run(&mut self) {
        println!("[Processor] started window={} deadline={}us", self.window_size, self.deadline_us);
        
        let mut buffers: HashMap<SensorType, VecDeque<f64>> = HashMap::new();
        let mut last_ts: HashMap<SensorType, Instant> = HashMap::new();
        let mut consecutive_overruns: u32 = 0;
        const MISS_CONFIRM_THRESHOLD: u32 = 3;

        loop {
            // ====================================================================
            // REQUIREMENT 2: Feedback Loop Closure
            // Check for feedback non-blockingly and adjust anomaly_threshold dynamically
            // ====================================================================
            while let Ok(fb) = self.feedback_rx.try_recv() {
                match fb.kind {
                    FeedbackKind::Error("unstable_sensor") => {
                        // Actuator detected unstable sensor: relax threshold to reduce false anomalies
                        self.anomaly_threshold *= 1.1;
                        println!(
                            "[Processor] Feedback: Unstable sensor. Relaxed threshold to {:.2}",
                            self.anomaly_threshold
                        );
                    }
                    FeedbackKind::Error("deadline_miss") => {
                        // Actuator missed deadline: tighten threshold to reduce noise
                        self.anomaly_threshold *= 0.95;
                        println!(
                            "[Processor] Feedback: Deadline miss. Tightened threshold to {:.2}",
                            self.anomaly_threshold
                        );
                    }
                    FeedbackKind::Ack => {
                        // Successful actuation: slowly fine-tune threshold toward optimum
                        if self.anomaly_threshold > 1.5 {
                            self.anomaly_threshold *= 0.999;
                        }
                    }
                    _ => {}
                }
            }

            match self.rx.try_recv() {
                Ok(data) => {
                    let cycle_start = Instant::now();
                    let sid = sensor_to_id(&data.sensor_type);

                    // Track sensor jitter (scheduling precision)
                    let jitter_abs = last_ts
                        .insert(data.sensor_type, data.timestamp)
                        .map(|prev| {
                            let actual = data.timestamp.duration_since(prev).as_micros() as i64;
                            (actual - self.expected_interval_us as i64).abs() as u64
                        })
                        .unwrap_or(0);
                    self.sync.record_jitter(sid, jitter_abs);

                    // SECTION 1 & 2: Filter data and detect anomalies
                    let (avg, is_anomaly) = self.process_data(&data, &mut buffers);

                    if is_anomaly {
                        self.sync.record_custom(100 + sid);
                    }

                    // Record filtered result event
                    let t1_ns = self.event_recorder.now_ns();
                    self.event_recorder.record(crate::utils::metrics::Event::SensorProcessed {
                        seq: data.seq,
                        ts_ns: t1_ns,
                        filtered_value: avg,
                        is_anomaly,
                    });

                    // Transmit processed packet downstream
                    let pkt = ProcessedPacket {
                        sensor_type: data.sensor_type,
                        filtered: avg,
                        raw: data.reading,
                        timestamp: cycle_start,
                        seq: data.seq,
                    };
                    self.transmitter.transmit(pkt);
                    self.sync.record_sample(sid);

                    // SECTION 3: Deadline enforcement (200 µs)
                    let elapsed_us = cycle_start.elapsed().as_micros() as u64;
                    self.update_metrics(elapsed_us, &mut consecutive_overruns, MISS_CONFIRM_THRESHOLD);
                     //self.update_metrics(elapsed_us);  
                    
                }

                Err(TryRecvError::Empty) => {
                    sleep(Duration::from_micros(50));
                }

                Err(TryRecvError::Disconnected) => {
                    println!("[Processor] channel closed; exiting");
                    break;
                }
            }
        }
    }

    /// Process sensor data: moving average filter + anomaly detection.
    /// SECTION 1: Noise-reduction filter (moving average)
    /// SECTION 2: Anomaly detection (statistical threshold - uses dynamic self.anomaly_threshold)
    /// SECTION 3b: Simulated CPU work (maintains deadline constraint)
    pub fn process_data(
        &self,
        data: &SensorData,
        buffers: &mut HashMap<SensorType, VecDeque<f64>>,
    ) -> (f64, bool) {
        // SECTION 1: Moving average filter
        let buf = buffers.entry(data.sensor_type).or_default();
        buf.push_back(data.reading);

        if buf.len() > self.window_size {
            buf.pop_front();
        }

        if buf.is_empty() {
            return (data.reading, false);
        }

        let avg = buf.iter().sum::<f64>() / buf.len() as f64;

        // Store filtered values in metrics
        {
            let mut m = match self.metrics.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            match data.sensor_type {
                SensorType::Force => push_capped(&mut m.force, avg),
                SensorType::Position => push_capped(&mut m.position, avg),
                SensorType::Temperature => push_capped(&mut m.temperature, avg),
            }
        }

        // SECTION 3b: Simulated CPU work (creates realistic deadline pressure)
        const BUSY_US: u128 = 110;
        let busy_start = Instant::now();
        while busy_start.elapsed().as_micros() < BUSY_US {
            black_box(0u64.wrapping_mul(1));
        }

        // SECTION 2: Anomaly detection (uses dynamically adjusted threshold)
        let variance = buf.iter().map(|v| (v - avg).powi(2)).sum::<f64>() / buf.len() as f64;
        let std_dev = variance.sqrt();
        let is_anomaly = (data.reading - avg).abs() > (self.anomaly_threshold * std_dev);

        (avg, is_anomaly)
    }

//     /// Update metrics and track deadline misses.
//     /// SECTION 3: Deadline enforcement (200 µs per cycle)
//     fn update_metrics(&self, elapsed_us: u64, consecutive_overruns: &mut u32, threshold: u32) {
//     let mut m = match self.metrics.lock() {
//         Ok(g) => g,
//         Err(poisoned) => poisoned.into_inner(),
//     };

//     push_capped_u64(&mut m.latency_us, elapsed_us);
//     m.total_cycles += 1;

//     // ====================================================================
//     // REQUIREMENT: Per-Component Deadline Tracking
//     // Track PROCESSOR computation misses separately (CPU overload indicator)
//     // ====================================================================
//     if elapsed_us > self.deadline_us {
//         //eprintln!("[Processor] OVERRUN: {:.0} µs > {:.0} µs", elapsed_us, self.deadline_us);
//         *consecutive_overruns += 1;
        
//         if *consecutive_overruns >= threshold {
//             // MISS CONFIRMED: Processor computation exceeded deadline
//             //m.miss_processor += 1;  // Specific counter for processor misses
//             m.record_deadline_miss(DeadlineComponent::Processor);
//             self.sync.record_proc_miss();  // Also log to CSV
//             *consecutive_overruns = 0;
//         }
//     } else {
//         *consecutive_overruns = 0;
//     }
// }

/// Tracks deadline misses: records every miss immediately + tracks consecutive for pattern detection.
///
/// **Dual approach:**
/// - **Immediate recording:** Every deadline miss logged for accuracy (real-time requirement).
/// - **Consecutive tracking:** Monitors for systemic problems (3+ misses = critical pattern).
///
/// This captures all failures while identifying sustained overload vs transient spikes.
///
/// # Arguments
/// * `elapsed_us` — Actual cycle execution time (microseconds).
/// * `consecutive_overruns` — Mutable counter; increments on overrun, resets on success.
/// * `threshold` — Consecutive threshold (typically 3); triggers critical alert when reached.
fn update_metrics(&self, elapsed_us: u64, consecutive_overruns: &mut u32, threshold: u32) {
    let mut m = match self.metrics.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    // Record latency and increment cycle counter
    push_capped_u64(&mut m.latency_us, elapsed_us);
    m.total_cycles += 1;

    // Deadline enforcement: 200µs per cycle
    if elapsed_us > self.deadline_us {
        // Record every miss immediately (accuracy for real-time monitoring)
        m.record_deadline_miss(DeadlineComponent::Processor);
        self.sync.record_proc_miss();  // Log to lock-free sync CSV
        
        // Also track consecutive misses for pattern detection
        *consecutive_overruns += 1;
        if *consecutive_overruns >= threshold {
            // Critical: sustained overload detected (3+ consecutive misses)
            eprintln!("[Processor] CRITICAL: {} consecutive deadline misses - systemic overload", *consecutive_overruns);
        }
    } else {
        // Cycle succeeded: reset consecutive counter
        *consecutive_overruns = 0;
    }
}
}

fn sensor_to_id(t: &SensorType) -> u16 {
    match t {
        SensorType::Force => 1,
        SensorType::Position => 2,
        SensorType::Temperature => 3,
    }
}
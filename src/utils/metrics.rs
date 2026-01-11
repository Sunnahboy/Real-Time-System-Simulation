
//! Metrics collection and event recording for real-time system monitoring.
//!
//! Two independent paths:
//! - **EventRecorder:** Lock-free queue (16K capacity) → background CSV export (nanosecond precision).
//! - **Metrics:** Shared mutex buffer for live dashboard (bounded to 1000 points per metric).
//!
//! Event tracing captures: sensor release → processing → transmission → actuator receipt → feedback.

use std::{
    sync::{Arc, Mutex},
    collections::VecDeque,
    fs::File,
    io::{BufWriter, Write},
    thread,
    time::{Instant, Duration},
};
use crossbeam_queue::ArrayQueue;
use log::error;

/// Event lifecycle: sensor release through feedback completion.
/// Each variant includes sequence number, nanosecond timestamp, and component-specific data.
#[derive(Debug, Clone)]
pub enum Event {
    /// Sensor raw sample acquired.
    SensorRelease {
        seq: u64,
        ts_ns: u64,
        sensor_type: String,
    },
    /// Sensor data anomaly-checked and filtered.
    SensorProcessed {
        seq: u64,
        ts_ns: u64,
        filtered_value: f64,
        is_anomaly: bool,
    },
    /// Sensor data enqueued to processor (or dropped due to full buffer).
    SensorSent {
        seq: u64,
        ts_ns: u64,
        enqueued: bool,
        queue_len: u32,
    },
    /// Processor received and dequeued sensor data.
    ActuatorReceive {
        seq: u64,
        ts_ns: u64,
    },
    /// Processor computed control output (deadline aware).
    ControllerComplete {
        seq: u64,
        ts_ns: u64,
        control_output: f64,
        exec_us: u64,
    },
    /// Actuator sent feedback to processor (loop-close signal).
    FeedbackSent {
        seq: u64,
        ts_ns: u64,
    },
    /// Processor received feedback (may adjust thresholds).
    #[allow(dead_code)]
    FeedbackReceived {
        seq: u64,
        ts_ns: u64,
    },
}

impl Event {
    /// Converts event to CSV row format: seq,pipeline,component,event,ts_ns,field1,field2,field3
    pub fn to_csv_row(&self) -> String {
        match self {
            Event::SensorRelease { seq, ts_ns, sensor_type } => {
                format!("{},threaded,sensor,SensorRelease,{},{},,", seq, ts_ns, sensor_type)
            }
            Event::SensorProcessed { seq, ts_ns, filtered_value, is_anomaly } => {
                format!("{},threaded,sensor,SensorProcessed,{},{},{},", seq, ts_ns, filtered_value, is_anomaly)
            }
            Event::SensorSent { seq, ts_ns, enqueued, queue_len } => {
                format!("{},threaded,sensor,SensorSent,{},{},{},", seq, ts_ns, enqueued, queue_len)
            }
            Event::ActuatorReceive { seq, ts_ns } => {
                format!("{},threaded,actuator,ActuatorReceive,{},,,", seq, ts_ns)
            }
            Event::ControllerComplete { seq, ts_ns, control_output, exec_us } => {
                format!("{},threaded,actuator,ControllerComplete,{},control_out={},{},", seq, ts_ns, control_output, exec_us)
            }
            Event::FeedbackSent { seq, ts_ns } => {
                format!("{},threaded,actuator,FeedbackSent,{},,,", seq, ts_ns)
            }
            Event::FeedbackReceived { seq, ts_ns } => {
                format!("{},threaded,sensor,FeedbackReceived,{},,,", seq, ts_ns)
            }
        }
    }
}

const EVENT_QUEUE_CAPACITY: usize = 16_384;

/// Non-blocking event recorder with background CSV export.
///
///Timestamps via now_ns() (elapsed nanos from recorder creation).
///record()` appends to lock-free queue; returns immediately (no blocking).
///start_exporter() spawns thread that drains queue → CSV file (one event/line).
///
/// Capacity: 16K events; drops silently if queue full (prevents event thread blocking).
pub struct EventRecorder {
    queue: Arc<ArrayQueue<Event>>,
    run_start: Instant,
}

impl EventRecorder {
    /// Creates new recorder with internal clock reference.
    pub fn new() -> Self {
        Self {
            queue: Arc::new(ArrayQueue::new(EVENT_QUEUE_CAPACITY)),
            run_start: Instant::now(),
        }
    }

    /// Appends event to queue (lock-free). Silently drops if queue full.
    #[inline]
    pub fn record(&self, event: Event) {
        let _ = self.queue.push(event);
    }

    /// Nanosecond timestamp since recorder creation.
    #[inline]
    pub fn now_ns(&self) -> u64 {
        self.run_start.elapsed().as_nanos() as u64
    }

    /// Spawns background thread draining queue → CSV file.
    /// Writes header with CPU load config; exits when queue empty + no producers.
    pub fn start_exporter(
        &self,
        output_csv: String,
        cpu_load_threads: usize,
    ) -> thread::JoinHandle<()> {
        let queue = self.queue.clone();

        thread::spawn(move || {
            match File::create(&output_csv) {
                Ok(file) => {
                    let mut writer = BufWriter::new(file);
                    let _ = writeln!(writer, "# cpu_load_threads={}", cpu_load_threads);
                    let _ = writeln!(writer, "seq,pipeline,component,event,ts_ns,field1,field2,field3");

                    loop {
                        match queue.pop() {
                            Some(event) => {
                                let _ = writeln!(writer, "{}", event.to_csv_row());
                            }
                            None => {
                                // Exit: queue empty + all producers dropped
                                thread::sleep(Duration::from_millis(10));
                                if queue.is_empty() {
                                    break;
                                }
                            }
                        }
                    }

                    let _ = writer.flush();
                }
                Err(e) => {
                    error!("Failed to create event CSV: {}", e);
                }
            }
        })
    }
}

impl Clone for EventRecorder {
    fn clone(&self) -> Self {
        Self {
            queue: self.queue.clone(),
            run_start: self.run_start,
        }
    }
}

/// Live dashboard metrics: sensor readings, actuator outputs, latency histograms, deadline misses.
/// Updated in real-time by subsystems; bounded to 1000 most recent points per metric.
#[derive(Default, Clone)]
pub struct Metrics {
    /// Sensor readings (last 1000 samples)
    pub force: VecDeque<f64>,
    pub position: VecDeque<f64>,
    pub temperature: VecDeque<f64>,

    /// Actuator outputs (last 1000 commands)
    pub gripper: VecDeque<f64>,
    pub motor: VecDeque<f64>,
    pub stabiliser: VecDeque<f64>,

    /// Latency tracking (microseconds)
    pub latency_us: VecDeque<u64>,
    pub jitter_us: VecDeque<u64>,

    /// Deadline miss counters per component
    pub miss_sensor: u64,
    pub miss_processor: u64,
    pub miss_actuator: u64,

    /// Total deadline misses across all components
    pub deadline_miss: u64,

    pub total_cycles: u64,
    pub cpu_load_threads: usize,
}

/// Component identifier for deadline miss attribution.
pub enum DeadlineComponent {
    Sensor,
    Processor,
    Actuator,
}

impl Metrics {
    /// Records deadline miss for specified component; updates total count.
    pub fn record_deadline_miss(&mut self, component: DeadlineComponent) {
        match component {
            DeadlineComponent::Sensor => self.miss_sensor += 1,
            DeadlineComponent::Processor => self.miss_processor += 1,
            DeadlineComponent::Actuator => self.miss_actuator += 1,
        }
        self.deadline_miss += 1;
    }
}

pub type SharedMetrics = Arc<Mutex<Metrics>>;

pub const MAX_POINTS: usize = 1_000;

/// Appends value to metrics buffer; removes oldest if at capacity (FIFO).
#[inline]
pub fn push_capped(buf: &mut VecDeque<f64>, val: f64) {
    if buf.len() >= MAX_POINTS {
        buf.pop_front();
    }
    buf.push_back(val);
}

/// Appends u64 value to metrics buffer; removes oldest if at capacity.
#[inline]
pub fn push_capped_u64(buf: &mut VecDeque<u64>, val: u64) {
    if buf.len() >= MAX_POINTS {
        buf.pop_front();
    }
    buf.push_back(val);
}

/// Statistics summary for a dataset.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Stats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub count: usize,
}

/// Computes min, max, mean for float buffer.
#[allow(dead_code)]
pub fn calculate_stats(data: &VecDeque<f64>) -> Option<Stats> {
    if data.is_empty() {
        return None;
    }

    let count = data.len();
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mean = data.iter().sum::<f64>() / count as f64;

    Some(Stats { min, max, mean, count })
}

/// Computes min, max, mean for u64 buffer (cast to f64).
#[allow(dead_code)]
pub fn calculate_stats_u64(data: &VecDeque<u64>) -> Option<Stats> {
    if data.is_empty() {
        return None;
    }

    let count = data.len();
    let min = data.iter().map(|&x| x as f64).fold(f64::INFINITY, f64::min);
    let max = data.iter().map(|&x| x as f64).fold(f64::NEG_INFINITY, f64::max);
    let mean = data.iter().map(|&x| x as f64).sum::<f64>() / count as f64;

    Some(Stats { min, max, mean, count })
}

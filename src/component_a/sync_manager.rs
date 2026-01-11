
//! sync_manager.rs
//! Synchronisation strategies for real-time diagnostic logging.
//! 
//! REQUIREMENT 1: Shared Resource Access
//! - Sensor and Processor both periodically record events (samples, jitter, misses, drops)
//! - Three modes trade-off contention, latency, and complexity
//!
//! REQUIREMENT 2: Synchronisation Primitives
//! - Mutex mode: Simple mutual exclusion (high contention risk)
//! - Atomics mode: Per-sensor counters (low contention, no ordering guarantees)
//! - LockFree mode: Bounded queue + background consumer thread (best for real-time)
//!
//! REQUIREMENT 3: Lock Contention & Priority Inversion
//! - Mutex: Subject to priority inversion if high-priority sensor/processor waits on low-priority consumer
//! - Atomics: Contention-free for individual counters; no waiting
//! - LockFree: Non-blocking push (drop on full); consumer runs in separate thread

use std::{
    fs::File,
    io::BufWriter,
    path::PathBuf,
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use crossbeam_queue::ArrayQueue;
use dashmap::DashMap;
use serde::Serialize;
use csv::Writer;
use log::{error, debug};

const LOG_CAPACITY: usize = 8192;        // Bounded queue size (prevents unbounded memory growth)
const CONSUMER_POLL_MS: u64 = 5;         // Consumer sleep interval (reduces busy-loop CPU)
const FLUSH_BATCHES: usize = 8;          // Batch writes before flushing to disk (reduces syscall jitter)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Mutex,      // High contention; simple
    Atomics,    // Per-counter CAS; contention-free for individual increments
    LockFree,   // Bounded queue + consumer thread; best for real-time (no blocking in producer)
}

#[derive(Debug, Clone, Copy)]
pub enum LogEventKind {
    Sample { sensor_id: u16 },
    Jitter { sensor_id: u16, jitter_us: u64 },
    ProcMiss,
    TxDrop,
    RxLatency { latency_us: u64 },
    Custom { code: u16 },
}

#[derive(Debug, Clone)]
pub struct RawLog {
    pub seq: u64,
    pub ts: Instant,
    pub kind: LogEventKind,
    pub value: f64,
}

#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    pub sample_count: HashMap<u16, u64>,
    pub jitter_sum: HashMap<u16, u64>,
    pub proc_miss_count: u64,
    pub tx_drop_count: u64,
}

#[derive(Debug, Serialize)]
struct CsvRow {
    seq: u64,
    ts_epoch_us: u64,
    age_us: u64,
    event: String,
    value: f64,
}

#[derive(Clone)]
pub struct SyncManager {
    pub mode: SyncMode,

    // ========================================================================
    // REQUIREMENT 2: MUTEX MODE (High contention, simple mutual exclusion)
    // ========================================================================
    // Single mutex protecting all diagnostics. Sensor/Processor lock on every record.
    // Risk: Priority inversion if high-priority thread blocks on low-priority holder.
    diag_mutex: Option<Arc<Mutex<Diagnostics>>>,

    // ========================================================================
    // REQUIREMENT 2: ATOMICS MODE (Contention-free per-counter)
    // ========================================================================
    // Per-sensor counters (DashMap): CAS-based increment, no mutex locks.
    // Each sensor_id gets its own AtomicU64 for sample counts and jitter sums.
    atomic_samples: Option<Arc<DashMap<u16, AtomicU64>>>,
    atomic_jitter: Option<Arc<DashMap<u16, AtomicU64>>>,
    // Global counters for proc misses and tx drops
    atomic_proc_miss: Option<Arc<AtomicU64>>,
    atomic_tx_drops: Option<Arc<AtomicU64>>,

    // ========================================================================
    // REQUIREMENT 2: LOCK-FREE MODE (Non-blocking queue + background consumer)
    // ========================================================================
    // Bounded queue (ArrayQueue): Sensor/Processor push events without blocking.
    // Consumer thread drains queue in background, batches writes, flushes periodically.
    // Risk mitigation: Non-blocking push avoids priority inversion.
    //                   Bounded queue prevents unbounded memory growth.
    //                   Batch flushing reduces syscall jitter.
    log_queue: Option<Arc<ArrayQueue<RawLog>>>,
    dropped_logs: Option<Arc<AtomicU64>>,    // Count events dropped due to queue full

    consumer_handle: Option<Arc<Mutex<Option<JoinHandle<()>>>>>,
    consumer_running: Arc<AtomicBool>,

    seq_counter: Arc<AtomicU64>,  // Sequence number for ordering events
}

impl SyncManager {
    pub fn new(mode: SyncMode) -> Self {
        SyncManager {
            mode,
            // ====================================================================
            // REQUIREMENT 1 & 2: Initialize shared resource based on mode
            // ====================================================================
            diag_mutex: if mode == SyncMode::Mutex {
                Some(Arc::new(Mutex::new(Diagnostics::default())))
            } else {
                None
            },
            atomic_samples: if mode == SyncMode::Atomics {
                Some(Arc::new(DashMap::new()))
            } else {
                None
            },
            atomic_jitter: if mode == SyncMode::Atomics {
                Some(Arc::new(DashMap::new()))
            } else {
                None
            },
            atomic_proc_miss: if mode == SyncMode::Atomics {
                Some(Arc::new(AtomicU64::new(0)))
            } else {
                None
            },
            atomic_tx_drops: if mode == SyncMode::Atomics {
                Some(Arc::new(AtomicU64::new(0)))
            } else {
                None
            },
            log_queue: if mode == SyncMode::LockFree {
                Some(Arc::new(ArrayQueue::new(LOG_CAPACITY)))
            } else {
                None
            },
            dropped_logs: if mode == SyncMode::LockFree {
                Some(Arc::new(AtomicU64::new(0)))
            } else {
                None
            },
            consumer_handle: Some(Arc::new(Mutex::new(None))),
            consumer_running: Arc::new(AtomicBool::new(false)),
            seq_counter: Arc::new(AtomicU64::new(1)),
        }
    }

    // ========================================================================
    // PRODUCER APIs: Sensor & Processor call these to record events
    // ========================================================================

    pub fn record_sample(&self, sensor_id: u16) {
        match self.mode {
            // MUTEX: Lock, modify, unlock. Risk: contention on every call.
            SyncMode::Mutex => {
                if let Some(m) = &self.diag_mutex {
                    let mut d = m.lock();
                    *d.sample_count.entry(sensor_id).or_insert(0) += 1;
                }
            }
            // ATOMICS: CAS-based increment, no lock. Contention-free.
            SyncMode::Atomics => {
                if let Some(map) = &self.atomic_samples {
                    map.entry(sensor_id)
                        .or_insert_with(|| AtomicU64::new(0))
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            // LOCK-FREE: Non-blocking push to queue. If full, increment dropped counter.
            SyncMode::LockFree => {
                if let Some(q) = &self.log_queue {
                    let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
                    let raw = RawLog {
                        seq,
                        ts: Instant::now(),
                        kind: LogEventKind::Sample { sensor_id },
                        value: 0.0,
                    };
                    if q.push(raw).is_err() {
                        if let Some(d) = &self.dropped_logs {
                            d.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    pub fn record_rx_latency(&self, latency_us: u64) {
        // Only meaningful in LockFree mode (others don't track per-event latency)
        if self.mode != SyncMode::LockFree {
            return;
        }
        let q = match &self.log_queue {
            Some(q) => q,
            None => return,
        };
        let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);

        let raw = RawLog {
            seq,
            ts: Instant::now(),
            kind: LogEventKind::RxLatency { latency_us },
            value: latency_us as f64,
        };
        // Non-blocking: if queue full, drop event and count it
        if q.push(raw).is_err() {
            if let Some(dropped) = &self.dropped_logs {
                dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn record_jitter(&self, sensor_id: u16, jitter_us: u64) {
        match self.mode {
            // MUTEX: Lock, accumulate jitter sum, unlock
            SyncMode::Mutex => {
                if let Some(m) = &self.diag_mutex {
                    let mut d = m.lock();
                    *d.jitter_sum.entry(sensor_id).or_insert(0) += jitter_us;
                }
            }
            // ATOMICS: Per-sensor atomic counter, add jitter
            SyncMode::Atomics => {
                if let Some(map) = &self.atomic_jitter {
                    map.entry(sensor_id)
                        .or_insert_with(|| AtomicU64::new(0))
                        .fetch_add(jitter_us, Ordering::Relaxed);
                }
            }
            // LOCK-FREE: Push jitter event to queue (non-blocking)
            SyncMode::LockFree => {
                if let Some(q) = &self.log_queue {
                    let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
                    let raw = RawLog {
                        seq,
                        ts: Instant::now(),
                        kind: LogEventKind::Jitter { sensor_id, jitter_us },
                        value: jitter_us as f64,
                    };
                    if q.push(raw).is_err() {
                        if let Some(d) = &self.dropped_logs {
                            d.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    pub fn record_proc_miss(&self) {
        match self.mode {
            // MUTEX: Increment proc_miss_count under lock
            SyncMode::Mutex => {
                if let Some(m) = &self.diag_mutex {
                    m.lock().proc_miss_count += 1;
                }
            }
            // ATOMICS: Global atomic counter for processor misses
            SyncMode::Atomics => {
                if let Some(a) = &self.atomic_proc_miss {
                    a.fetch_add(1, Ordering::Relaxed);
                }
            }
            // LOCK-FREE: Push miss event to queue
            SyncMode::LockFree => {
                if let Some(q) = &self.log_queue {
                    let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
                    let raw = RawLog {
                        seq,
                        ts: Instant::now(),
                        kind: LogEventKind::ProcMiss,
                        value: 0.0,
                    };
                    if q.push(raw).is_err() {
                        if let Some(d) = &self.dropped_logs {
                            d.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    pub fn record_tx_drop(&self) {
        match self.mode {
            SyncMode::Mutex => {
                if let Some(m) = &self.diag_mutex {
                    let mut d = m.lock();
                    d.tx_drop_count += 1;
                }
            }
            SyncMode::Atomics => {
                if let Some(a) = &self.atomic_tx_drops {
                    a.fetch_add(1, Ordering::Relaxed);
                }
            }
            SyncMode::LockFree => {
                if let Some(q) = &self.log_queue {
                    let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
                    let raw = RawLog {
                        seq,
                        ts: Instant::now(),
                        kind: LogEventKind::TxDrop,
                        value: 0.0,
                    };
                    if q.push(raw).is_err() {
                        if let Some(d) = &self.dropped_logs {
                            d.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    pub fn record_custom(&self, code: u16) {
        // Custom events only in LockFree mode
        if self.mode == SyncMode::LockFree {
            if let Some(q) = &self.log_queue {
                let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
                let raw = RawLog {
                    seq,
                    ts: Instant::now(),
                    kind: LogEventKind::Custom { code },
                    value: 0.0,
                };
                if q.push(raw).is_err() {
                    if let Some(d) = &self.dropped_logs {
                        d.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    // ========================================================================
    // REQUIREMENT 3: Benchmarking snapshots (measure contention effects)
    // ========================================================================

    #[cfg(feature = "bench")]
    pub fn snapshot_mutex(&self) -> Option<Diagnostics> {
        self.diag_mutex.as_ref().map(|m| m.lock().clone())
    }

    #[cfg(feature = "bench")]
    pub fn snapshot_atomics(&self) -> Option<(Vec<(u16, u64)>, u64, u64)> {
        if let (Some(samples), Some(miss), Some(tx)) =
            (&self.atomic_samples, &self.atomic_proc_miss, &self.atomic_tx_drops)
        {
            let mut out = Vec::new();
            for r in samples.iter() {
                out.push((r.key().clone(), r.value().load(Ordering::Relaxed)));
            }
            return Some((
                out,
                miss.load(Ordering::Relaxed),
                tx.load(Ordering::Relaxed),
            ));
        }
        None
    }

    #[cfg(feature = "bench")]
    pub fn dropped_log_count(&self) -> Option<u64> {
        self.dropped_logs
            .as_ref()
            .map(|a| a.load(Ordering::Relaxed))
    }

    #[cfg(feature = "bench")]
    pub fn queue_len(&self) -> Option<usize> {
        self.log_queue.as_ref().map(|q| q.len())
    }

    #[cfg(feature = "bench")]
    pub fn queue_capacity(&self) -> Option<usize> {
        self.log_queue.as_ref().map(|q| q.capacity())
    }

    // ========================================================================
    // CONSUMER THREAD: Background thread drains queue, batches writes
    // Reduces contention & syscall jitter vs. inline logging
    // ========================================================================

    pub fn start_log_consumer(
        &self,
        output_csv: PathBuf,
        sensor_map: Option<HashMap<u16, String>>,
    ) -> Result<(), String> {
        if self.mode != SyncMode::LockFree {
            return Err("start_log_consumer only valid for LockFree".into());
        }

        let q = match &self.log_queue {
            Some(q) => q.clone(),
            None => return Err("start_log_consumer called but queue missing".into()),
        };

        let dropped_logs = match &self.dropped_logs {
            Some(d) => d.clone(),
            None => return Err("start_log_consumer called but dropped_logs missing".into()),
        };

        let running = self.consumer_running.clone();

        let guard = match &self.consumer_handle {
            Some(guard) => guard.clone(),
            None => return Err("consumer_handle missing".into()),
        };

        {
            let h = guard.lock();
            if h.is_some() {
                return Err("consumer already running".into());
            }
        }

        running.store(true, Ordering::SeqCst);
        let sensor_map = sensor_map.unwrap_or_default();

        let handle = thread::spawn(move || {
            let file = match File::create(&output_csv) {
                Ok(f) => f,
                Err(e) => {
                    error!("failed to create csv file: {:?}", e);
                    return;
                }
            };
            let buf = BufWriter::new(file);
            let mut wtr = Writer::from_writer(buf);
            wtr.serialize(("seq", "ts_epoch_us", "age_us", "event", "value"))
                .ok();
            let mut flush_counter = 0usize;

            while running.load(Ordering::SeqCst) {
                let mut any = false;
                // Batch dequeue: drain up to 256 events per poll
                for _ in 0..256 {
                    match q.pop() {
                        Some(raw) => {
                            any = true;
                            let ts_epoch_micros = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_micros() as u64;
                            let age_micros = raw.ts.elapsed().as_micros() as u64;
                            let event = match raw.kind {
                                LogEventKind::Sample { sensor_id } => sensor_map
                                    .get(&sensor_id)
                                    .cloned()
                                    .unwrap_or_else(|| format!("sensor:{}", sensor_id)),
                                LogEventKind::ProcMiss => "proc_miss".to_string(),
                                LogEventKind::TxDrop => "tx_drop".to_string(),
                                LogEventKind::Jitter {
                                    sensor_id,
                                    jitter_us,
                                } => format!("jitter:{}us@sensor:{}", jitter_us, sensor_id),
                                LogEventKind::Custom { code } => format!("custom:{}", code),
                                LogEventKind::RxLatency { latency_us } => {
                                    format!("rx_latency:{}us", latency_us)
                                }
                            };
                            let row = CsvRow {
                                seq: raw.seq,
                                ts_epoch_us: ts_epoch_micros,
                                age_us: age_micros,
                                event,
                                value: raw.value,
                            };
                            wtr.serialize(&row).ok();
                        }
                        None => break,
                    }
                }
                if any {
                    flush_counter += 1;
                    // Batch flushing: only flush to disk after FLUSH_BATCHES batches
                    // Reduces syscall overhead and jitter
                    if flush_counter >= FLUSH_BATCHES {
                        wtr.flush().ok();
                        flush_counter = 0;
                    }
                } else {
                    // Queue empty: sleep to avoid busy-loop
                    thread::sleep(Duration::from_millis(CONSUMER_POLL_MS));
                }
            }

            // Final drain: flush all remaining events
            while let Some(raw) = q.pop() {
                let ts_epoch_micros = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64;
                let age_micros = raw.ts.elapsed().as_micros() as u64;
                let event = match raw.kind {
                    LogEventKind::Sample { sensor_id } => sensor_map
                        .get(&sensor_id)
                        .cloned()
                        .unwrap_or_else(|| format!("sensor:{}", sensor_id)),
                    LogEventKind::ProcMiss => "proc_miss".to_string(),
                    LogEventKind::TxDrop => "tx_drop".to_string(),
                    LogEventKind::Jitter {
                        sensor_id,
                        jitter_us,
                    } => format!("jitter:{}us@sensor:{}", jitter_us, sensor_id),
                    LogEventKind::Custom { code } => format!("custom:{}", code),
                    LogEventKind::RxLatency { latency_us } => {
                        format!("rx_latency:{}us", latency_us)
                    }
                };
                let row = CsvRow {
                    seq: raw.seq,
                    ts_epoch_us: ts_epoch_micros,
                    age_us: age_micros,
                    event,
                    value: raw.value,
                };
                wtr.serialize(&row).ok();
            }
            wtr.flush().ok();
            let final_drops = dropped_logs.load(Ordering::Relaxed);
            debug!(
                "[SyncManager::consumer] exiting. dropped_logs={}",
                final_drops
            );
        });

        {
            let mut h = guard.lock();
            *h = Some(handle);
        }
        Ok(())
    }

    pub fn stop_consumer(&self) -> Result<(), String> {
        if self.mode != SyncMode::LockFree {
            return Err("stop_consumer only valid for LockFree mode".into());
        }
        self.consumer_running.store(false, Ordering::SeqCst);
        if let Some(guard) = &self.consumer_handle {
            let handle = guard.lock().take();
            if let Some(h) = handle {
                let _ = h.join();
            }
        }
        Ok(())
    }
}

impl Drop for SyncManager {
    fn drop(&mut self) {
        if self.mode == SyncMode::LockFree {
            let _ = self.stop_consumer();
        }
    }
}




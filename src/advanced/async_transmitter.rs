//! Async transmission of processed sensor packets to actuator pipeline.
//!
//! Non-blocking try_send: records event (enqueue success/failure), updates sync stats.
//! Enables optional future async processing path (currently unused; transmitter is sync).

use tokio::sync::mpsc;
use std::sync::Arc;
use crate::component_a::{
    sync_manager::SyncManager,
    processor::ProcessedPacket,
    sensor::sensor_to_id, 
};
use crate::utils::metrics::{EventRecorder, Event};




/// Attempts non-blocking transmit of processed packet; records outcome and updates metrics.
///
/// **Flow:**
/// 1. Try-send packet to async channel (non-blocking).
/// 2. Record "SensorSent" event: seq, timestamp, enqueue success, queue capacity.
/// 3. Update sync stats: successful send → record_sample(), dropped → record_tx_drop().
///
/// # Arguments
/// * tx — MPSC sender to actuator pipeline.
/// * pkt — Processed sensor packet (filtered value, timestamp, seq).
/// * sync — Synchronization manager for lock-free event logging.
/// * event_recorder — Event recorder for CSV latency analysis.
/// 
/// 
pub async fn async_transmit(
    tx: &mpsc::Sender<ProcessedPacket>,
    pkt: ProcessedPacket,
    sync: Arc<SyncManager>,
    event_recorder: Arc<EventRecorder>,
) {
    // T2: SensorSent
    let enqueued = tx.try_send(pkt.clone()).is_ok();
    let queue_len = tx.capacity() as u32;
    let t2_ns = event_recorder.now_ns();
    event_recorder.record(Event::SensorSent {
        seq: pkt.seq,
        ts_ns: t2_ns,
        enqueued,
        queue_len,
    });

    if enqueued {
        // Match synchronous path: count a successful sample in SyncManager.
        let sid = sensor_to_id(&pkt.sensor_type);
        sync.record_sample(sid);
    } else {
        sync.record_tx_drop();
    }
}

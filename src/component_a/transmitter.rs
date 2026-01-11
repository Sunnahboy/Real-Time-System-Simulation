// //! transmitter.rs
// //! Sends processed sensor packets to Component B(actuator) in real time.
// //! - enforces 0.1 ms (100 µs) transmit deadline
// //! - uses non-blocking try_send for real-time safety
// //! - logs deadline misses + TX drops via SyncManager
// //! - measures latency for benchmarking
//! Sends processed packets to Component B via lock-free channel.


use crossbeam::channel::Sender;
use std::sync::Arc;
use crate::component_a::{
    processor::ProcessedPacket,
    sync_manager::SyncManager,
};
use log::debug;

#[derive(Clone)]
pub struct Transmitter {
    tx: Sender<ProcessedPacket>,
    max_queued: usize,
    sync: Arc<SyncManager>,
}
// rts_simulation/src/component_a/transmitter.rs
impl Transmitter {
    pub fn new(tx: Sender<ProcessedPacket>, max_queued: usize, sync: Arc<SyncManager>) -> Self {
        Self { tx, max_queued, sync }
    }

    /// Transmit processed packet to Component B.
    /// Non-blocking IPC via crossbeam channel; drops on queue saturation.
    pub fn transmit(&self, packet: ProcessedPacket) {
        // 1. Check if the channel is already full to avoid overhead
        // Backpressure: fast-path check before try_send
        if self.tx.len() >= self.max_queued {
            self.sync.record_tx_drop();
            return;
        }

        //2. Attempt non-blocking send(real-time safety)
        if let Err(err) = self.tx.try_send(packet) {
            self.sync.record_tx_drop();
            debug!("[Transmitter] try_send failed: {:?}", err);
        }
    }
}

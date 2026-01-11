
//! receiver.rs
//! Receives processed sensor packets from Component A and dispatches them to the Controller and actuators.
//! - uses high-performance crossbeam channels for efficient input handling
//! - minimizes delay by immediately timestamping and dispatching packets
//! - decouples receiving logic from actuation work (motors or grippers) to ensure minimal delay 


use crossbeam::channel::Receiver;
use std::{
    sync::Arc,
    time::Instant,
};

use crate::component_a::{
    processor::ProcessedPacket,
    sync_manager::SyncManager,
};

use crate::utils::metrics::{SharedMetrics, push_capped_u64, EventRecorder, Event};

use crate::component_b::{
    controller::Controller,
    multi_actuator::MultiActuator,
    feedback::FeedbackLoop,
};

/// Receiving stage: bridges Processor → Controller → Actuators.
/// Minimizes latency via non-blocking IPC and immediate hand-off.
pub struct Receiving {
    rx: Receiver<ProcessedPacket>,
    controller: Controller,
    multi_actuator: MultiActuator,
    metrics: SharedMetrics,
    event_recorder: Arc<EventRecorder>,
}

impl Receiving {
    pub fn new(
        rx: Receiver<ProcessedPacket>,
        sync: Arc<SyncManager>,
        multi_actuator: MultiActuator,
        feedback_loop: FeedbackLoop,
        metrics: SharedMetrics,
        event_recorder: Arc<EventRecorder>,
    ) -> Self {
        Self {
            rx,
            controller: Controller::new(sync, feedback_loop, metrics.clone(), event_recorder.clone()),
            multi_actuator,
            metrics,
            event_recorder,
        }
    }

    /// Receive and dispatch packets to actuators.
    /// IPC: Crossbeam lock-free channel (non-blocking, bounded queue).
    /// Latency: Immediate timestamp, zero processing, decoupled actuation threads.
    pub fn run(&mut self) {
        while let Ok(packet) = self.rx.recv() {
            // T3: ActuatorReceive event (timestamp on dequeue)
            let t3_ns = self.event_recorder.now_ns();
            self.event_recorder.record(Event::ActuatorReceive {
                seq: packet.seq,
                ts_ns: t3_ns,
            });

            // Measure end-to-end latency (Processor → Receiver)
            let now = Instant::now();
            let latency_us = now.duration_since(packet.timestamp).as_micros() as u64;
            {
                let mut m = match self.metrics.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                push_capped_u64(&mut m.latency_us, latency_us);
            }

            // Fast hand-off: controller + actuators process independently
            self.controller.handle_packet(&packet);
            self.controller.record_rx_latency(latency_us);
            self.multi_actuator.dispatch(packet, self.controller.get_sync().clone());
        }
    }
}
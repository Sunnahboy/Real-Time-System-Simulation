//! feedback.rs
//! Feedback loop: Component B → Component A (sensor/processor recalibration).
//!
//! REQUIREMENT 1: Send feedback (acks, actuator state, error data) to sensor module.
//! REQUIREMENT 2: Enable dynamic recalibration/threshold adjustment via feedback.
//! REQUIREMENT 3: Enforce 0.5 ms feedback deadline.

use crossbeam::channel::{bounded, Sender, Receiver};
use std::{
    time::Instant,
    sync::Arc,
};
use crate::utils::metrics::{EventRecorder, Event};

/// REQUIREMENT 1: Feedback message types (ack, state, error).
#[derive(Debug, Clone)]
pub enum FeedbackKind {
    Ack,                          // Acknowledgement of successful actuation
    ActuatorState(f64),           // Current actuator state (for recalibration)
    Error(&'static str),          // Error indicator (threshold adjustment trigger)
}

#[derive(Debug, Clone)]
pub struct Feedback {
    pub actuator: &'static str,
    pub kind: FeedbackKind,
    pub timestamp: Instant,
    #[allow(dead_code)]
    pub seq: u64,
}

/// Real-time feedback producer: non-blocking send to Component A.
#[derive(Clone)]
pub struct FeedbackLoop {
    tx: Sender<Feedback>,
    event_recorder: Arc<EventRecorder>,
}

impl FeedbackLoop {
    /// bounded feedback channel.
    /// REQUIREMENT 1: Feedback channel for Component A (sensor/processor).
    pub fn new(capacity: usize, event_recorder: Arc<EventRecorder>) -> (Self, Receiver<Feedback>) {
        let (tx, rx) = bounded(capacity);
        (Self { tx, event_recorder }, rx)
    }

    /// Emit feedback without blocking (non-blocking try_send).
    /// 
    /// REQUIREMENT 1: Send actuator state, acks, errors to sensor module.
    /// REQUIREMENT 2: Enable dynamic recalibration (actuator state feedback).
    /// REQUIREMENT 3: Enforce 0.5 ms deadline; flag deadline misses.
    pub fn emit(
        &self,
        actuator: &'static str,
        kind: FeedbackKind,
        actuation_start: Instant,
    ) {
        let seq = 0u64;
        // rts_simulation/src/component_b/feedback.rs
        // ====================================================================
        // REQUIREMENT 3: Feedback deadline (0.5 ms = 500 µs)
        // ====================================================================
        let elapsed_us = actuation_start.elapsed().as_micros();

        let feedback = if elapsed_us > 500 {
            // Deadline miss: override kind with error flag for sensor recalibration
            Feedback {
                actuator,
                kind: FeedbackKind::Error("feedback_deadline_miss"),
                timestamp: Instant::now(),
                seq,
            }
        } else {
            // ================================================================
            // REQUIREMENT 1: Send actuator state & acks for sensor feedback
            // REQUIREMENT 2: Actuator state enables threshold adjustment
            // ================================================================
            Feedback {
                actuator,
                kind,
                timestamp: Instant::now(),
                seq,
            }
        };

        // T5: FeedbackSent event
        let t5_ns = self.event_recorder.now_ns();
        self.event_recorder.record(Event::FeedbackSent {
            seq,
            ts_ns: t5_ns,
        });

        // Non-blocking send (real-time safety)
        let _ = self.tx.try_send(feedback);
    }
}
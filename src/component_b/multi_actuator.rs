//! multi_actuator.rs
//! Manages multiple actuators (gripper, motor, stabiliser) with concurrent dispatch.
//!
//! REQUIREMENT 1: Multiple actuators concurrently (bounded channels, dedicated threads).
//! REQUIREMENT 2: Per-actuator deadline enforcement (2 ms, ThreadPriority::Max).

use crossbeam::channel::{Sender, Receiver, bounded};
use std::{sync::Arc, thread::{self, JoinHandle}, time::{Instant}};
use thread_priority::{ThreadPriority, ThreadBuilderExt};
use crate::{component_a::{
    processor::ProcessedPacket,
    sensor::SensorType,
    sync_manager::SyncManager,
}, };

use crate::component_b::{
    controller::Controller,
    feedback::{FeedbackLoop, FeedbackKind},
};
use crate::utils::metrics::{SharedMetrics, push_capped, EventRecorder,DeadlineComponent};

const ACTUATOR_DEADLINE_US: u64 = 2_000;     // 2 ms deadline per actuator
const CHANNEL_CAPACITY: usize = 8;            // Bounded queue per actuator

/// Routes packets to multiple actuators; each runs in independent priority thread.
pub struct MultiActuator {
    tx_gripper: Sender<ProcessedPacket>,
    tx_motor: Sender<ProcessedPacket>,
    tx_stabiliser: Sender<ProcessedPacket>,
    _handles: Vec<JoinHandle<()>>,
    _feedback: FeedbackLoop,
}

impl MultiActuator {
    /// Create and start all actuator threads with max priority.
    /// REQUIREMENT 1: Three independent channels (gripper, motor, stabiliser).
    /// REQUIREMENT 2: Each thread spawned with ThreadPriority::Max for deadline adherence.
    pub fn new(sync: Arc<SyncManager>, feedback: FeedbackLoop, metrics: SharedMetrics, event_recorder: Arc<EventRecorder>) -> Self {
        // ====================================================================
        // REQUIREMENT 1: Create bounded channels for concurrent packet dispatch
        // ====================================================================
        let (tx_g, rx_g) = bounded(CHANNEL_CAPACITY);
        let (tx_m, rx_m) = bounded(CHANNEL_CAPACITY);
        let (tx_s, rx_s) = bounded(CHANNEL_CAPACITY);

        let mut handles = Vec::new();

        // Spawn independent actuator threads (gripper, motor, stabiliser)
        handles.push(spawn_actuator_thread(
            "Gripper",
            rx_g,
            sync.clone(),
            feedback.clone(),
            metrics.clone(),
            ActuatorType::Gripper,
            event_recorder.clone(),
        ));

        handles.push(spawn_actuator_thread(
            "Motor",
            rx_m,
            sync.clone(),
            feedback.clone(),
            metrics.clone(),
            ActuatorType::Motor,
            event_recorder.clone(),
        ));

        handles.push(spawn_actuator_thread(
            "Stabiliser",
            rx_s,
            sync.clone(),
            feedback.clone(),
            metrics.clone(),
            ActuatorType::Stabiliser,
            event_recorder,
        ));

        Self {
            tx_gripper: tx_g,
            tx_motor: tx_m,
            tx_stabiliser: tx_s,
            _handles: handles,
            _feedback: feedback,
        }
    }

    /// Dispatch processed packet to correct actuator (non-blocking).
    /// REQUIREMENT 1: Route by sensor type (Force→Gripper, Position→Motor, Temperature→Stabiliser).
    pub fn dispatch(&self, pkt: ProcessedPacket, sync: Arc<SyncManager>) {
        let result = match pkt.sensor_type {
            SensorType::Force => self.tx_gripper.try_send(pkt),
            SensorType::Position => self.tx_motor.try_send(pkt),
            SensorType::Temperature => self.tx_stabiliser.try_send(pkt),
        };

        if result.is_err() {
            sync.record_tx_drop();
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ActuatorType {
    Gripper,
    Motor,
    Stabiliser,
}

/// Spawn independent actuator thread with max OS priority.
/// REQUIREMENT 2: Enforce 2 ms deadline; track deadline misses per actuator.

fn spawn_actuator_thread(
    name: &'static str,
    rx: Receiver<ProcessedPacket>,
    sync: Arc<SyncManager>,
    feedback: FeedbackLoop,
    metrics: SharedMetrics,
    actuator_type: ActuatorType,
    event_recorder: Arc<EventRecorder>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name(name.to_string())
        .spawn_with_priority(ThreadPriority::Max, move |_| {
            let mut controller = Controller::new(sync.clone(), feedback.clone(), metrics.clone(), event_recorder.clone());

            while let Ok(pkt) = rx.recv() {
                let cycle_start = Instant::now();
                controller.handle_packet(&pkt);

                let state = controller.current_state();
                
                {
                    let mut m = match metrics.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                     
                    
                    match actuator_type {
                        ActuatorType::Gripper => push_capped(&mut m.gripper, state),
                        ActuatorType::Motor => push_capped(&mut m.motor, state),
                        ActuatorType::Stabiliser => push_capped(&mut m.stabiliser, state),
                    }
                }

                // ====================================================================
                // REQUIREMENT: Per-Component Deadline Tracking
                // Track ACTUATOR execution misses separately (contention/lock blocking)
                // ====================================================================
                let elapsed_us = cycle_start.elapsed().as_micros() as u64;

                if elapsed_us > ACTUATOR_DEADLINE_US {
                    sync.record_proc_miss();

                    {
                        let mut m = match metrics.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        m.record_deadline_miss(DeadlineComponent::Actuator);
                    }
                }

                // Emit feedback: ack on success, error on deadline miss
                if elapsed_us <= 500 {
                    feedback.emit(name, FeedbackKind::Ack, cycle_start);
                    feedback.emit(
                        name,
                        FeedbackKind::ActuatorState(controller.current_state()),
                        cycle_start,
                    );
                } else {
                    feedback.emit(name, FeedbackKind::Error("deadline_miss"), cycle_start);
                }
            }
        })
        .expect("Failed to spawn actuator thread")
}
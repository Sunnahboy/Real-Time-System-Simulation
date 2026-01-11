

//! controller.rs
//! Virtual actuator commander with PID feedback control.
//! 
//! 
//! /*Controller struct is a virtual actuator commander that integrates sensor feedback, 
//! applies PID control, and enforces real-time deadlines. 
//! It simulates actuator dynamics while ensuring stability and responsiveness./* 
//! 
//! 
//! REQUIREMENT 1: Virtual actuator responding to sensor inputs (grip, motor, stabilizer correction).
//! REQUIREMENT 2: PID control algorithm (Kp=1.2, Ki=0.01, Kd=0.2, anti-windup).
//! REQUIREMENT 3: Real-time scheduling (2 ms deadline enforcement, deadline miss tracking).

use std::{
    sync::Arc, 
    time::Instant, 
}; 

use pidgeon::{ControllerConfig, PidController};

use crate::component_a::{
    processor::ProcessedPacket,
    sensor::SensorType,
    sync_manager::SyncManager,
};
use crate::component_b::feedback::{FeedbackLoop, FeedbackKind};
use crate::utils::metrics::{SharedMetrics, push_capped, EventRecorder, Event};

/// Virtual actuator controller: maintains state, computes PID control signals.
pub struct Controller {
    pid: PidController,
    last_update: Instant,
    current_target: f64,
    actuator_state: f64,      // Virtual actuator state (integration of control signals)
    sync: Arc<SyncManager>,
    deadline_us: u64,
    feedback: FeedbackLoop,
    metrics: SharedMetrics,
    event_recorder: Arc<EventRecorder>,
}

impl Controller {
    /// Create controller with PID gains (Kp=1.2, Ki=0.01, Kd=0.2) and anti-windup.
    /// REQUIREMENT 2: Predictive control algorithm configured at initialization.
    pub fn new(
        sync: Arc<SyncManager>, 
        feedback: FeedbackLoop, 
        metrics: SharedMetrics,
        event_recorder: Arc<EventRecorder>,
    ) -> Self {
        // PID configuration: Kp=1.2 (proportional), Ki=0.01 (integral), Kd=0.2 (derivative)
        let config = ControllerConfig::new()
            .with_kp(1.2)
            .with_ki(0.01)
            .with_kd(0.2)
            .with_output_limits(-50.0, 50.0)
            .with_anti_windup(true);//prevents runaway intergral term

        Self {
            pid: PidController::new(config),
            last_update: Instant::now(),
            current_target: 0.0,
            actuator_state: 0.0,
            sync,
            deadline_us: 2_000,
            feedback,
            metrics,
            event_recorder,
        }
    }

    pub fn record_rx_latency(&self, latency_us: u64) {
        self.sync.record_rx_latency(latency_us);
    }

    #[inline]
    pub fn current_state(&self) -> f64 {
        self.actuator_state
    }

    /// Process sensor packet: compute PID control signal, update actuator state.
    /// Detects sensor anomalies, enforces 2 ms deadline, emits feedback events.
    pub fn handle_packet(&mut self, pkt: &ProcessedPacket) {
        let cycle_start = Instant::now();

        // Compute time step (clamped for stability)
        let now = Instant::now();
        let mut dt = now.duration_since(self.last_update).as_secs_f64();
        self.last_update = now;
        dt = dt.clamp(1e-6, 0.05);//clamped btwn 1e-6 to 0.05 for stability

        // ====================================================================
        // REQUIREMENT 1: Virtual Actuator Responding to Sensor Input
        // ====================================================================
        // Anomaly detection: raw vs. filtered divergence indicates instability
        if (pkt.raw - pkt.filtered).abs() > 10.0 {
            self.sync.record_custom(900);
            self.feedback.emit(
                "Controller",
                FeedbackKind::Error("unstable_sensor"),
                cycle_start,
            );
        }

        // Update setpoint based on sensor type (e.g, grip force, position correction)
        // ensures the actuator reacts differently
        let new_target = match pkt.sensor_type {
            SensorType::Force => 100.0,
            SensorType::Position => 0.0,
            SensorType::Temperature => 25.0,
        };

        if (new_target - self.current_target).abs() > f64::EPSILON {
            if let Err(_) = self.pid.set_setpoint(new_target) {
                self.sync.record_proc_miss();
                self.feedback.emit(
                    "Controller",
                    FeedbackKind::Error("pid_config_failed"),
                    cycle_start,
                );
            }
            self.current_target = new_target;
        }
      

        // ====================================================================
        // REQUIREMENT 2: Predictive Control Algorithm (PID)
        // ====================================================================
        // Compute PID control signal: adjusts actuation dynamically
        let control_signal = self.pid.compute(pkt.filtered, dt);

        // ====================================================================
        // REQUIREMENT 1: Virtual Actuator State Integration
        // ====================================================================
        // Apply control signal to virtual actuator (simulates grip, motor, stabilizer)
        self.apply_to_actuator(control_signal);

        // T4: ControllerComplete event (after control computation)
        let exec_us = cycle_start.elapsed().as_micros() as u64;
        let t4_ns = self.event_recorder.now_ns();
        self.event_recorder.record(Event::ControllerComplete {
            seq: pkt.seq,
            ts_ns: t4_ns,
            control_output: control_signal,
            exec_us,
        });

        // ====================================================================
        // REQUIREMENT 3: Real-Time Scheduling (Deadline Enforcement)
        // ====================================================================
        // Enforce 2 ms deadline; prioritize critical actuator tasks
        let elapsed_us = cycle_start.elapsed().as_micros() as u64;
        if elapsed_us > self.deadline_us {
            self.sync.record_proc_miss();
            self.feedback.emit(
                "Controller",
                FeedbackKind::Error("deadline_miss"),
                cycle_start,
            );
        } else {
            self.feedback.emit(
                "Controller",
                FeedbackKind::ActuatorState(self.actuator_state),
                cycle_start,
            );
        }
    }

    pub fn get_sync(&self) -> &Arc<SyncManager> {
        &self.sync
    }

    /// Virtual actuator dynamics: integrate control signal into state.
    /// Simulates physical response (grip, motor, stabilizer correction).
    /// Updates metrics for monitoring.
    fn apply_to_actuator(&mut self, control_signal: f64) {
        self.actuator_state += control_signal;

        {
            let mut m = match self.metrics.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            
            push_capped(&mut m.gripper, self.actuator_state);
            push_capped(&mut m.motor, self.actuator_state);
        }
    }
}